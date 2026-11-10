//! Runtime implementations of body-less prelude functions, keyed by the
//! qualified name the checked program calls them by (`Stdout.println`).
//!
//! To add one: write a function with the [`Intrinsic`] signature and add
//! it to [`INTRINSICS`]. A method's receiver is its first argument.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use oska_syntax::Span;

use crate::confine::{self, Intent};
use crate::data;
use crate::host::Host;
use crate::interp::Interpreter;
use crate::panics::{codes, Panic};
use crate::tasks;
use crate::traits;
use crate::value::{Access, Capability, FsCap, Root, Value, SECRET};

/// An intrinsic: the host, the arguments (the receiver first), and the
/// call's span, for panics.
pub type Intrinsic = fn(&mut dyn Host, Vec<Value>, Span) -> Result<Value, Panic>;

/// A mutating intrinsic, such as `List.push`: it changes its receiver in
/// place, given the call's other arguments and its span.
pub type MutatingIntrinsic = fn(&mut Value, Vec<Value>, Span) -> Result<Value, Panic>;

/// An intrinsic that calls back into the program, such as `List.map`,
/// which calls its closure: it gets the interpreter instead of the host.
pub type HigherOrder = fn(&mut Interpreter<'_, '_>, Vec<Value>, Span) -> Result<Value, Panic>;

/// Every intrinsic, by qualified name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    ("Args.get", args_get),
    ("Clock.now_millis", clock_now_millis),
    ("Env.get", env_get),
    ("Env.secret", env_secret),
    ("Fs.read_only", fs_read_only),
    ("Fs.restrict_to", fs_restrict_to),
    ("Fs.write_only", fs_write_only),
    ("FsRead.read_to_string", fs_read_to_string),
    ("FsRead.restrict_to", fs_restrict_to),
    ("FsWrite.restrict_to", fs_restrict_to),
    ("FsWrite.write", fs_write),
    ("Int.to_string", int_to_string),
    ("Option.is_some", option_is_some),
    ("Option.ok_or", option_ok_or),
    ("Option.unwrap_or", option_unwrap_or),
    ("Path.from", path_new),
    ("Path.new", path_new),
    ("Random.next_int", random_next_int),
    ("Result.is_ok", result_is_ok),
    ("Result.unwrap_or", result_unwrap_or),
    ("Secret.constant_time_eq", secret_constant_time_eq),
    (
        "Secret.constant_time_eq_secret",
        secret_constant_time_eq_secret,
    ),
    ("Secret.expose", secret_expose),
    ("Stderr.println", stderr_println),
    ("Stdin.read_line", stdin_read_line),
    ("Stdout.println", stdout_println),
    ("String.to_string", string_to_string),
    ("dbg", dbg),
    ("todo", todo),
];

