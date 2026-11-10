//! Runtime implementations of body-less prelude functions, keyed by the
//! qualified name the checked program calls them by (`Stdout.println`).
//!
//! To add one: write a function with the signature of one of the
//! [`Intrinsic`] kinds and add it, wrapped in its kind, to its module's
//! table ([`INTRINSICS`] here). [`lookup`] searches every table. A
//! method's receiver is its first argument.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use oska_check::builtins;
use oska_syntax::Span;

use crate::confine::{self, Intent};
use crate::data;
use crate::host::Host;
use crate::html;
use crate::interp::Interpreter;
use crate::panics::{codes, Panic};
use crate::tasks;
use crate::testing;
use crate::traits;
use crate::value::{Access, Capability, FsCap, Root, Value};

use Intrinsic::Plain;

/// A body-less prelude function's runtime implementation. Each gets the
/// arguments, the receiver first for a method, and the call's span, for
/// panics, and what else its kind needs.
#[derive(Clone, Copy)]
pub enum Intrinsic {
    /// One that needs only the host.
    Plain(fn(&mut dyn Host, Vec<Value>, Span) -> Result<Value, Panic>),
    /// A mutating method (`var self`), such as `List.push`: it changes
    /// its receiver in place, given the call's other arguments.
    Mutating(fn(&mut Value, Vec<Value>, Span) -> Result<Value, Panic>),
    /// One that calls back into the program, such as `List.map`, which
    /// calls its closure: it gets the interpreter instead of the host.
    HigherOrder(fn(&mut Interpreter<'_, '_>, Vec<Value>, Span) -> Result<Value, Panic>),
}

/// A table of intrinsics, by the qualified name the checked program calls
/// them by.
pub type Table = &'static [(&'static str, Intrinsic)];

/// Every intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("Args.get", Plain(args_get)),
    ("Clock.now_millis", Plain(clock_now_millis)),
    ("Env.get", Plain(env_get)),
    ("Env.secret", Plain(env_secret)),
    ("Fs.read_only", Plain(fs_read_only)),
    ("Fs.restrict_to", Plain(fs_restrict_to)),
    ("Fs.write_only", Plain(fs_write_only)),
    ("FsRead.list_dir", Plain(fs_list_dir)),
    ("FsRead.read_to_string", Plain(fs_read_to_string)),
    ("FsRead.restrict_to", Plain(fs_restrict_to)),
    ("FsWrite.restrict_to", Plain(fs_restrict_to)),
    ("FsWrite.write", Plain(fs_write)),
    ("Int.to_string", Plain(int_to_string)),
    ("Option.is_some", Plain(option_is_some)),
    ("Option.ok_or", Plain(option_ok_or)),
    ("Option.unwrap_or", Plain(option_unwrap_or)),
    ("Path.from", Plain(path_new)),
    ("Path.new", Plain(path_new)),
    ("Random.next_int", Plain(random_next_int)),
    ("Result.is_ok", Plain(result_is_ok)),
    ("Result.unwrap_or", Plain(result_unwrap_or)),
    ("Secret.constant_time_eq", Plain(secret_constant_time_eq)),
    (
        "Secret.constant_time_eq_secret",
        Plain(secret_constant_time_eq_secret),
    ),
    ("Secret.expose", Plain(secret_expose)),
    ("Stderr.println", Plain(stderr_println)),
    ("Stdin.read_line", Plain(stdin_read_line)),
    ("Stdout.println", Plain(stdout_println)),
    ("String.assume_trusted", Plain(string_assume_trusted)),
    ("String.to_string", Plain(string_to_string)),
    ("dbg", Plain(dbg)),
    ("todo", Plain(todo)),
];

/// The intrinsic named `name`, from every module's table.
///
/// A `std` module's function is called by its own name (`text`), or by
/// its module-qualified name (`html.text`) when the program declares the
/// same name itself; both find the same intrinsic.
pub fn lookup(name: &str) -> Option<Intrinsic> {
    let unqualified = builtins::std_modules()
        .filter_map(|path| path.rsplit('.').next())
        .find_map(|module| name.strip_prefix(module)?.strip_prefix('.'));
    [
        INTRINSICS,
        data::INTRINSICS,
        tasks::INTRINSICS,
        traits::INTRINSICS,
        testing::INTRINSICS,
        html::INTRINSICS,
    ]
    .into_iter()
    .flatten()
    .find(|(n, _)| *n == name || Some(*n) == unqualified)
    .map(|(_, intrinsic)| *intrinsic)
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
        Value::Str(s) => out.push_str(&format!("{:?}", &**s)),
        Value::Range { start, end, .. } => out.push_str(&format!("{start}..{end}")),
        Value::Struct { ty, .. } if &**ty == builtins::SECRET => out.push_str("<redacted>"),
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
    Ok(Value::int(host.now_millis()))
}

/// `Random.next_int(self)`: from 0 up to, but excluding, 2^63.
fn random_next_int(host: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(i128::from(host.next_random() >> 1)))
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

