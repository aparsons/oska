//! The memory run limit on the interpreter (`--max-memory`, spec #446):
//! the bytes of the values a program makes, net of those it lets go of,
//! with a panic (`P0040`) before a value that would take them past the
//! limit is made.
//!
//! **What counts.** Each text, list, struct or enum value the program
//! builds, and each closure: its bytes as this module reckons them
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
//! since the last sweep (so it never grows without end).
//!
//! **No cost without the flag.** The meter is a thread-local, the
//! interpreter's thread's, set only for a run under `--max-memory`
//! ([`Metering`]); every counted site first reads one thread-local flag.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
    Closure(Weak<ClosureValue>),
}

impl Held {
    fn alive(&self) -> bool {
        match self {
            Held::Text(weak) => weak.strong_count() > 0,
            Held::Items(weak) => weak.strong_count() > 0,
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
    /// How many entries there were after the last sweep.
    swept: usize,
}

impl Meter {
    /// Drop the entries of values that are gone, and count the rest.
    fn sweep(&mut self) {
        self.entries.retain(|entry| entry.held.alive());
        self.items.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            if let Held::Items(weak) = &entry.held {
                self.items.insert(weak.as_ptr() as usize, index);
            }
        }
        self.live = self.entries.iter().map(|entry| entry.bytes).sum();
        self.swept = self.entries.len();
    }

    fn record(&mut self, held: Held, bytes: u64) {
        if let Held::Items(weak) = &held {
            self.items
                .insert(weak.as_ptr() as usize, self.entries.len());
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
            meter.sweep();
            if meter.live.saturating_add(bytes) > meter.limit {
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

/// `text` as a text value, made at `span`.
#[inline]
pub(crate) fn text(text: &str, span: Span) -> Result<Value, Panic> {
    if metered() {
        return text_metered(text, span);
    }
    Ok(Value::str(text))
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
