// SPDX-License-Identifier: Apache-2.0
//! The memory run limit on the interpreter (`--max-memory`, spec #446):
//! the bytes of the values a program makes, net of those it lets go of,
//! with a panic (`P0040`) before a value that would take them past the
//! limit is made.
//!
//! **What counts.** Each text, list, struct or enum value the program
//! builds, each `Bytes`, each closure, and each struct the prelude makes for it (a
//! `Range`, `Secret`, `Path`, `IoError`, or `std.html` markup): its bytes as this module reckons them
//! ([`text_bytes`] and the like), which are the bytes the interpreter
//! allocates for it, its record here included. A list that grows in
//! place counts its new capacity. The small `Option` and `Result` values
//! the prelude's functions return are not counted, but what they hold
//! is.
//!
//! **Checked first.** Every counted value is charged ([`charge`]) before
//! it is made, with the bytes it is about to take, so a value that would
//! pass the limit is never made: `"x".repeat(200000000)` under a 64 MiB
//! limit panics instead of allocating.
//!
//! **Net of frees.** The meter keeps a weak reference to each counted
//! value ([`Held`]). A value dropped since is found when the meter is
//! next swept: whenever a charge would pass the limit (so a panic is
//! decided on the exact bytes held), and whenever the record has doubled
//! since the last sweep (so it never grows without end). A sweep that
//! frees less than an eighth of the limit stops the program too
//! ([`charge`]), so sweeps never run back to back.
//!
//! **No cost without the flag.** The meter is a thread-local, the
//! interpreter's thread's, set only for a run under `--max-memory`
//! ([`Metering`]); every counted site first reads one thread-local flag.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::{Rc, Weak};

use oska_check::limits::Limit;
use oska_check::program::Construct;
use oska_syntax::Span;

use crate::panics::Panic;
use crate::value::{ClosureValue, Value};

thread_local! {
    /// Whether this thread runs a program under `--max-memory`.
    static METERED: Cell<bool> = const { Cell::new(false) };
    /// The meter, while [`METERED`].
    static METER: RefCell<Option<Meter>> = const { RefCell::new(None) };
}

/// A counted value, held weakly: it is gone once nothing else holds it.
enum Held {
    Text(Weak<str>),
    Items(Weak<Vec<Value>>),
    Bytes(Weak<Vec<u8>>),
    Closure(Weak<ClosureValue>),
}

impl Held {
    fn alive(&self) -> bool {
        match self {
            Held::Text(weak) => weak.strong_count() > 0,
            Held::Items(weak) => weak.strong_count() > 0,
            Held::Bytes(weak) => weak.strong_count() > 0,
            Held::Closure(weak) => weak.strong_count() > 0,
        }
    }
}

/// One counted value and its bytes.
struct Entry {
    held: Held,
    bytes: u64,
}

struct Meter {
    limit: u64,
    /// The bytes of every entry, and of charges about to be recorded.
    live: u64,
    entries: Vec<Entry>,
    /// The entry of each counted list or field list, by its address: a
    /// list that grows in place, or moves when it is written to, keeps
    /// its entry. (A weak reference keeps the address taken, so it names
    /// one list while its entry lives.)
    items: HashMap<usize, usize>,
    /// The entry of each counted `Bytes`, by its address, as for lists.
    buffers: HashMap<usize, usize>,
    /// How many entries there were after the last sweep.
    swept: usize,
}

impl Meter {
    /// Drop the entries of values that are gone, and count the rest.
    fn sweep(&mut self) {
        self.entries.retain(|entry| entry.held.alive());
        self.items.clear();
        self.buffers.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            match &entry.held {
                Held::Items(weak) => {
                    self.items.insert(weak.as_ptr() as usize, index);
                }
                Held::Bytes(weak) => {
                    self.buffers.insert(weak.as_ptr() as usize, index);
                }
                Held::Text(_) | Held::Closure(_) => {}
            }
        }
        self.live = self.entries.iter().map(|entry| entry.bytes).sum();
        self.swept = self.entries.len();
    }

    fn record(&mut self, held: Held, bytes: u64) {
        match &held {
            Held::Items(weak) => {
                self.items
                    .insert(weak.as_ptr() as usize, self.entries.len());
            }
            Held::Bytes(weak) => {
                self.buffers
                    .insert(weak.as_ptr() as usize, self.entries.len());
            }
            Held::Text(_) | Held::Closure(_) => {}
        }
        self.entries.push(Entry { held, bytes });
    }
}

