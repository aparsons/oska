//! Operators on built-in values.
//!
//! Integers of every type are held as `i128`, which holds all of them, and
//! every arithmetic result is checked against its type's range: arithmetic
//! never wraps. Overflow panics with `P0001`, division by zero with
//! `P0004`, both at the operation's span. The bitwise operators, on `U8`,
//! `U32` and `U64`, never panic, but a shift or rotation by an amount
//! below 0 or not below the type's width does, with `P0008`: the
//! amount is never taken modulo the width.

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
    int: Option<IntType>,
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
            Value::Int(arithmetic(op, lhs.as_int(), rhs.as_int(), int, span)?, int)
        }
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            let int = int.expect("the checker records the type of a bitwise operator");
            let (a, b) = (lhs.as_int(), rhs.as_int());
            let value = match op {
                BinaryOp::BitAnd => a & b,
                BinaryOp::BitOr => a | b,
                BinaryOp::BitXor => a ^ b,
                BinaryOp::Shl => (a << amount(op.text(), b, int, span)?) & mask(int),
                _ => a >> amount(op.text(), b, int, span)?,
            };
            Value::Int(value, int)
        }
    };
    Ok(value)
}

/// Unary `-` on an integer of type `int`.
pub fn neg(operand: Value, int: IntType, span: Span) -> Result<Value, Panic> {
    let n = operand.as_int();
    fits(
        -n,
        int,
        || format!("`-{n}` overflows `{}`", int.name()),
        span,
    )
    .map(|n| Value::Int(n, int))
}

/// `~operand` on an integer of type `int`: each of its bits flipped.
pub fn bit_not(operand: Value, int: IntType) -> Value {
    Value::Int(operand.as_int() ^ mask(int), int)
}

/// `x.rotl(n)` (`left`) or `x.rotr(n)` on an integer of type `int`.
pub fn rotate(x: Value, n: Value, left: bool, span: Span) -> Result<Value, Panic> {
    let Value::Int(x, int) = x else {
        unreachable!("the checker allows a rotation on integers only");
    };
    let name = if left { "rotl" } else { "rotr" };
    let n = amount(name, n.as_int(), int, span)?;
    let bits = int.bits();
    let back = (bits - n) % bits;
    let value = if left {
        (x << n | x >> back) & mask(int)
    } else {
        (x >> n | x << back) & mask(int)
    };
    Ok(Value::Int(value, int))
}

/// Every bit of an integer of type `int` set: an unsigned value `&` it
/// is that value modulo 2 to the type's width.
pub(crate) fn mask(int: IntType) -> i128 {
    (1i128 << int.bits()) - 1
}

/// `n` as the amount of a shift or rotation, `op`, of an integer of type
/// `int`: `P0008` unless it is from 0 to the type's width less 1.
fn amount(op: &str, n: i128, int: IntType, span: Span) -> Result<u32, Panic> {
    let bits = int.bits();
    match u32::try_from(n) {
        Ok(n) if n < bits => Ok(n),
        _ => Err(Panic::new(
            codes::SHIFT_OUT_OF_RANGE,
            format!(
                "`{op}` by {n}: the amount must be from 0 to {} for `{}`",
                bits - 1,
                int.name()
            ),
            span,
        )),
    }
}

fn arithmetic(op: BinaryOp, a: i128, b: i128, int: IntType, span: Span) -> Result<i128, Panic> {
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
            int.name(),
            int.min(),
            int.max()
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
    int: IntType,
    message: impl FnOnce() -> String,
    span: Span,
) -> Result<i128, Panic> {
    if (int.min()..=int.max()).contains(&n) {
        Ok(n)
    } else {
        Err(Panic::new(codes::OVERFLOW, message(), span))
    }
}
