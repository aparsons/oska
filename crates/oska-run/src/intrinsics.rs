// SPDX-License-Identifier: Apache-2.0
//! Runtime implementations of body-less prelude functions, keyed by the
//! qualified name the checked program calls them by (`Stdout.println`).
//!
//! To add one: write a function with the signature of one of the
//! [`Intrinsic`] kinds and add it, wrapped in its kind, to its module's
//! table ([`INTRINSICS`] here), and a new module's table to `TABLES`.
//! [`lookup`] finds an intrinsic by name in an index of every table,
//! made once; [`resolved`] finds a call site's once and keeps it there. A
//! method's receiver is its first argument.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

use oska_check::builtins;
use oska_check::limits;
use oska_check::program::IntrinsicName;
use oska_syntax::Span;

use crate::approval;
use crate::budget;
use crate::bytes;
use crate::confine::{self, Intent};
use crate::data;
use crate::host::{Host, RootHandle};
use crate::html;
use crate::int_methods;
use crate::interp::{Interpreter, Stream};
use crate::memory;
use crate::ops;
use crate::panics::{codes, Panic};
use crate::secrets;
use crate::tasks;
use crate::testing;
use crate::traits;
use crate::value::{Access, Capability, FsCap, Root, Value};

use Intrinsic::{HigherOrder, Plain};

/// A body-less prelude function's runtime implementation. Each gets the
/// arguments, the receiver first for a method, and the call's span, for
/// panics, and what else its kind needs.
/// An intrinsic call's arguments, the receiver first for a method. The
/// interpreter passes them in a buffer it reuses, so a call allocates no
/// `Vec` of its own (#505): an intrinsic takes what it keeps out of it
/// (`swap_remove`, `pop`, `std::mem::take` of an item), and what it
/// leaves is dropped when the call returns, as it was when the intrinsic
/// owned its arguments.
pub struct Args(pub(crate) Vec<Value>);

impl std::ops::Deref for Args {
    type Target = Vec<Value>;

    fn deref(&self) -> &Vec<Value> {
        &self.0
    }
}

impl std::ops::DerefMut for Args {
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        &mut self.0
    }
}

#[derive(Clone, Copy)]
pub enum Intrinsic {
    /// One that needs only the host.
    Plain(fn(&mut dyn Host, &mut Args, Span) -> Result<Value, Panic>),
    /// A mutating method (`var self`), such as `List.push`: it changes
    /// its receiver in place, given the call's other arguments.
    Mutating(fn(&mut Value, &mut Args, Span) -> Result<Value, Panic>),
    /// One that calls back into the program, such as `List.map`, which
    /// calls its closure, or uses the interpreter's state, such as
    /// `String.chars`'s shared texts: it gets the interpreter instead of
    /// the host.
    HigherOrder(fn(&mut Interpreter<'_, '_>, &mut Args, Span) -> Result<Value, Panic>),
}

/// A table of intrinsics, by the qualified name the checked program calls
/// them by.
pub type Table = &'static [(&'static str, Intrinsic)];

/// Every intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("Args.get", Plain(args_get)),
    ("Clock.now_millis", Plain(clock_now_millis)),
    ("Dir.read_only", Plain(fs_read_only)),
    ("Dir.restrict_to", Plain(dir_restrict_to)),
    ("Dir.write_only", Plain(fs_write_only)),
    ("DirRead.list_dir", Plain(fs_list_dir)),
    ("DirRead.read_bytes", Plain(fs_read_bytes)),
    ("DirRead.read_to_string", Plain(fs_read_to_string)),
    ("DirRead.restrict_to", Plain(dir_restrict_to)),
    ("DirRead.restrict_to_file", Plain(dir_restrict_to_file)),
    ("DirWrite.create_dir", Plain(fs_create_dir)),
    ("DirWrite.restrict_to", Plain(dir_restrict_to)),
    ("DirWrite.restrict_to_file", Plain(dir_restrict_to_file)),
    ("DirWrite.write", Plain(fs_write)),
    ("DirWrite.write_bytes", Plain(fs_write_bytes)),
    ("Env.get", Plain(env_get)),
    ("Env.secret", HigherOrder(env_secret)),
    ("FileRead.read_bytes", Plain(file_read_bytes)),
    ("FileRead.read_to_string", Plain(file_read_to_string)),
    ("FileWrite.write", Plain(file_write)),
    ("FileWrite.write_bytes", Plain(file_write_bytes)),
    ("Fs.read_only", Plain(fs_read_only)),
    ("Fs.restrict_to", Plain(fs_restrict_to)),
    ("Fs.write_only", Plain(fs_write_only)),
    ("FsRead.restrict_to", Plain(fs_restrict_to)),
    ("FsRead.restrict_to_file", Plain(fs_restrict_to_file)),
    ("FsWrite.restrict_to", Plain(fs_restrict_to)),
    ("FsWrite.restrict_to_file", Plain(fs_restrict_to_file)),
    ("Int.to_string", Plain(int_to_string)),
    ("Option.is_some", Plain(option_is_some)),
    ("Option.ok_or", Plain(option_ok_or)),
    ("Option.unwrap_or", Plain(option_unwrap_or)),
    ("Path.from", Plain(path_new)),
    ("Path.new", Plain(path_new)),
    ("Random.next_int", Plain(random_next_int)),
    ("Random.secure_bytes", Plain(random_secure_bytes)),
    ("Result.is_ok", Plain(result_is_ok)),
    ("Result.unwrap_or", Plain(result_unwrap_or)),
    ("Secret.constant_time_eq", Plain(secret_constant_time_eq)),
    (
        "Secret.constant_time_eq_secret",
        Plain(secret_constant_time_eq_secret),
    ),
    ("Secret.expose", Plain(secret_expose)),
    ("Stderr.println", HigherOrder(stderr_println)),
    ("Stdin.read_line", HigherOrder(stdin_read_line)),
    ("Stdout.println", HigherOrder(stdout_println)),
    ("String.assume_trusted", Plain(string_assume_trusted)),
    ("String.to_string", Plain(string_to_string)),
    ("U32.rotl", Plain(rotl)),
    ("U32.rotr", Plain(rotr)),
    ("U64.rotl", Plain(rotl)),
    ("U64.rotr", Plain(rotr)),
    ("U8.rotl", Plain(rotl)),
    ("U8.rotr", Plain(rotr)),
    ("dbg", HigherOrder(dbg)),
    ("todo", Plain(todo)),
    ("map_entry_limit", Plain(map_entry_limit)),
    ("map_probe_limit", Plain(map_probe_limit)),
];