/// The meter of one run under `--max-memory`, on this thread, until it
/// is dropped.
pub(crate) struct Metering(());

impl Metering {
    /// Count this thread's values against `limit` bytes.
    pub(crate) fn start(limit: u64) -> Metering {
        METER.with(|meter| {
            *meter.borrow_mut() = Some(Meter {
                limit,
                live: 0,
                entries: Vec::new(),
                items: HashMap::new(),
                buffers: HashMap::new(),
                swept: 0,
            });
        });
        METERED.with(|metered| metered.set(true));
        Metering(())
    }
}

impl Drop for Metering {
    fn drop(&mut self) {
        METERED.with(|metered| metered.set(false));
        METER.with(|meter| *meter.borrow_mut() = None);
    }
}

/// Whether this thread runs under `--max-memory`: the one check every
/// counted site makes without it.
#[inline]
pub(crate) fn metered() -> bool {
    METERED.with(Cell::get)
}

/// Charge `bytes` that the program is about to allocate at `span`:
/// `P0040` instead when they would take what it holds past the limit.
///
/// A charge that would pass the limit first sweeps the record. If the
/// bytes still do not fit, or the sweep freed less than an eighth of the
/// limit, it is `P0040`: a program that holds nearly all of its limit
/// and keeps allocating stops then, instead of sweeping its whole record
/// again at every charge (each sweep is paid for by an eighth of the
/// limit allocated since).
fn charge(bytes: u64, span: Span) -> Result<(), Panic> {
    METER.with(|meter| {
        let mut meter = meter.borrow_mut();
        let Some(meter) = meter.as_mut() else {
            return Ok(());
        };
        if meter.entries.len() > 2 * meter.swept + 4096 {
            meter.sweep();
        }
        if meter.live.saturating_add(bytes) > meter.limit {
            let before = meter.live;
            meter.sweep();
            let freed = before.saturating_sub(meter.live);
            if meter.live.saturating_add(bytes) > meter.limit || freed < meter.limit / SLACK {
                return Err(Panic::new(
                    Limit::Memory.code(),
                    Limit::Memory.message(meter.limit),
                    span,
                ));
            }
        }
        meter.live += bytes;
        Ok(())
    })
}

/// Record a value made after [`charge`] took its `bytes`.
fn record(held: Held, bytes: u64) {
    METER.with(|meter| {
        if let Some(meter) = meter.borrow_mut().as_mut() {
            meter.record(held, bytes);
        }
    });
}

/// A sweep that frees less than this part of the limit stops the
/// program ([`charge`]).
const SLACK: u64 = 8;

/// The bytes of one [`Entry`], which each counted value also takes.
const ENTRY: u64 = (std::mem::size_of::<Entry>() + 2 * std::mem::size_of::<usize>()) as u64;

/// The counts an `Rc` keeps beside its value.
const RC: u64 = 2 * std::mem::size_of::<usize>() as u64;

/// What a text of `len` bytes takes.
pub(crate) fn text_bytes(len: usize) -> u64 {
    ENTRY + RC + len as u64
}

/// What a list of `capacity` items takes.
fn items_bytes(capacity: usize) -> u64 {
    ENTRY
        + RC
        + std::mem::size_of::<Vec<Value>>() as u64
        + (capacity as u64).saturating_mul(std::mem::size_of::<Value>() as u64)
}

/// What a `Bytes` with room for `capacity` bytes takes.
fn buffer_bytes(capacity: usize) -> u64 {
    ENTRY + RC + std::mem::size_of::<Vec<u8>>() as u64 + capacity as u64
}

/// The `Bytes` `make` builds, of `len` bytes, made at `span`: charged
/// before `make` runs, so a buffer that would pass the limit is never
/// built.
#[inline]
pub(crate) fn bytes_of_len(
    len: usize,
    span: Span,
    make: impl FnOnce() -> Vec<u8>,
) -> Result<Value, Panic> {
    if metered() {
        return bytes_of_len_metered(len, span, make);
    }
    Ok(Value::Bytes(Rc::new(make())))
}