/// The intrinsic named `name`.
pub fn lookup(name: &str) -> Option<Intrinsic> {
    INTRINSICS
        .iter()
        .chain(data::INTRINSICS)
        .chain(traits::INTRINSICS)
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// The mutating intrinsic named `name`.
pub fn lookup_mutating(name: &str) -> Option<MutatingIntrinsic> {
    data::MUTATING
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// The higher-order intrinsic named `name`.
pub fn lookup_higher_order(name: &str) -> Option<HigherOrder> {
    data::HIGHER_ORDER
        .iter()
        .chain(tasks::HIGHER_ORDER)
        .chain(traits::HIGHER_ORDER)
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// Whether the intrinsic named `name` has a runtime implementation.
pub fn implemented(name: &str) -> bool {
    lookup(name).is_some() || lookup_mutating(name).is_some() || lookup_higher_order(name).is_some()
}

/// The text of the string argument at `index`.
fn text(args: &[Value], index: usize) -> &str {
    match args.get(index) {
        Some(Value::Str(s)) => s,
        other => unreachable!("the checker guarantees a string argument, found {other:?}"),
    }
}

/// An I/O result as an Oska `Result<T, IoError>`.
fn io_result<T>(result: io::Result<T>, ok: impl FnOnce(T) -> Value) -> Value {
    match result {
        Ok(value) => Value::ok(ok(value)),
        Err(err) => Value::err(Value::io_error(&err.to_string())),
    }
}

// ---- Standard streams --------------------------------------------------

/// `Stdout.println(self, line)`: the line and a newline.
fn stdout_println(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let line = format!("{}\n", text(&args, 1));
    Ok(io_result(host.write_stdout(&line), |()| Value::Unit))
}

/// `Stderr.println(self, line)`: the line and a newline.
fn stderr_println(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let line = format!("{}\n", text(&args, 1));
    Ok(io_result(host.write_stderr(&line), |()| Value::Unit))
}

/// `Stdin.read_line(self)`: the next line, or `None` at end of input.
fn stdin_read_line(host: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(io_result(host.read_line(), |line| {
        Value::option(line.map(|l| Value::str(&l)))
    }))
}

/// `dbg(value)`: print `value` to stderr for debugging, and return it. A
/// `Secret` prints as `<redacted>`, wherever it is.
fn dbg(host: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let value = args.swap_remove(0);
    let mut line = String::from("[dbg] ");
    debug(&value, &mut line);
    line.push('\n');
    // `dbg` is for debugging: a stderr that cannot be written loses the
    // line, not the program.
    let _ = host.write_stderr(&line);
    Ok(value)
}

/// `value` as `dbg` shows it, appended to `out`.
fn debug(value: &Value, out: &mut String) {
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
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Str(s) => out.push_str(&format!("{:?}", &**s)),
        Value::Range { start, end } => out.push_str(&format!("{start}..{end}")),
        Value::Struct { ty, .. } if &**ty == SECRET => out.push_str("<redacted>"),
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
        Value::Closure(_) => out.push_str("<closure>"),
        Value::Function(_) => out.push_str("<fn>"),
        Value::Capability(cap) => out.push_str(&format!("<{cap:?}>")),
        Value::Args(args) => out.push_str(&format!("Args{:?}", &**args)),
        Value::Scope(_) => out.push_str("<scope>"),
        Value::Task(_) => out.push_str("<task>"),
    }
}

// ---- Arguments and environment -----------------------------------------

/// `Args.get(self, index)`: the argument at `index`, the program path at
/// 0, or `None` past the end.
fn args_get(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let Value::Args(all) = &args[0] else {
        unreachable!("the checker guarantees `Args`, found {:?}", args[0]);
    };
    let found = usize::try_from(args[1].as_int())
        .ok()
        .and_then(|i| all.get(i));
    Ok(Value::option(found.map(|a| Value::Str(Rc::clone(a)))))
}

/// `Env.get(self, name)`: the variable, or `None` when it is not set.
fn env_get(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let value = host.env_var(text(&args, 1));
    Ok(Value::option(value.map(|v| Value::str(&v))))
}

/// `Env.secret(self, name)`: the variable, as a `Secret`. The signature
/// has no room for a missing secret, and running on with a stand-in, say
/// an empty one, could let an empty password through, so a missing one
/// panics.
fn env_secret(host: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let name = text(&args, 1);
    match host.env_var(name) {
        Some(value) => Ok(Value::secret(Value::str(&value))),
        None => Err(Panic::new(
            codes::MISSING_SECRET,
            format!("the secret `{name}` is not set in the environment"),
            span,
        )),
    }
}

// ---- Secrets -----------------------------------------------------------

/// `Secret.expose(self)`: the wrapped value.
fn secret_expose(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(args[0].fields()[0].clone())
}

/// `Secret.constant_time_eq(self, other)`.
fn secret_constant_time_eq(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let secret = args[0].fields()[0].as_str();
    Ok(Value::Bool(constant_time_eq(secret, text(&args, 1))))
}

/// `Secret.constant_time_eq_secret(self, other)`.
fn secret_constant_time_eq_secret(
    _: &mut dyn Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let a = args[0].fields()[0].as_str();
    let b = args[1].fields()[0].as_str();
    Ok(Value::Bool(constant_time_eq(a, b)))
}

/// Whether `a` and `b` are equal, in time that depends on their lengths
/// but not on where they first differ.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let diff = a
        .iter()
        .zip(b)
        .fold(a.len() ^ b.len(), |acc, (x, y)| acc | usize::from(x ^ y));
    diff == 0
}

// ---- Clock and randomness ----------------------------------------------

/// `Clock.now_millis(self)`.
fn clock_now_millis(host: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Int(host.now_millis()))
}

/// `Random.next_int(self)`: from 0 up to, but excluding, 2^63.
fn random_next_int(host: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Int(i128::from(host.next_random() >> 1)))
}

// ---- Files -------------------------------------------------------------

/// The filesystem capability the checker guarantees `value` is.
fn fs_cap(value: &Value) -> &FsCap {
    match value {
        Value::Capability(Capability::Fs(fs)) => fs,
        other => unreachable!("the checker guarantees a filesystem capability, found {other:?}"),
    }
}