/// The real path `path` names through `fs`, for `intent`, with the root
/// it must stay under when `fs` is restricted, or why `fs` may not reach
/// it.
fn real_path<'f>(
    host: &mut dyn Host,
    fs: &'f FsCap,
    path: &Value,
    intent: Intent,
) -> io::Result<(PathBuf, Option<&'f Path>)> {
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
        Root::Anywhere => Ok((PathBuf::from(raw), None)),
        Root::Dir(root) => Ok((confine::resolve(host, root, raw, intent)?, Some(root))),
        Root::Nowhere => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this filesystem capability was restricted to a directory it cannot reach",
        )),
    }
}

/// `FsRead.read_to_string(self, path)`.
fn fs_read_to_string(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let result = real_path(host, fs_cap(&args[0]), &args[1], Intent::Read)
        .and_then(|(path, beneath)| host.read_file(&path, beneath));
    Ok(io_result(result, |text| Value::str(&text)))
}

/// `FsRead.list_dir(self, path)`: the names of the entries of the
/// directory, sorted. It is confined like a read.
fn fs_list_dir(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let result = real_path(host, fs_cap(&args[0]), &args[1], Intent::Read)
        .and_then(|(path, beneath)| host.list_dir(&path, beneath));
    Ok(io_result(result, |names| {
        Value::List(Rc::new(names.iter().map(|n| Value::str(n)).collect()))
    }))
}

/// `FsWrite.write(self, path, contents)`.
fn fs_write(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let result = real_path(host, fs_cap(&args[0]), &args[1], Intent::Write)
        .and_then(|(path, beneath)| host.write_file(&path, text(&args, 2), beneath));
    Ok(io_result(result, |()| Value::Unit))
}

// ---- Option and Result -------------------------------------------------

/// `Option.is_some(self)`.
fn option_is_some(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant(builtins::SOME)))
}

/// `Result.is_ok(self)`.
fn result_is_ok(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].is_variant(builtins::OK)))
}

/// `Option.unwrap_or(self, default)`.
fn option_unwrap_or(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, builtins::SOME))
}

/// `Option.ok_or(self, err)`: `Ok` of the value, or `Err(err)` when
/// there is none.
fn option_ok_or(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let err = args.swap_remove(1);
    Ok(match args[0].is_variant(builtins::SOME) {
        true => Value::ok(args[0].fields()[0].clone()),
        false => Value::err(err),
    })
}

/// `Result.unwrap_or(self, default)`.
fn result_unwrap_or(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(unwrap_or(args, builtins::OK))
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

/// `Untrusted<String>.assume_trusted(self)`: the text itself. Trust is a
/// property the checker tracks; at runtime untrusted text is a plain
/// string.
fn string_assume_trusted(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(args.swap_remove(0))
}

/// `String.to_string(self)`: the text itself.
fn string_to_string(_: &mut dyn Host, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
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
    /// one, such as `Connection`. A program that holds one is refused
    /// before it starts (`E0370`), so none of their methods is ever
    /// called. Everything else in the prelude needs an intrinsic.
    fn no_runtime() -> Vec<String> {
        let missing =
            |name: &str| builtins::is_capability(name) && Capability::granted(name).is_none();
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
    fn the_exempt_types_are_the_ones_with_no_runtime_and_have_no_intrinsics() {
        let names = bodiless_prelude_functions();
        let exempt = no_runtime();
        for expected in [
            "Net",
            "NetConnect",
            "NetListen",
            "Spawn",
            "Sched",
            "Connection",
        ] {
            assert!(exempt.iter().any(|e| e == expected), "{exempt:?}");
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
    //! The checker gives `FsRead` no `write` and `FsWrite` no
    //! `read_to_string`, so no source program reaches the runtime's own
    //! access check. It is tested on checked programs built by hand, in
    //! which a read-only filesystem calls `write` and a write-only one
    //! `read_to_string` anyway.

    use oska_check::program::{Callee, Expr, ExprKind, Function, Param, Program};
    use oska_syntax::Span;

    use crate::host::FakeHost;
    use crate::interp::Interpreter;
    use crate::value::{Access, Capability, FsCap, Root, Value};

    fn at(kind: ExprKind) -> Expr {
        Expr {
            kind,
            span: Span { start: 0, end: 1 },
        }
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        at(ExprKind::Call {
            callee: Callee::Intrinsic(name.to_string()),
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
                receiver: false,
                params: vec![Param {
                    name: "fs".to_string(),
                    ty: String::new(),
                    binds: true,
                }],
                body: call(method, args.collect()),
            },
        );
        program
    }

    /// What `program`'s `main` returns, given a filesystem with `access`,
    /// run on `host`.
    fn run_with(program: &Program, access: Access, host: &mut FakeHost) -> Value {
        let fs = Value::Capability(Capability::Fs(FsCap {
            root: Root::Anywhere,
            access,
        }));
        Interpreter::new(program, host)
            .call(&program.functions["main"], vec![fs])
            .expect("no panic")
    }

    fn io_error_message(result: &Value) -> &str {
        assert!(result.is_variant("Err"), "{result:?}");
        result.fields()[0].fields()[0].as_str()
    }

    #[test]
    fn a_read_only_filesystem_refuses_to_write_at_runtime() {
        let program = calling("FsWrite.write", vec![at(ExprKind::Str("x".to_string()))]);
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
    fn a_write_only_filesystem_refuses_to_read_at_runtime() {
        let program = calling("FsRead.read_to_string", Vec::new());
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
