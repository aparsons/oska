//! Operators on built-in values.
//!
//! Integers of every type are held as `i128`, which holds all of them, and
//! every arithmetic result is checked against its type's range: arithmetic
//! never wraps. Overflow panics with `P0001`, division by zero with
//! `P0004`, both at the operation's span.

use oska_check::program::IntType;
use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use crate::panics::{codes, Panic};
use crate::value::Value;

/// `lhs op rhs`, both evaluated. `int` is the operands' type for
/// arithmetic.
pub fn binary(
    op: BinaryOp,
    lhs: Value,
    rhs: Value,
    int: Option<&IntType>,
    span: Span,
) -> Result<Value, Panic> {
    let value = match op {
        BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => Value::Bool(rhs.as_bool()),
        BinaryOp::Eq => Value::Bool(lhs == rhs),
        BinaryOp::Ne => Value::Bool(lhs != rhs),
        BinaryOp::Lt => Value::Bool(lhs.as_int() < rhs.as_int()),
        BinaryOp::Le => Value::Bool(lhs.as_int() <= rhs.as_int()),
        BinaryOp::Gt => Value::Bool(lhs.as_int() > rhs.as_int()),
        BinaryOp::Ge => Value::Bool(lhs.as_int() >= rhs.as_int()),
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            let int = int.expect("the checker records the type of integer arithmetic");
            Value::Int(arithmetic(op, lhs.as_int(), rhs.as_int(), int, span)?)
        }
    };
    Ok(value)
}

/// Unary `-` on an integer of type `int`.
pub fn neg(operand: Value, int: &IntType, span: Span) -> Result<Value, Panic> {
    let n = operand.as_int();
    fits(-n, int, || format!("`-{n}` overflows `{}`", int.name), span).map(Value::Int)
}

fn arithmetic(op: BinaryOp, a: i128, b: i128, int: &IntType, span: Span) -> Result<i128, Panic> {
    if matches!(op, BinaryOp::Div | BinaryOp::Rem) && b == 0 {
        return Err(Panic::new(
            codes::DIVISION_BY_ZERO,
            format!("division by zero: `{a} {} 0`", op.text()),
            span,
        ));
    }
    // `i128` overflows only for `U64` products past its range, which are
    // past `U64`'s as well.
    let result = match op {
        BinaryOp::Add => a.checked_add(b),
        BinaryOp::Sub => a.checked_sub(b),
        BinaryOp::Mul => a.checked_mul(b),
        BinaryOp::Div => a.checked_div(b),
        BinaryOp::Rem => a.checked_rem(b),
        _ => unreachable!("not arithmetic: {op:?}"),
    };
    let overflow = || {
        format!(
            "`{a} {} {b}` overflows `{}`, which ranges from {} to {}",
            op.text(),
            int.name,
            int.min,
            int.max
        )
    };
    match result {
        Some(n) => fits(n, int, overflow, span),
        None => Err(Panic::new(codes::OVERFLOW, overflow(), span)),
    }
}

/// `n` when it fits `int`, otherwise an overflow panic with `message`.
fn fits(
    n: i128,
    int: &IntType,
    message: impl FnOnce() -> String,
    span: Span,
) -> Result<i128, Panic> {
    if (int.min..=int.max).contains(&n) {
        Ok(n)
    } else {
        Err(Panic::new(codes::OVERFLOW, message(), span))
    }
}