#[cold]
#[inline(never)]
fn bytes_of_len_metered(
    len: usize,
    span: Span,
    make: impl FnOnce() -> Vec<u8>,
) -> Result<Value, Panic> {
    let charged = buffer_bytes(len);
    charge(charged, span)?;
    let data = Rc::new(make());
    let bytes = buffer_bytes(data.capacity());
    METER.with(|meter| {
        if let Some(meter) = meter.borrow_mut().as_mut() {
            meter.live = meter.live.saturating_add(bytes).saturating_sub(charged);
            meter.record(Held::Bytes(Rc::downgrade(&data)), bytes);
        }
    });
    Ok(Value::Bytes(data))
}

/// `write` applied to the bytes of `buffer`, at `span`, which adds
/// `extra` bytes at the end: the buffer is copied first when it is
/// shared, as [`Rc::make_mut`] does. Under the limit the growth, or the
/// copy, is charged first.
#[inline]
pub(crate) fn write_bytes(
    buffer: &mut Rc<Vec<u8>>,
    extra: usize,
    span: Span,
    write: impl FnOnce(&mut Vec<u8>),
) -> Result<(), Panic> {
    if metered() {
        return write_bytes_metered(buffer, extra, span, write);
    }
    write(Rc::make_mut(buffer));
    Ok(())
}

#[cold]
#[inline(never)]
fn write_bytes_metered(
    buffer: &mut Rc<Vec<u8>>,
    extra: usize,
    span: Span,
    write: impl FnOnce(&mut Vec<u8>),
) -> Result<(), Panic> {
    let len = buffer.len();
    let capacity = buffer.capacity();
    let needed = len.saturating_add(extra);
    // The capacity it grows to, which is reserved exactly.
    let wanted = if needed <= capacity {
        capacity
    } else {
        capacity.saturating_mul(2).max(needed).max(8)
    };
    if Rc::strong_count(buffer) > 1 {
        // Shared: a copy of its own, with room for what is added.
        let bytes = buffer_bytes(wanted);
        charge(bytes, span)?;
        let mut copy = Vec::with_capacity(wanted);
        copy.extend_from_slice(buffer);
        write(&mut copy);
        *buffer = Rc::new(copy);
        record(Held::Bytes(Rc::downgrade(buffer)), bytes);
        return Ok(());
    }
    let grown = buffer_bytes(wanted) - buffer_bytes(capacity);
    charge(grown, span)?;
    let before = Rc::as_ptr(buffer) as usize;
    let data = Rc::make_mut(buffer);
    data.reserve_exact(wanted - len);
    write(data);
    let actual = buffer_bytes(data.capacity()) - buffer_bytes(capacity);
    METER.with(|meter| {
        let mut meter = meter.borrow_mut();
        let Some(meter) = meter.as_mut() else {
            return;
        };
        meter.live = meter.live.saturating_add(actual).saturating_sub(grown);
        let after = Rc::as_ptr(buffer) as usize;
        match meter.buffers.remove(&before) {
            Some(index) => {
                let entry = &mut meter.entries[index];
                entry.held = Held::Bytes(Rc::downgrade(buffer));
                entry.bytes += actual;
                meter.buffers.insert(after, index);
            }
            // A buffer made before counting started: from now on it is
            // counted.
            None => {
                let bytes = buffer_bytes(buffer.capacity());
                meter.live = meter.live.saturating_add(bytes).saturating_sub(actual);
                meter.record(Held::Bytes(Rc::downgrade(buffer)), bytes);
            }
        }
    });
    Ok(())
}

/// `text` as a text value, made at `span`.
#[inline]
pub(crate) fn text(text: &str, span: Span) -> Result<Value, Panic> {
    if metered() {
        return text_metered(text, span);
    }
    Ok(Value::str(text))
}