/// `Path.new(raw)` and `Path.from(raw)`.
fn path_new(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::path(args.swap_remove(0)))
}

/// `restrict_to(self, dir)` on `Fs`, `FsRead`, and `FsWrite`: the same
/// access, confined to `dir`. On an unrestricted filesystem, `dir` is any
/// directory; on a restricted one, it is resolved under the current root
/// like any path, so a restriction never widens. A directory that does
/// not exist, or lies outside the current root, leaves the result able to
/// reach nothing.
fn fs_restrict_to(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let fs = fs_cap(&args[0]);
    let dir = text(&args, 1);
    let real = match &fs.root {
        Root::Anywhere if dir.is_empty() => None,
        Root::Anywhere => host.canonicalize(Path::new(dir)).ok(),
        Root::Dir(root) => confine::resolve(host, root, dir, Intent::Read).ok(),
        Root::Nowhere => None,
    };
    let root = real.map_or(Root::Nowhere, |r| Root::Dir(Rc::from(r.as_path())));
    Ok(Value::Capability(Capability::Fs(FsCap {
        root,
        access: fs.access,
    })))
}

/// `Fs.read_only(self)`.
fn fs_read_only(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(with_access(&args[0], Access::Read))
}

/// `Fs.write_only(self)`.
fn fs_write_only(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(with_access(&args[0], Access::Write))
}

fn with_access(value: &Value, access: Access) -> Value {
    Value::Capability(Capability::Fs(FsCap {
        root: fs_cap(value).root.clone(),
        access,
    }))
}

/// The real path `path` names through `fs`, for `intent`, or why `fs`
/// may not reach it.
fn real_path(host: &mut dyn Host, fs: &FsCap, path: &Value, intent: Intent) -> io::Result<PathBuf> {
    let allowed = match intent {
        Intent::Read => fs.access.reads(),
        Intent::Write => fs.access.writes(),
    };
    if !allowed {
        let what = if intent == Intent::Read {
            "read"
        } else {
            "write"
        };
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("this filesystem capability cannot {what}"),
        ));
    }
    let raw = path.fields()[0].as_str();
    match &fs.root {
        Root::Anywhere if raw.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an empty path names no file",
        )),
        Root::Anywhere => Ok(PathBuf::from(raw)),
        Root::Dir(root) => confine::resolve(host, root, raw, intent),
        Root::Nowhere => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this filesystem capability was restricted to a directory it cannot reach",
        )),
    }
}

/// `FsRead.read_to_string(self, path)`.
fn fs_read_to_string(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let result =
        real_path(host, fs_cap(&args[0]), &args[1], Intent::Read).and_then(|p| host.read_file(&p));
    Ok(io_result(result, |text| Value::str(&text)))
}

/// `FsWrite.write(self, path, contents)`.
fn fs_write(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let result = real_path(host, fs_cap(&args[0]), &args[1], Intent::Write)
        .and_then(|p| host.write_file(&p, text(&args, 2)));
    Ok(io_result(result, |()| Value::Unit))
}

// ---- Option and Result -------------------------------------------------

/// `Option.is_some(self)`.
fn option_is_some(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant("Some")))
}

/// `Result.is_ok(self)`.
fn result_is_ok(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant("Ok")))
}

/// `Option.unwrap_or(self, default)`.
fn option_unwrap_or(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, "Some"))
}

/// `Option.ok_or(self, err)`: `Ok` of the value, or `Err(err)` when
/// there is none.
fn option_ok_or(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let err = args.swap_remove(1);
    Ok(match args[0].is_variant("Some") {
        true => Value::ok(args[0].fields()[0].clone()),
        false => Value::err(err),
    })
}

/// `Result.unwrap_or(self, default)`.
fn result_unwrap_or(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, "Ok"))
}

/// The field of `args[0]` when it is the variant `present`, else
/// `args[1]`, the default.
fn unwrap_or(mut args: Vec<Value>, present: &str) -> Value {
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
fn todo(_: &mut dyn Host, _: Vec<Value>, span: Span) -> Result<Value, Panic> {
    Err(Panic::new(
        codes::TODO,
        "reached `todo()`: this code is not written yet",
        span,
    ))
}

/// `Int.to_string(self)`: the integer in decimal.
fn int_to_string(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::str(&args[0].as_int().to_string()))
}

/// `String.to_string(self)`: the text itself.
fn string_to_string(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(args.swap_remove(0))
}