/// The intrinsic named `name`, from every module's table.
///
/// A `std` module's function is called by its own name (`text`), or by
/// its module-qualified name (`html.text`) when the program declares the
/// same name itself; both find the same intrinsic. A qualified name finds
/// only its own module's functions: `sql.text` finds nothing.
pub fn lookup(name: &str) -> Option<Intrinsic> {
    let index = index();
    index.by_name.get(name).map(|&at| index.all[at as usize])
}

/// The intrinsic a call site names, looked up on the site's first call
/// and kept in its [`IntrinsicName`], so a call site that runs many times
/// looks its name up once (#502).
pub fn resolved(name: &IntrinsicName) -> Intrinsic {
    index().all[resolved_at(name) as usize]
}

/// Where the intrinsic the call site `name` names is in the [`Index`],
/// looked up on the first call and kept in `name`.
fn resolved_at(name: &IntrinsicName) -> u32 {
    name.resolve_with(|name| {
        *index()
            .by_name
            .get(name)
            .expect("`crate::run` refuses a call to a missing intrinsic")
    })
}

/// The prelude `Display` implementations an interpolation hole can show
/// in place, without calling them ([`display_in_place`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InPlace {
    /// `Display for Int`: the integer in decimal.
    Int,
    /// `Display for String`: the text itself.
    Str,
}

/// Which of the [`InPlace`] implementations the call site `name` names,
/// if one does: resolved and kept in `name` as [`resolved`] resolves it,
/// so the site is resolved whether or not it is ever called (#572).
pub(crate) fn display_in_place(name: &IntrinsicName) -> Option<InPlace> {
    static PLACES: OnceLock<[Option<u32>; 2]> = OnceLock::new();
    let index = index();
    let [int, text] = *PLACES.get_or_init(|| {
        [
            index.by_name.get("Int.to_string").copied(),
            index.by_name.get("String.to_string").copied(),
        ]
    });
    let at = resolved_at(name);
    if Some(at) == int {
        Some(InPlace::Int)
    } else if Some(at) == text {
        Some(InPlace::Str)
    } else {
        None
    }
}

/// Every module's table, in the order [`lookup`] prefers them, with the
/// `std` module whose functions it holds, for a `std` module's table.
const TABLES: [(Option<&str>, Table); 12] = [
    (None, INTRINSICS),
    (None, crate::net::INTRINSICS),
    (None, budget::INTRINSICS),
    (None, approval::INTRINSICS),
    (None, int_methods::INTRINSICS),
    (None, data::INTRINSICS),
    (None, bytes::INTRINSICS),
    (None, secrets::INTRINSICS),
    (None, tasks::INTRINSICS),
    (None, traits::INTRINSICS),
    (None, testing::INTRINSICS),
    (Some("std.html"), html::INTRINSICS),
];

/// Every intrinsic, and where each is in it by every name a call site
/// can name it by.
struct Index {
    all: Vec<Intrinsic>,
    by_name: HashMap<String, u32>,
}

/// The [`Index`], made once. Where two tables share a name, the first
/// table's intrinsic has it. A `std` module's function also has its
/// module-qualified name, `html.text` for `text`.
fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index = Index {
            all: Vec::new(),
            by_name: HashMap::new(),
        };
        for (module, table) in TABLES {
            let module = module.map(|path| path.rsplit('.').next().unwrap_or(path));
            for &(name, intrinsic) in table {
                let at = u32::try_from(index.all.len()).expect("fewer than 2^32 intrinsics");
                index.all.push(intrinsic);
                index.by_name.entry(name.to_string()).or_insert(at);
                if let Some(module) = module {
                    index
                        .by_name
                        .entry(format!("{module}.{name}"))
                        .or_insert(at);
                }
            }
        }
        index
    })
}

/// Whether the intrinsic named `name` has a runtime implementation.
pub fn implemented(name: &str) -> bool {
    lookup(name).is_some()
}

/// The text of the string argument at `index`.
fn text(args: &[Value], index: usize) -> &str {
    match args.get(index) {
        Some(Value::Str(s)) => s,
        other => unreachable!("the checker guarantees a string argument, found {other:?}"),
    }
}

/// `Ok(())` for a write or other operation that succeeded, else
/// `Err(IoError)` with Oska's wording of the failure, made at `span`
/// (counted under `--max-memory`).
pub(crate) fn io_unit(result: io::Result<()>, span: Span) -> Result<Value, Panic> {
    match result {
        Ok(()) => Ok(Value::ok(Value::Unit)),
        Err(err) => io_failure(&err, span),
    }
}

/// `Err(IoError)` with Oska's wording of `err`, made at `span`.
pub(crate) fn io_failure(err: &io::Error, span: Span) -> Result<Value, Panic> {
    Ok(Value::err(memory::io_error(
        &oska_sys::io_error_message(err),
        span,
    )?))
}

/// The text of the bytes read from outside, or the failure, counted
/// under `--max-memory` once read, at `span`. Bytes that are not UTF-8
/// are a failure, worded as Rust's `read_to_string` words it.
pub(crate) fn io_text(result: io::Result<Vec<u8>>, span: Span) -> Result<Value, Panic> {
    let result = result.and_then(|bytes| {
        String::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )
        })
    });
    match result {
        Ok(text) => Ok(Value::ok(memory::text(&text, span)?)),
        Err(err) => io_failure(&err, span),
    }
}

/// The bytes read from outside, as they are, or the failure, counted
/// under `--max-memory` once read, at `span`.
pub(crate) fn io_bytes(result: io::Result<Vec<u8>>, span: Span) -> Result<Value, Panic> {
    match result {
        Ok(bytes) => Ok(Value::ok(memory::bytes_of_len(bytes.len(), span, || {
            bytes
        })?)),
        Err(err) => io_failure(&err, span),
    }
}

