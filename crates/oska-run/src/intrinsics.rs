//! Runtime implementations of body-less prelude functions, keyed by the
//! qualified name the checked program calls them by (`Stdout.println`).
//!
//! To add one: write a function with the [`Intrinsic`] signature and add
//! it to [`INTRINSICS`]. A method's receiver is its first argument.

use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Unwind;
use crate::value::Value;

/// An intrinsic: the host, the arguments (the receiver first), and the
/// call's span, for panics.
pub type Intrinsic = fn(&mut dyn Host, Vec<Value>, Span) -> Result<Value, Unwind>;

/// Every intrinsic, by qualified name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[("Stdout.println", stdout_println)];

/// The intrinsic named `name`.
pub fn lookup(name: &str) -> Option<Intrinsic> {
    INTRINSICS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

/// The text of the string argument at `index`.
fn text(args: &[Value], index: usize) -> &str {
    match args.get(index) {
        Some(Value::Str(s)) => s,
        other => unreachable!("the checker guarantees a string argument, found {other:?}"),
    }
}

/// `Stdout.println(self, line)`: the line and a newline.
fn stdout_println(host: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Unwind> {
    let line = format!("{}\n", text(&args, 1));
    Ok(match host.write_stdout(&line) {
        Ok(()) => Value::ok(Value::Unit),
        Err(err) => Value::err(Value::io_error(&err.to_string())),
    })
}