/// `part`, a piece of the text value `whole`, as a text value made at
/// `span`: `whole` itself, shared, when `part` is all of it (#572), and
/// otherwise a new text. Under `--max-memory` it is always a new text,
/// charged as [`text`] charges it.
#[inline]
pub(crate) fn text_within(whole: &Value, part: &str, span: Span) -> Result<Value, Panic> {
    if !metered() && part.len() == whole.as_str().len() {
        return Ok(whole.clone());
    }
    text(part, span)
}

#[cold]
#[inline(never)]
fn text_metered(text: &str, span: Span) -> Result<Value, Panic> {
    let bytes = text_bytes(text.len());
    charge(bytes, span)?;
    let text: Rc<str> = Rc::from(text);
    record(Held::Text(Rc::downgrade(&text)), bytes);
    Ok(Value::Str(text))
}

/// Texts kept by the address of the `&str` they were made from, each
/// made once and then shared: the string literals of [`Literals`] and the
/// type and variant names of [`shared_name`].
///
/// The address only finds the text: a text whose bytes differ (an
/// address reused for another text) is made again, so the text given is
/// always the one asked for.
#[derive(Default)]
struct AddressTexts {
    texts: HashMap<usize, Rc<str>, BuildHasherDefault<AddressHasher>>,
}

impl AddressTexts {
    /// `text`, shared.
    #[inline]
    fn shared(&mut self, text: &str) -> Rc<str> {
        let kept = self
            .texts
            .entry(text.as_ptr() as usize)
            .or_insert_with(|| Rc::from(text));
        if **kept != *text {
            *kept = Rc::from(text);
        }
        Rc::clone(kept)
    }
}

/// The texts of the string literals a run has evaluated, each made once
/// (#500): evaluating a literal again shares its text instead of copying
/// it into a new one. Under `--max-memory` each evaluation still makes
/// and charges its own text ([`text`]), so a metered run is charged, and
/// gets bytes back, exactly as when every evaluation copied.
#[derive(Default)]
pub(crate) struct Literals {
    /// Each literal's text, by the address of the literal in the program.
    texts: AddressTexts,
}

impl Literals {
    /// The string literal `literal` as a text value, made at `span`.
    #[inline]
    pub(crate) fn text(&mut self, literal: &str, span: Span) -> Result<Value, Panic> {
        if metered() {
            return text_metered(literal, span);
        }
        Ok(Value::Str(self.texts.shared(literal)))
    }
}

thread_local! {
    /// The type and variant names [`shared_name`] has made on this
    /// thread, while a run is under way here ([`Names`]).
    static NAMES: RefCell<Option<AddressTexts>> = const { RefCell::new(None) };
}

/// The names [`shared_name`] shares, for as long as one run's
/// [`crate::interp::Interpreter`] lives on this thread: the first one on
/// a thread starts them, and when the last one ends they are let go of,
/// so a host that runs many programs on one thread keeps none between
/// runs.
pub(crate) struct Names {
    /// Whether this one started them, and so ends them.
    first: bool,
}

impl Names {
    pub(crate) fn start() -> Names {
        NAMES.with(|names| {
            let mut names = names.borrow_mut();
            let first = names.is_none();
            if first {
                *names = Some(AddressTexts::default());
            }
            Names { first }
        })
    }
}

impl Drop for Names {
    fn drop(&mut self) {
        if self.first {
            // A thread being torn down may have let go of them already.
            let _ = NAMES.try_with(|names| names.borrow_mut().take());
        }
    }
}

/// The type or variant name `name` as an `Rc<str>`, made once in a run
/// and then shared (#572): a struct or variant value holds its type's
/// name, and making a value would otherwise copy it. Outside a run
/// ([`Names`]) each name is made anew. Nothing is charged for a name
/// under `--max-memory` by its `Rc`, only by its length, so sharing it
/// changes no charge.
pub(crate) fn shared_name(name: &str) -> Rc<str> {
    NAMES
        .try_with(|names| names.borrow_mut().as_mut().map(|names| names.shared(name)))
        .ok()
        .flatten()
        .unwrap_or_else(|| Rc::from(name))
}

/// Hashes the addresses that key [`AddressTexts`]: one multiply, where the
/// standard hasher would hash eight bytes with SipHash on every literal.
#[derive(Default)]
struct AddressHasher(u64);