/// The bytes of the `Bytes` argument at `index`.
fn bytes_arg(args: &[Value], index: usize) -> &[u8] {
    args[index].as_bytes()
}

// ---- Standard streams --------------------------------------------------

// The program's writes to stdout and stderr go through the interpreter,
// which counts them against `--max-output` (`Interpreter::write_output`).

/// `Stdout.println(self, line)`: the line and a newline.
fn stdout_println(
    interp: &mut Interpreter<'_, '_>,
    args: &mut Args,
    span: Span,
) -> Result<Value, Panic> {
    let line = format!("{}\n", text(args, 1));
    let written = interp.write_output(Stream::Stdout, &line, span)?;
    io_unit(written, span)
}

/// `Stderr.println(self, line)`: the line and a newline.
fn stderr_println(
    interp: &mut Interpreter<'_, '_>,
    args: &mut Args,
    span: Span,
) -> Result<Value, Panic> {
    let line = format!("{}\n", text(args, 1));
    let written = interp.write_output(Stream::Stderr, &line, span)?;
    io_unit(written, span)
}

/// `Stdin.read_line(self)`: the next line, or `None` at end of input.
/// Under `--timeout` the wait ends at the deadline (`P0039`).
fn stdin_read_line(
    interp: &mut Interpreter<'_, '_>,
    _: &mut Args,
    span: Span,
) -> Result<Value, Panic> {
    match interp.read_line(span)? {
        Ok(line) => {
            let line = line.map(|l| memory::text(&l, span)).transpose()?;
            Ok(Value::ok(Value::option(line)))
        }
        Err(err) => io_failure(&err, span),
    }
}

/// `dbg(value)`: print `value` to stderr for debugging, and return it. A
/// `Secret` prints as `<redacted>`, wherever it is.
fn dbg(interp: &mut Interpreter<'_, '_>, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let value = args.swap_remove(0);
    let mut line = String::from("[dbg] ");
    debug(&value, &mut line);
    line.push('\n');
    // `dbg` is for debugging: a stderr that cannot be written loses the
    // line, not the program.
    let _ = interp.write_output(Stream::Stderr, &line, span)?;
    Ok(value)
}

/// `value` as `dbg` shows it, appended to `out`.
pub(crate) fn debug(value: &Value, out: &mut String) {
    let tuple = |out: &mut String, fields: &[Value]| {
        out.push('(');
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            debug(field, out);
        }
        out.push(')');
    };
    match value {
        Value::Unit => out.push_str("()"),
        Value::Bool(b) => out.push_str(&b.to_string()),
        Value::Int(n, _) => out.push_str(&n.to_string()),
        Value::Str(s) => debug_text(s, out),
        Value::Struct { ty, fields } if &**ty == builtins::RANGE => {
            debug(&fields[0], out);
            out.push_str("..");
            debug(&fields[1], out);
        }
        Value::Struct { ty, .. } if &**ty == builtins::SECRET => out.push_str("<redacted>"),
        // Its one field is the runtime's own number for it.
        Value::Struct { ty, .. } if &**ty == builtins::RESERVATION => out.push_str("<reservation>"),
        // Its last field is the runtime's own number for it.
        Value::Struct { ty, .. } if &**ty == builtins::APPROVAL => out.push_str("<approval>"),
        Value::Struct { ty, fields } => {
            out.push_str(ty);
            tuple(out, fields);
        }
        Value::Variant {
            ty,
            variant,
            fields,
        } => {
            out.push_str(&format!("{ty}.{variant}"));
            if !fields.is_empty() {
                tuple(out, fields);
            }
        }
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                debug(item, out);
            }
            out.push(']');
        }
        Value::Bytes(data) => {
            out.push_str("Bytes(");
            out.push_str(&bytes::hex(data));
            out.push(')');
        }
        Value::Closure(_) => out.push_str("<closure>"),
        Value::Function(_) => out.push_str("<fn>"),
        // Only the runtime's own number of its counters would show.
        Value::Capability(Capability::Budget(_)) => out.push_str("<Budget>"),
        Value::Capability(cap) => out.push_str(&format!("<{cap:?}>")),
        Value::Args(args) => {
            out.push_str("Args[");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                debug_text(arg, out);
            }
            out.push(']');
        }
        Value::Scope(_) => out.push_str("<scope>"),
        Value::Task(_) => out.push_str("<task>"),
    }
}

