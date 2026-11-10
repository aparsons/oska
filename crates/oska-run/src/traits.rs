//! Intrinsics for the prelude's own trait impls, such as `Eq for Int` or
//! `Display for IoError`, named `Type.method` like any prelude method.
//!
//! Generic code reaches them by a value's runtime type tag (ADR 0007),
//! and so do the impls for `Option`, `Result`, and `List` for their
//! items: `Some(p) == Some(q)` compares `p` and `q` through the impl for
//! their own type, which may be one the program declares.

use std::rc::Rc;

use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Interpreter;
use crate::intrinsics::{HigherOrder, Intrinsic};
use crate::panics::Panic;
use crate::value::Value;

/// Every trait intrinsic that needs only its arguments, by qualified
/// name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    // `()` is `Eq` without an impl: generic code compares it by its tag.
    ("().eq", same),
    ("Bool.eq", same),
    ("I16.eq", same),
    ("I32.eq", same),
    ("I64.eq", same),
    ("I8.eq", same),
    ("Int.eq", same),
    ("IoError.eq", same),
    ("IoError.to_string", io_error_to_string),
    ("Path.eq", same),
    ("String.eq", same),
    ("U16.eq", same),
    ("U32.eq", same),
    ("U64.eq", same),
    ("U8.eq", same),
];

/// Every trait intrinsic that calls back into the program, by qualified
/// name.
pub const HIGHER_ORDER: &[(&str, HigherOrder)] = &[
    ("List.eq", items_eq),
    ("Option.eq", items_eq),
    ("Result.eq", items_eq),
];

/// `eq(self, other)` for a type whose values are equal exactly when
/// they hold the same data: integers, `Bool`, text, `Path`, `IoError`.
fn same(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0] == args[1]))
}

/// `IoError.to_string(self)`: its message.
fn io_error_to_string(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(args[0].field(0))
}

/// `eq(self, other)` for `Option`, `Result`, and `List`: the same variant
/// or length, with each item equal to the other's through `Eq` for its
/// type.
fn items_eq(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let (lhs, rhs): (&Rc<Vec<Value>>, &Rc<Vec<Value>>) = match (&args[0], &args[1]) {
        (Value::List(lhs), Value::List(rhs)) => (lhs, rhs),
        (
            Value::Variant {
                variant: lv,
                fields: lhs,
                ..
            },
            Value::Variant {
                variant: rv,
                fields: rhs,
                ..
            },
        ) => {
            if lv != rv {
                return Ok(Value::Bool(false));
            }
            (lhs, rhs)
        }
        other => unreachable!("the checker guarantees two lists or two variants, found {other:?}"),
    };
    if lhs.len() != rhs.len() {
        return Ok(Value::Bool(false));
    }
    for (l, r) in lhs.iter().zip(rhs.iter()) {
        if !interp
            .call_trait("Eq", "eq", l.clone(), vec![r.clone()], span)?
            .as_bool()
        {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