impl Hasher for AddressHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(byte);
        }
    }

    fn write_usize(&mut self, address: usize) {
        self.0 = address as u64;
    }

    fn finish(&self) -> u64 {
        // Fibonacci hashing, folded so that the low bits, which pick the
        // bucket, depend on every bit of the address.
        let mixed = self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        mixed ^ (mixed >> 32)
    }
}

/// The text `make` builds, of `len` bytes, made at `span`: charged
/// before `make` runs, so a text that would pass the limit is never
/// built.
#[inline]
pub(crate) fn text_of_len(
    len: usize,
    span: Span,
    make: impl FnOnce() -> String,
) -> Result<Value, Panic> {
    if metered() {
        return text_of_len_metered(len, span, make);
    }
    Ok(Value::str(&make()))
}

#[cold]
#[inline(never)]
fn text_of_len_metered(
    len: usize,
    span: Span,
    make: impl FnOnce() -> String,
) -> Result<Value, Panic> {
    charge(text_bytes(len), span)?;
    let made = make();
    let bytes = text_bytes(made.len());
    let text: Rc<str> = Rc::from(made);
    record(Held::Text(Rc::downgrade(&text)), bytes);
    Ok(Value::Str(text))
}

/// `items` as a list value, made at `span`.
#[inline]
pub(crate) fn list(items: Vec<Value>, span: Span) -> Result<Value, Panic> {
    if metered() {
        return list_metered(items, span);
    }
    Ok(Value::List(Rc::new(items)))
}

#[cold]
#[inline(never)]
fn list_metered(items: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let bytes = items_bytes(items.capacity());
    charge(bytes, span)?;
    let items = Rc::new(items);
    record(Held::Items(Rc::downgrade(&items)), bytes);
    Ok(Value::List(items))
}

/// The list `make` builds, of `len` items, made at `span`. Under
/// `--max-memory` its room is charged before `make` runs, so a list that
/// would pass the limit is never built; its items are charged as they
/// are made.
#[inline]
pub(crate) fn list_of_len(
    len: usize,
    span: Span,
    make: impl FnOnce() -> Result<Vec<Value>, Panic>,
) -> Result<Value, Panic> {
    if metered() {
        return list_of_len_metered(len, span, make);
    }
    Ok(Value::List(Rc::new(make()?)))
}

#[cold]
#[inline(never)]
fn list_of_len_metered(
    len: usize,
    span: Span,
    make: impl FnOnce() -> Result<Vec<Value>, Panic>,
) -> Result<Value, Panic> {
    let charged = items_bytes(len);
    charge(charged, span)?;
    let items = Rc::new(make()?);
    let bytes = items_bytes(items.capacity());
    METER.with(|meter| {
        if let Some(meter) = meter.borrow_mut().as_mut() {
            meter.live = meter.live.saturating_add(bytes).saturating_sub(charged);
            meter.record(Held::Items(Rc::downgrade(&items)), bytes);
        }
    });
    Ok(Value::List(items))
}

/// What `ctor` builds from `fields`, at `span`.
#[inline]
pub(crate) fn construct(ctor: &Construct, fields: Vec<Value>, span: Span) -> Result<Value, Panic> {
    if metered() {
        return construct_metered(ctor, fields, span);
    }
    Ok(Value::construct(ctor, fields))
}

#[cold]
#[inline(never)]
fn construct_metered(ctor: &Construct, fields: Vec<Value>, span: Span) -> Result<Value, Panic> {
    // The fields, and the type's and the variant's names.
    let names = ctor.ty.len() + ctor.variant.as_ref().map_or(0, String::len);
    let bytes = items_bytes(fields.capacity()) + 2 * RC + names as u64;
    charge(bytes, span)?;
    let value = Value::construct(ctor, fields);
    if let Value::Struct { fields, .. } | Value::Variant { fields, .. } = &value {
        record(Held::Items(Rc::downgrade(fields)), bytes);
    }
    Ok(value)
}

/// A struct of the prelude's type `ty` holding `fields`, made at `span`:
/// a `Range`, `Secret`, `Path`, `IoError`, or `std.html` markup.
#[inline]
pub(crate) fn structure(ty: &str, fields: Vec<Value>, span: Span) -> Result<Value, Panic> {
    if metered() {
        return structure_metered(ty, fields, span);
    }
    Ok(Value::Struct {
        ty: shared_name(ty),
        fields: Rc::new(fields),
    })
}