/// `dbg` of a text, as `std.rt`'s `debug_string` writes it: in double
/// quotes, `"` and `\` escaped by a `\`, NUL, tab, line feed and carriage
/// return as `\0`, `\t`, `\n` and `\r`, and every character `std.rt`'s
/// table lists as `\u{hex}`. This is what Rust's `{:?}` writes, for the
/// Unicode of the pinned Rust, but it reads the table rather than asking
/// Rust, so it does not change with the Rust that built the interpreter.
fn debug_text(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if oska_check::debug_escapes(c) => {
                out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---- Arguments and environment -----------------------------------------

/// `Args.get(self, index)`: the argument at `index`, the program path at
/// 0, or `None` past the end.
fn args_get(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let Value::Args(all) = &args[0] else {
        unreachable!("the checker guarantees `Args`, found {:?}", args[0]);
    };
    let found = usize::try_from(args[1].as_int())
        .ok()
        .and_then(|i| all.get(i));
    Ok(Value::option(found.map(|a| Value::Str(Rc::clone(a)))))
}

/// `Env.get(self, name)`: the variable, or `None` when it is not set.
fn env_get(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let value = host.env_var(text(args, 1));
    let value = value.map(|v| memory::text(&v, span)).transpose()?;
    Ok(Value::option(value))
}

/// `Env.secret(self, name)`: the variable, as a `Secret`. The signature
/// has no room for a missing secret, and running on with a stand-in, say
/// an empty one, could let an empty password through, so a missing one
/// panics.
///
/// The report quotes the name, which the program made: under
/// `--max-output` it quotes no more of it than the limit has left.
fn env_secret(
    interp: &mut Interpreter<'_, '_>,
    args: &mut Args,
    span: Span,
) -> Result<Value, Panic> {
    let name = text(args, 1);
    match interp.host().env_var(name) {
        Some(value) => memory::wrap(builtins::SECRET, memory::text(&value, span)?, span),
        None => {
            let name = match interp.output_left() {
                Some(left) => limits::cut_quoted(name, left),
                None => name.into(),
            };
            Err(Panic::new(
                codes::MISSING_SECRET,
                format!("the secret `{name}` is not set in the environment"),
                span,
            ))
        }
    }
}

// ---- Secrets -----------------------------------------------------------

/// `Secret.expose(self)`: the wrapped value.
fn secret_expose(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args[0].fields()[0].clone())
}

/// `Secret.constant_time_eq(self, other)`, on secret text or bytes.
fn secret_constant_time_eq(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let secret = &args[0].fields()[0];
    Ok(Value::Bool(constant_time_eq(
        contents(secret),
        contents(&args[1]),
    )))
}

/// `Secret.constant_time_eq_secret(self, other)`, on secret text or bytes.
fn secret_constant_time_eq_secret(
    _: &mut dyn Host,
    args: &mut Args,
    _: Span,
) -> Result<Value, Panic> {
    let (a, b) = (&args[0].fields()[0], &args[1].fields()[0]);
    Ok(Value::Bool(constant_time_eq(contents(a), contents(b))))
}

/// The bytes of a text or `Bytes` value.
fn contents(value: &Value) -> &[u8] {
    match value {
        Value::Bytes(bytes) => bytes,
        _ => value.as_str().as_bytes(),
    }
}

/// Whether `a` and `b` are equal, in time that depends on their lengths
/// but not on where they first differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let diff = a
        .iter()
        .zip(b)
        .fold(a.len() ^ b.len(), |acc, (x, y)| acc | usize::from(x ^ y));
    diff == 0
}

// ---- Clock and randomness ----------------------------------------------

/// `Clock.now_millis(self)`.
fn clock_now_millis(host: &mut dyn Host, _: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(host.now_millis()))
}

/// `Random.next_int(self)`: from 0 up to, but excluding, 2^63.
fn random_next_int(host: &mut dyn Host, _: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(i128::from(host.next_random() >> 1)))
}

/// `Random.secure_bytes(self, n)`: `n` bytes from the operating system's
/// secure random source, as a `Secret<Bytes>`. A negative `n` panics with
/// `P0042` and one past [`bytes::MAX_BYTES`] with `P0043`, as
/// `Bytes.zeroed` does. A host with no secure source ends the program,
/// as the native runtime does, rather than fall back to a guessable one.
fn random_secure_bytes(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let len = bytes::checked_len(args[1].as_int(), span)?;
    let Ok(data) = host.secure_bytes(len) else {
        eprintln!("the operating system has no secure random source");
        std::process::exit(101);
    };
    let bytes = memory::bytes_of_len(len, span, || data)?;
    memory::wrap(builtins::SECRET, bytes, span)
}

// ---- Files -------------------------------------------------------------

/// The filesystem or directory capability the checker guarantees `value`
/// is. Both carry the same attenuation state, so their intrinsics share
/// it; only the type the capability reports differs.
fn fs_cap(value: &Value) -> &FsCap {
    match value {
        Value::Capability(Capability::Fs(fs) | Capability::Dir(fs)) => fs,
        other => unreachable!("the checker guarantees a filesystem capability, found {other:?}"),
    }
}

/// `Path.new(raw)` and `Path.from(raw)`.
fn path_new(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::wrap(builtins::PATH, args.swap_remove(0), span)
}

/// `restrict_to(self, dir)` on `Fs`, `FsRead`, and `FsWrite`: the
/// directory capability with the same access, confined to `dir`, which
/// the host opens now and holds. `dir` is trusted text, any path whose
/// last component is not a symlink.
fn fs_restrict_to(host: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(restricted(host, fs_cap(&args[0]), text(args, 1)))
}

/// `restrict_to(self, dir)` on `Dir`, `DirRead`, and `DirWrite`: the same
/// capability narrowed to `dir`, a `Path` that may come from untrusted
/// text. It is walked beneath the current root like any path read there
/// (`confine.rs`), so a narrowing never widens.
fn dir_restrict_to(host: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(restricted(
        host,
        fs_cap(&args[0]),
        args[1].fields()[0].as_str(),
    ))
}

/// The directory capability `fs` attenuates to at `dir`, a directory.
/// Attenuation never fails: a root that cannot be opened, or is not a
/// directory, leaves the result able to reach nothing, and every operation
/// on it fails with the reason, which names the root.
fn restricted(host: &mut dyn Host, fs: &FsCap, dir: &str) -> Value {
    let opened = match &fs.root {
        Root::Anywhere if dir.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an empty path names no directory",
        )),
        Root::Anywhere => host.open_root(Path::new(dir)),
        Root::Dir(root) => confine::names(dir).and_then(|names| host.open_beneath(root, &names)),
        Root::Nowhere(why) => Err(io::Error::new(io::ErrorKind::NotFound, why.to_string())),
    };
    Value::Capability(Capability::Dir(FsCap {
        root: attenuated_root(fs, opened, dir),
        access: fs.access,
    }))
}

/// `restrict_to_file(self, file)` on `FsRead` and `FsWrite`: the file
/// capability with the same access for the one file at `file`, trusted
/// text. The host opens its parent now and holds it, with the file's name.
fn fs_restrict_to_file(host: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(restricted_file(host, fs_cap(&args[0]), text(args, 1)))
}

/// `restrict_to_file(self, file)` on `DirRead` and `DirWrite`: the file
/// capability for the file at `file`, a `Path` that may come from
/// untrusted text. It is walked beneath the current root like any path
/// read there (`confine.rs`), so a file never lies outside it.
fn dir_restrict_to_file(host: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(restricted_file(
        host,
        fs_cap(&args[0]),
        args[1].fields()[0].as_str(),
    ))
}

