//! Runtime implementations of body-less prelude functions, keyed by the
//! qualified name the checked program calls them by (`Stdout.println`).
//!
//! To add one: write a function with the [`Intrinsic`] signature and add
//! it to [`INTRINSICS`]. A method's receiver is its first argument.

use oska_syntax::Span;

use crate::host::Host;
use crate::panics::{codes, Panic};
use crate::data;
use crate::value::Value;

/// An intrinsic: the host, the arguments (the receiver first), and the
/// call's span, for panics.
pub type Intrinsic = fn(&mut dyn Host, Vec<Value>, Span) -> Result<Value, Panic>;

/// Every intrinsic, by qualified name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    ("Int.to_string", int_to_string),
    ("Stdout.println", stdout_println),
    ("String.to_string", string_to_string),
    ("todo", todo),
];

/// The intrinsic named `name`.
pub fn lookup(name: &str) -> Option<Intrinsic> {
    INTRINSICS
        .iter()
        .chain(data::INTRINSICS)
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// The text of the string argument at `index`.
fn text(args: &[Value], index: usize) -> &str {
    match args.get(index) {
        Some(Value::Str(s)) => s,
        other => unreachable!("the checker guarantees a string argument, found {other:?}"),
    }
}

/// `Stdout.println(self, line)`: the line and a newline.
fn stdout_println(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let line = format!("{}\n", text(&args, 1));
    Ok(match host.write_stdout(&line) {
        Ok(()) => Value::ok(Value::Unit),
        Err(err) => Value::err(Value::io_error(&err.to_string())),
    })
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