#[cold]
#[inline(never)]
fn structure_metered(ty: &str, fields: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let bytes = items_bytes(fields.capacity()) + RC + ty.len() as u64;
    charge(bytes, span)?;
    let fields = Rc::new(fields);
    record(Held::Items(Rc::downgrade(&fields)), bytes);
    Ok(Value::Struct {
        ty: shared_name(ty),
        fields,
    })
}

/// A `Secret` or `Path` (the prelude's type `ty`) wrapping `value`, made
/// at `span`.
pub(crate) fn wrap(ty: &str, value: Value, span: Span) -> Result<Value, Panic> {
    structure(ty, vec![value], span)
}

/// An `IoError` with `message`, made at `span`.
pub(crate) fn io_error(message: &str, span: Span) -> Result<Value, Panic> {
    let message = text(message, span)?;
    structure(oska_check::builtins::IO_ERROR, vec![message], span)
}

/// `start..end`, a `Range`, made at `span`.
pub(crate) fn range(start: Value, end: Value, span: Span) -> Result<Value, Panic> {
    structure(oska_check::builtins::RANGE, vec![start, end], span)
}

/// `closure` as a function value, made at `span`.
#[inline]
pub(crate) fn closure(closure: ClosureValue, span: Span) -> Result<Value, Panic> {
    if metered() {
        return closure_metered(closure, span);
    }
    Ok(Value::Closure(Rc::new(closure)))
}

#[cold]
#[inline(never)]
fn closure_metered(closure: ClosureValue, span: Span) -> Result<Value, Panic> {
    let bytes = ENTRY
        + RC
        + std::mem::size_of::<ClosureValue>() as u64
        + (closure.captured.capacity() * std::mem::size_of::<Value>()) as u64;
    charge(bytes, span)?;
    let closure = Rc::new(closure);
    record(Held::Closure(Rc::downgrade(&closure)), bytes);
    Ok(Value::Closure(closure))
}

/// `item` added at the end of `items`, at `span`, copying them first when
/// they are shared, as `List.push` does. Under the limit the growth is
/// charged first.
#[inline]
pub(crate) fn push(items: &mut Rc<Vec<Value>>, item: Value, span: Span) -> Result<(), Panic> {
    if metered() {
        return push_metered(items, item, span);
    }
    Rc::make_mut(items).push(item);
    Ok(())
}

/// `change` applied to `items` held by this value alone, as `List.set`,
/// `pop` and `truncate` need: copied first when shared. Under the limit a
/// copy is charged for its length, as `push` charges one, and a list that
/// only this value holds charges nothing more.
#[inline]
pub(crate) fn unshared<R>(
    items: &mut Rc<Vec<Value>>,
    span: Span,
    change: impl FnOnce(&mut Vec<Value>) -> R,
) -> Result<R, Panic> {
    if metered() {
        return unshared_metered(items, span, change);
    }
    Ok(change(Rc::make_mut(items)))
}

#[cold]
#[inline(never)]
fn unshared_metered<R>(
    items: &mut Rc<Vec<Value>>,
    span: Span,
    change: impl FnOnce(&mut Vec<Value>) -> R,
) -> Result<R, Panic> {
    if Rc::strong_count(items) > 1 {
        let bytes = items_bytes(items.len());
        charge(bytes, span)?;
        let mut copy = Vec::with_capacity(items.len());
        copy.extend(items.iter().cloned());
        // Changed before the meter holds it weakly: `Rc::get_mut` refuses
        // a value that is held weakly.
        let mut own = Rc::new(copy);
        let result = change(Rc::get_mut(&mut own).expect("a new list is held once"));
        *items = own;
        record(Held::Items(Rc::downgrade(items)), bytes);
        return Ok(result);
    }
    // Held by the meter weakly: `make_mut` may move it to a new address.
    let before = Rc::as_ptr(items) as usize;
    let result = change(Rc::make_mut(items));
    moved(before, items, 0, 0);
    Ok(result)
}