/// The file capability `fs` attenuates to at `file`. Like
/// [`restricted`], it never fails: a parent that cannot be opened leaves
/// the result able to reach nothing. The file itself is not opened, so it
/// may not exist yet; each use opens its name beneath the parent, and
/// fails if that is a symlink or no regular file.
fn restricted_file(host: &mut dyn Host, fs: &FsCap, file: &str) -> Value {
    let opened = match &fs.root {
        Root::Anywhere if file.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an empty path names no file",
        )),
        Root::Anywhere => host.open_file_root(Path::new(file)),
        Root::Dir(root) => {
            confine::names_to_create(file).and_then(|names| host.open_file_beneath(root, &names))
        }
        Root::Nowhere(why) => Err(io::Error::new(io::ErrorKind::NotFound, why.to_string())),
    };
    Value::Capability(Capability::File(FsCap {
        root: attenuated_root(fs, opened, file),
        access: fs.access,
    }))
}

/// The root of what `fs` attenuates to at `at`, as the host `opened` it:
/// that root, or nowhere, with the reason, if it could not be opened. A
/// capability that reaches nowhere stays there with its own reason.
fn attenuated_root(fs: &FsCap, opened: io::Result<RootHandle>, at: &str) -> Root {
    match opened {
        Ok(root) => Root::Dir(root),
        Err(_) if matches!(fs.root, Root::Nowhere(_)) => fs.root.clone(),
        Err(error) => Root::Nowhere(Rc::from(format!(
            "cannot restrict to `{at}`: {}",
            oska_sys::io_error_message(&error)
        ))),
    }
}

/// `read_only(self)` on `Fs` and `Dir`.
fn fs_read_only(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(with_access(&args[0], Access::Read))
}

/// `write_only(self)` on `Fs` and `Dir`.
fn fs_write_only(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(with_access(&args[0], Access::Write))
}

/// `value`, a filesystem or directory capability, of the same kind and
/// root, with `access`.
fn with_access(value: &Value, access: Access) -> Value {
    let attenuated = FsCap {
        root: fs_cap(value).root.clone(),
        access,
    };
    Value::Capability(match value {
        Value::Capability(Capability::Dir(_)) => Capability::Dir(attenuated),
        _ => Capability::Fs(attenuated),
    })
}

/// Refused unless `fs`'s access allows `intent`.
fn allowed(fs: &FsCap, intent: Intent) -> io::Result<()> {
    let (allowed, what) = match intent {
        Intent::Read => (fs.access.reads(), "read"),
        Intent::Write => (fs.access.writes(), "write"),
    };
    if allowed {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("this filesystem capability cannot {what}"),
    ))
}

/// Why a directory capability whose root could not be opened reaches
/// nothing.
fn broken(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.to_string())
}

/// The root handle `fs` reaches beneath, or why it may not be used for
/// `intent`: its access forbids it, or its root could not be opened.
///
/// Only a directory capability reads or writes, and one never has the
/// unrestricted root: `restrict_to` always gives it a handle or the reason
/// it has none.
fn root_for(fs: &FsCap, intent: Intent) -> io::Result<&RootHandle> {
    allowed(fs, intent)?;
    match &fs.root {
        Root::Dir(root) => Ok(root),
        Root::Nowhere(why) => Err(broken(why)),
        Root::Anywhere => unreachable!("a directory capability is never rooted anywhere"),
    }
}

/// `DirRead.read_to_string(self, path)`: the path is walked beneath the
/// root's handle (`confine.rs`).
fn fs_read_to_string(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_text(fs_read(host, args), span)
}

/// `DirRead.read_bytes(self, path)`: read as `read_to_string` reads, but
/// the bytes as they are.
fn fs_read_bytes(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_bytes(fs_read(host, args), span)
}

/// The bytes of the file a `DirRead` (`args[0]`) reaches at the path
/// `args[1]`.
fn fs_read(host: &mut dyn Host, args: &[Value]) -> io::Result<Vec<u8>> {
    let raw = args[1].fields()[0].as_str();
    root_for(fs_cap(&args[0]), Intent::Read).and_then(|root| {
        let names = confine::names(raw)?;
        if names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{raw}` names the directory capability's root itself, not a file"),
            ));
        }
        host.read_file_beneath(root, &names)
    })
}

/// `DirRead.list_dir(self, path)`: the names of the entries of the
/// directory, sorted. The path is walked beneath the root's handle, and
/// `.` lists the root.
fn fs_list_dir(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let raw = args[1].fields()[0].as_str();
    let result = root_for(fs_cap(&args[0]), Intent::Read).and_then(|root| {
        let names = confine::names(raw)?;
        host.list_dir_beneath(root, &names)
    });
    match result {
        Ok(names) => {
            let names = names
                .iter()
                .map(|n| memory::text(n, span))
                .collect::<Result<Vec<Value>, Panic>>()?;
            Ok(Value::ok(memory::list(names, span)?))
        }
        Err(err) => io_failure(&err, span),
    }
}

/// `DirWrite.write(self, path, contents)`: `path` ends in a plain name,
/// and is walked beneath the root's handle.
fn fs_write(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_unit(fs_put(host, args, text(args, 2).as_bytes()), span)
}

/// `DirWrite.write_bytes(self, path, contents)`: `write` with bytes.
fn fs_write_bytes(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_unit(fs_put(host, args, bytes_arg(args, 2)), span)
}

/// Write `contents` to the file a `DirWrite` (`args[0]`) reaches at the
/// path `args[1]`.
fn fs_put(host: &mut dyn Host, args: &[Value], contents: &[u8]) -> io::Result<()> {
    let raw = args[1].fields()[0].as_str();
    root_for(fs_cap(&args[0]), Intent::Write).and_then(|root| {
        let names = confine::names_to_create(raw)?;
        host.write_file_beneath(root, &names, contents)
    })
}

/// `DirWrite.create_dir(self, path)`: confined like a write, so `path`
/// ends in a plain name, and is walked beneath the root's handle.
fn fs_create_dir(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let raw = args[1].fields()[0].as_str();
    let result = root_for(fs_cap(&args[0]), Intent::Write).and_then(|root| {
        let names = confine::names_to_create(raw)?;
        host.create_dir_beneath(root, &names)
    });
    io_unit(result, span)
}

/// `FileRead.read_to_string(self)`: the file's text, opened by its name
/// beneath the parent directory's handle (`confine.rs`).
fn file_read_to_string(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let result = file_root(&args[0]).and_then(|root| host.read_file_beneath(root, &[]));
    io_text(result, span)
}

/// `FileRead.read_bytes(self)`: the file's bytes, as they are.
fn file_read_bytes(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let result = file_root(&args[0]).and_then(|root| host.read_file_beneath(root, &[]));
    io_bytes(result, span)
}

/// `FileWrite.write(self, contents)`: creates the file, or replaces what
/// it holds, opened by its name beneath the parent directory's handle.
fn file_write(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let contents = text(args, 1).as_bytes();
    let result = file_root(&args[0]).and_then(|root| host.write_file_beneath(root, &[], contents));
    io_unit(result, span)
}

/// `FileWrite.write_bytes(self, contents)`: `write` with bytes.
fn file_write_bytes(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let contents = bytes_arg(args, 1);
    let result = file_root(&args[0]).and_then(|root| host.write_file_beneath(root, &[], contents));
    io_unit(result, span)
}

/// The root of the file capability the checker guarantees `value` is, or
/// why it reaches nothing.
fn file_root(value: &Value) -> io::Result<&RootHandle> {
    match value {
        Value::Capability(Capability::File(FsCap {
            root: Root::Dir(root),
            ..
        })) => Ok(root),
        Value::Capability(Capability::File(FsCap {
            root: Root::Nowhere(why),
            ..
        })) => Err(broken(why)),
        other => unreachable!("the checker guarantees a file capability, found {other:?}"),
    }
}

// ---- Option and Result -------------------------------------------------

/// `Option.is_some(self)`.
fn option_is_some(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant(builtins::SOME)))
}

/// `Result.is_ok(self)`.
fn result_is_ok(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant(builtins::OK)))
}

/// `Option.unwrap_or(self, default)`.
fn option_unwrap_or(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, builtins::SOME))
}

/// `Option.ok_or(self, err)`: `Ok` of the value, or `Err(err)` when
/// there is none.
fn option_ok_or(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let err = args.swap_remove(1);
    Ok(match args[0].is_variant(builtins::SOME) {
        true => Value::ok(args[0].fields()[0].clone()),
        false => Value::err(err),
    })
}

/// `Result.unwrap_or(self, default)`.
fn result_unwrap_or(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, builtins::OK))
}

/// The field of `args[0]` when it is the variant `present`, else
/// `args[1]`, the default.
fn unwrap_or(args: &mut Args, present: &str) -> Value {
    let default = args.swap_remove(1);
    let value = args.swap_remove(0);
    if value.is_variant(present) {
        value.fields()[0].clone()
    } else {
        default
    }
}

/// `todo()`: a placeholder for code not yet written, which panics when
/// reached.
fn todo(_: &mut dyn Host, _: &mut Args, span: Span) -> Result<Value, Panic> {
    Err(Panic::new(
        codes::TODO,
        "reached `todo()`: this code is not written yet",
        span,
    ))
}

/// `std.map`'s `P0044`: an entry past the map's entry limit.
fn map_entry_limit(_: &mut dyn Host, _: &mut Args, span: Span) -> Result<Value, Panic> {
    Err(Panic::new(
        codes::MAP_ENTRY_LIMIT,
        MAP_ENTRY_LIMIT_MESSAGE,
        span,
    ))
}

/// `std.map`'s `P0045`: probes past the limit under every hash.
fn map_probe_limit(_: &mut dyn Host, _: &mut Args, span: Span) -> Result<Value, Panic> {
    Err(Panic::new(
        codes::MAP_PROBE_LIMIT,
        MAP_PROBE_LIMIT_MESSAGE,
        span,
    ))
}

/// The messages of `P0044` and `P0045`, the same in the native backend.
pub const MAP_ENTRY_LIMIT_MESSAGE: &str = "the map holds as many entries as its limit allows";
pub const MAP_PROBE_LIMIT_MESSAGE: &str =
    "the map's keys collide under every hash it tries: they look chosen to flood it";

/// `U8.rotl(self, n)`, and `U32`'s and `U64`'s: `P0008` for an amount
/// out of range (`ops::rotate`).
fn rotl(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    rotate(args, true, span)
}

/// `U8.rotr(self, n)`, and `U32`'s and `U64`'s.
fn rotr(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    rotate(args, false, span)
}

/// A rotation's receiver and amount, `args`, rotated left or not.
fn rotate(args: &mut Args, left: bool, span: Span) -> Result<Value, Panic> {
    let (Some(n), Some(x)) = (args.pop(), args.pop()) else {
        unreachable!("a rotation's receiver and amount");
    };
    ops::rotate(x, n, left, span)
}

/// `Int.to_string(self)`: the integer in decimal.
fn int_to_string(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::text(&args[0].as_int().to_string(), span)
}

/// `Untrusted<String>.assume_trusted(self)`: the text itself. Trust is a
/// property the checker tracks; at runtime untrusted text is a plain
/// string.
fn string_assume_trusted(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args.swap_remove(0))
}

/// `String.to_string(self)`: the text itself.
fn string_to_string(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args.swap_remove(0))
}

#[cfg(test)]
mod tests {
    use oska_check::builtins;
    use oska_syntax::ast::{Fields, Item, ItemKind, Type, TypeKind};

    use super::{implemented, lookup, Intrinsic};
    use crate::value::Capability;

    /// The prelude's source, the same file the checker embeds.
    const PRELUDE: &str = include_str!("../../oska-check/src/prelude.osk");

    /// The types whose methods have no runtime: the built-in capabilities
    /// the runtime does not implement, and the prelude's structs that hold
    /// one. A program that holds one is refused
    /// before it starts (`E0370`), so none of their methods is ever
    /// called. Everything else in the prelude needs an intrinsic.
    fn no_runtime() -> Vec<String> {
        let missing = |name: &str| builtins::is_capability(name) && !Capability::implemented(name);
        let module = oska_syntax::parse_prelude(PRELUDE).expect("the prelude should parse");
        module
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Struct(s) => Some(s),
                _ => None,
            })
            .filter(|s| {
                let fields = match &s.fields {
                    Fields::Named(fields) | Fields::Positional(fields) => fields.as_slice(),
                    Fields::Unit => &[],
                };
                missing(&s.name.name)
                    || fields
                        .iter()
                        .any(|f| call_owner(&f.ty).is_some_and(|ty| missing(&ty)))
            })
            .map(|s| s.name.name.clone())
            .collect()
    }

    /// The qualified name the checked program calls `item`'s function by,
    /// if `item` is a function without a body.
    fn bodiless(owner: Option<&str>, item: &Item) -> Option<String> {
        let ItemKind::Fn(f) = &item.kind else {
            return None;
        };
        if f.body.is_some() {
            return None;
        }
        Some(match owner {
            Some(owner) => format!("{owner}.{}", f.name.name),
            None => f.name.name.clone(),
        })
    }

    /// The type name the checker puts in the intrinsic names of methods
    /// declared on `target`: the type's own name, except that trust is
    /// transparent, so `Untrusted<String>`'s methods are `String`'s.
    fn call_owner(target: &Type) -> Option<String> {
        let TypeKind::Path { segments, args } = &target.kind else {
            return None;
        };
        let head = &segments.last()?.name;
        match (head.as_str(), args.as_slice()) {
            ("Untrusted", [inner]) => call_owner(inner),
            _ => Some(head.clone()),
        }
    }

    /// Every body-less function the prelude's free functions and impls
    /// declare, by qualified name (`Stdout.println`, `Int.eq`, `dbg`). A
    /// trait's own method signatures are not callable and are skipped.
    fn bodiless_prelude_functions() -> Vec<String> {
        bodiless_prelude_methods()
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// [`bodiless_prelude_functions`], each with whether it is a mutating
    /// method (`var self`).
    fn bodiless_prelude_methods() -> Vec<(String, bool)> {
        let module = oska_syntax::parse_prelude(PRELUDE).expect("the prelude should parse");
        bodiless_methods(&module)
    }

    /// Every body-less function `module` declares, by qualified name.
    fn bodiless_functions(module: &oska_syntax::ast::Module) -> Vec<String> {
        bodiless_methods(module)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// [`bodiless_functions`], each with whether it is a mutating method
    /// (`var self`).
    fn bodiless_methods(module: &oska_syntax::ast::Module) -> Vec<(String, bool)> {
        let mutating = |item: &Item| match &item.kind {
            ItemKind::Fn(f) => f.receiver.as_ref().is_some_and(|r| r.mutable),
            _ => false,
        };
        let mut names = Vec::new();
        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(_) => names.extend(bodiless(None, item).map(|n| (n, false))),
                ItemKind::Impl(decl) => {
                    let Some(owner) = call_owner(&decl.target) else {
                        continue;
                    };
                    for method in &decl.methods {
                        let name = bodiless(Some(&owner), method);
                        names.extend(name.map(|n| (n, mutating(method))));
                    }
                }
                _ => {}
            }
        }
        names
    }

    #[test]
    fn exactly_the_mutating_prelude_methods_are_mutating_intrinsics() {
        let methods = bodiless_prelude_methods();
        assert!(
            methods.iter().any(|(n, m)| n == "List.push" && *m),
            "{methods:?}"
        );
        for (name, var_self) in methods {
            let registered = matches!(lookup(&name), Some(Intrinsic::Mutating(_)));
            if lookup(&name).is_some() {
                assert_eq!(
                    registered, var_self,
                    "`{name}`: `var self` is a mutating intrinsic"
                );
            }
        }
    }

    /// The index gives a `std` module's function its qualified name
    /// (`html.text`) too, so no intrinsic's own name may look like one,
    /// and every module `TABLES` names must be a real `std` module.
    #[test]
    fn no_intrinsic_is_named_with_a_std_modules_prefix() {
        for (module, _) in super::TABLES {
            if let Some(module) = module {
                assert!(
                    builtins::std_modules().any(|m| m == module),
                    "`{module}` is not a std module"
                );
            }
        }
        let modules: Vec<&str> = builtins::std_modules()
            .filter_map(|path| path.rsplit('.').next())
            .collect();
        assert!(modules.contains(&"html"), "{modules:?}");
        for (name, _) in super::TABLES.iter().flat_map(|(_, table)| table.iter()) {
            for module in &modules {
                assert!(
                    !name.starts_with(&format!("{module}.")),
                    "`{name}` starts with the std module name `{module}`"
                );
            }
        }
    }

    #[test]
    fn a_qualified_std_name_finds_the_same_intrinsic_as_its_own_name() {
        assert!(matches!(lookup("text"), Some(Intrinsic::Plain(_))));
        assert!(matches!(lookup("html.text"), Some(Intrinsic::Plain(_))));
        assert!(
            lookup("sql.text").is_none(),
            "`text` is std.html's, not std.sql's"
        );
        assert!(lookup("html.no_such_function").is_none());
        assert!(lookup("nohtml.text").is_none());
    }

    fn owner(name: &str) -> &str {
        name.split('.').next().unwrap_or("")
    }

    #[test]
    fn every_bodiless_prelude_function_has_a_runtime_intrinsic() {
        let names = bodiless_prelude_functions();
        assert!(
            names.len() > 40 && names.iter().any(|n| n == "Stdout.println"),
            "the scan should find the prelude's functions, found {names:?}"
        );
        let exempt = no_runtime();
        let missing: Vec<&String> = names
            .iter()
            .filter(|name| !exempt.iter().any(|e| e == owner(name)))
            .filter(|name| !implemented(name))
            .collect();
        assert!(
            missing.is_empty(),
            "prelude functions with no body and no runtime intrinsic: {missing:?}"
        );
    }

    #[test]
    fn every_bodiless_std_html_function_has_a_runtime_intrinsic() {
        let module = oska_syntax::parse_prelude(include_str!("../../oska-check/src/std/html.osk"))
            .expect("std.html should parse");
        let names = bodiless_functions(&module);
        assert!(names.iter().any(|n| n == "text"), "{names:?}");
        let missing: Vec<&String> = names.iter().filter(|n| !implemented(n)).collect();
        assert!(
            missing.is_empty(),
            "std.html functions with no runtime: {missing:?}"
        );
    }

    #[test]
    fn std_rt_intrinsics_are_the_toolchains_and_have_no_interpreter_implementation() {
        // `std.rt` is body-less by design (ADR 0013): the native backend
        // lowers its intrinsics to machine code, and no user code can
        // import it, so the interpreter implements none of them.
        let module =
            oska_syntax::parse_prelude(oska_check::STD_RT_SOURCE).expect("std.rt should parse");
        let names = bodiless_functions(&module);
        assert!(names.iter().any(|n| n == "rt_sys_write"), "{names:?}");
        assert_eq!(names, oska_check::std_rt_intrinsics());
        let implemented: Vec<&String> = names.iter().filter(|n| implemented(n)).collect();
        assert!(
            implemented.is_empty(),
            "std.rt intrinsics the interpreter must refuse: {implemented:?}"
        );
    }

    #[test]
    fn the_exempt_types_are_the_ones_with_no_runtime_and_have_no_intrinsics() {
        let names = bodiless_prelude_functions();
        let exempt = no_runtime();
        for expected in ["Spawn", "Sched"] {
            assert!(exempt.iter().any(|e| e == expected), "{exempt:?}");
        }
        // The network has a runtime (ADR 0025).
        for run in [
            "Net",
            "NetConnect",
            "NetListen",
            "HostConnect",
            "LoopbackListen",
            "Connection",
        ] {
            assert!(!exempt.iter().any(|e| e == run), "{exempt:?}");
        }
        for granted in ["Stdout", "Fs", "FsRead", "TestContext", "Args"] {
            assert!(!exempt.iter().any(|e| e == granted), "{exempt:?}");
        }
        for exempt in &exempt {
            assert!(
                !names.iter().any(|n| owner(n) == *exempt && implemented(n)),
                "`{exempt}` has an intrinsic now, but no capability runtime"
            );
        }
    }
}

#[cfg(test)]
mod access_tests {
    //! The checker gives `DirRead` no `write` and `DirWrite` no
    //! `read_to_string`, so no source program reaches the runtime's own
    //! access check. It is tested on checked programs built by hand, in
    //! which a read-only filesystem calls `write` and a write-only one
    //! `read_to_string` anyway.

    use oska_check::program::{Callee, Contracts, Expr, ExprKind, Function, Param, Program, Type};
    use oska_syntax::Span;

    use std::path::Path;

    use crate::host::{FakeHost, Host};
    use crate::interp::Interpreter;
    use crate::value::{Access, Capability, FsCap, Root, Value};

    fn at(kind: ExprKind) -> Expr {
        Expr {
            kind,
            ty: Type::Unit,
            span: Span { start: 0, end: 1 },
        }
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        at(ExprKind::Call {
            callee: Callee::intrinsic(name),
            type_args: Vec::new(),
            args,
        })
    }

    /// `fn main(fs) { fs.<method>(Path.new("notes.txt"), extra...) }`.
    fn calling(method: &str, extra: Vec<Expr>) -> Program {
        let path = call("Path.new", vec![at(ExprKind::Str("notes.txt".to_string()))]);
        let args = [at(ExprKind::Local(0)), path].into_iter().chain(extra);
        let mut program = Program::default();
        program.functions.insert(
            "main".to_string(),
            Function {
                name: "main".to_string(),
                span: Span::at(0),
                generics: Vec::new(),
                self_ty: None,
                receiver: false,
                mutating: false,
                params: vec![Param {
                    name: "fs".to_string(),
                    ty: Type::Unit,
                    binds: true,
                    annotation: None,
                }],
                ret: Type::Unit,
                ret_annotation: None,
                contracts: Contracts::default(),
                body: call(method, args.collect()),
            },
        );
        program
    }

    /// What `program`'s `main` returns, given a directory capability for
    /// the host's `/` with `access`, run on `host`.
    fn run_with(program: &Program, access: Access, host: &mut FakeHost) -> Value {
        let root = host.open_root(Path::new("/")).expect("the root");
        let fs = Value::Capability(Capability::Dir(FsCap {
            root: Root::Dir(root),
            access,
        }));
        let main = &program.functions["main"];
        Interpreter::new(program, host)
            .call(main, vec![fs], main.body.span)
            .expect("no panic")
    }

    fn io_error_message(result: &Value) -> &str {
        assert!(result.is_variant("Err"), "{result:?}");
        result.fields()[0].fields()[0].as_str()
    }

    #[test]
    fn a_read_only_directory_refuses_to_write_at_runtime() {
        let program = calling("DirWrite.write", vec![at(ExprKind::Str("x".to_string()))]);
        let mut host = FakeHost::new();
        let result = run_with(&program, Access::Read, &mut host);
        assert_eq!(
            io_error_message(&result),
            "this filesystem capability cannot write"
        );
        assert_eq!(host.file("notes.txt"), None, "nothing was written");
        let mut host = FakeHost::new();
        let result = run_with(&program, Access::ReadWrite, &mut host);
        assert!(result.is_variant("Ok"), "{result:?}");
        assert_eq!(host.file("notes.txt"), Some("x"));
    }

    #[test]
    fn a_write_only_directory_refuses_to_read_at_runtime() {
        let program = calling("DirRead.read_to_string", Vec::new());
        let mut host = FakeHost::new().with_file("notes.txt", "secret");
        let result = run_with(&program, Access::Write, &mut host);
        assert_eq!(
            io_error_message(&result),
            "this filesystem capability cannot read"
        );
        let result = run_with(&program, Access::ReadWrite, &mut host);
        assert_eq!(result, Value::ok(Value::str("secret")));
    }
}