#[cold]
#[inline(never)]
fn push_metered(items: &mut Rc<Vec<Value>>, item: Value, span: Span) -> Result<(), Panic> {
    let len = items.len();
    let capacity = items.capacity();
    // The capacity `Vec` would grow to, which is reserved exactly.
    let wanted = if len < capacity {
        capacity
    } else {
        capacity.saturating_mul(2).max(len + 1).max(4)
    };
    if Rc::strong_count(items) > 1 {
        // Shared: a copy of its own, with room for the item.
        let bytes = items_bytes(wanted);
        charge(bytes, span)?;
        let mut copy = Vec::with_capacity(wanted);
        copy.extend(items.iter().cloned());
        copy.push(item);
        *items = Rc::new(copy);
        record(Held::Items(Rc::downgrade(items)), bytes);
        return Ok(());
    }
    let grown = items_bytes(wanted) - items_bytes(capacity);
    charge(grown, span)?;
    let before = Rc::as_ptr(items) as usize;
    let list = Rc::make_mut(items);
    list.reserve_exact(wanted - len);
    list.push(item);
    let actual = items_bytes(list.capacity()) - items_bytes(capacity);
    moved(before, items, actual, grown);
    Ok(())
}

/// After a write through a struct's fields at `before`, which `shared`
/// says were shared then, and which are `fields` now: writing a field
/// makes them the struct's own ([`Rc::make_mut`]), as a copy when they
/// were shared, else at a new address when the meter held them weakly.
/// A copy is counted (too small to be worth a panic of its own: the next
/// charge decides); a move keeps its entry.
pub(crate) fn relocated(before: usize, shared: bool, fields: &Rc<Vec<Value>>) {
    if Rc::as_ptr(fields) as usize == before {
        return;
    }
    if !shared {
        moved(before, fields, 0, 0);
        return;
    }
    METER.with(|meter| {
        if let Some(meter) = meter.borrow_mut().as_mut() {
            let bytes = items_bytes(fields.capacity());
            meter.live = meter.live.saturating_add(bytes);
            meter.record(Held::Items(Rc::downgrade(fields)), bytes);
        }
    });
}

/// After `items`, held only by its owner (and the meter's weak
/// reference), was written to: it may have moved to a new address, which
/// its entry follows, and it took `actual` bytes more where `charged`
/// were charged.
fn moved(before: usize, items: &Rc<Vec<Value>>, actual: u64, charged: u64) {
    METER.with(|meter| {
        let mut meter = meter.borrow_mut();
        let Some(meter) = meter.as_mut() else {
            return;
        };
        meter.live = meter.live.saturating_add(actual).saturating_sub(charged);
        let after = Rc::as_ptr(items) as usize;
        match meter.items.remove(&before) {
            Some(index) => {
                let entry = &mut meter.entries[index];
                entry.held = Held::Items(Rc::downgrade(items));
                entry.bytes += actual;
                meter.items.insert(after, index);
            }
            // A list made before counting started, or by the prelude:
            // from now on it is counted.
            None => {
                let bytes = items_bytes(items.capacity());
                meter.live = meter.live.saturating_add(bytes).saturating_sub(actual);
                meter.record(Held::Items(Rc::downgrade(items)), bytes);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{shared_name, Names};
    use std::rc::Rc;

    #[test]
    fn names_are_shared_only_while_a_run_is_under_way() {
        const NAME: &str = "Point";
        let outside = (shared_name(NAME), shared_name(NAME));
        assert!(!Rc::ptr_eq(&outside.0, &outside.1), "no run: made anew");

        let run = Names::start();
        let nested = Names::start();
        let (first, second) = (shared_name(NAME), shared_name(NAME));
        assert!(Rc::ptr_eq(&first, &second), "in a run: shared");
        drop(nested);
        assert!(
            Rc::ptr_eq(&first, &shared_name(NAME)),
            "the outer run keeps them"
        );
        drop(run);
        // Only the values hold the name now: the thread let go of it.
        assert_eq!(Rc::strong_count(&first), 2);
        assert!(
            !Rc::ptr_eq(&first, &shared_name(NAME)),
            "after the run: made anew"
        );
    }
}
