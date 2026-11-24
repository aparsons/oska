// SPDX-License-Identifier: Apache-2.0
//! `Float`: IEEE 754 binary64, whose every result is pinned down to the
//! bits (ADR 0027).
//!
//! A value is held as its bits, so `Value` stays `Eq`. The host's IEEE
//! operations give `+ - * /` and `sqrt`: each is correctly rounded, to
//! nearest with ties to even, and Rust never fuses two of them. A result
//! that is a NaN is the one canonical NaN, [`CANONICAL_NAN`], whichever
//! NaN the host made, so no program can tell which operation made it.

use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::Plain;
use crate::intrinsics::{Args, Table};
use crate::panics::Panic;
use crate::value::Value;

/// The bits of the canonical NaN: the quiet NaN with a zero payload and
/// a clear sign.
pub const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

/// Every intrinsic this module implements, by qualified name.
pub const INTRINSICS: Table = &[
    ("Float.eq", Plain(eq)),
    ("Float.from_bits", Plain(from_bits)),
    ("Float.sqrt", Plain(sqrt)),
    ("Float.to_bits", Plain(to_bits)),
];

/// The bits of `x`, with every NaN made the canonical one.
pub fn bits(x: f64) -> u64 {
    if x.is_nan() {
        CANONICAL_NAN
    } else {
        x.to_bits()
    }
}

/// `lhs op rhs` for the float operators: `+ - * /` give a `Float`, and
/// the comparisons a `Bool`, as IEEE 754 defines them.
pub fn binary(op: BinaryOp, lhs: &Value, rhs: &Value) -> Value {
    let (a, b) = (lhs.as_float(), rhs.as_float());
    match op {
        BinaryOp::Add => Value::Float(bits(a + b)),
        BinaryOp::Sub => Value::Float(bits(a - b)),
        BinaryOp::Mul => Value::Float(bits(a * b)),
        BinaryOp::Div => Value::Float(bits(a / b)),
        BinaryOp::Eq => Value::Bool(a == b),
        BinaryOp::Ne => Value::Bool(a != b),
        BinaryOp::Lt => Value::Bool(a < b),
        BinaryOp::Le => Value::Bool(a <= b),
        BinaryOp::Gt => Value::Bool(a > b),
        BinaryOp::Ge => Value::Bool(a >= b),
        other => unreachable!("the checker allows no `{}` on a `Float`", other.text()),
    }
}

/// Unary `-`.
pub fn neg(operand: &Value) -> Value {
    Value::Float(bits(-operand.as_float()))
}

fn eq(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(binary(BinaryOp::Eq, &args[0], &args[1]))
}

fn sqrt(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Float(bits(args[0].as_float().sqrt())))
}

fn to_bits(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Int(
        i128::from(bits(args[0].as_float())),
        oska_check::program::IntType::U64,
    ))
}

fn from_bits(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let raw = u64::try_from(args[0].as_int()).expect("a `U64` fits `u64`");
    Ok(Value::Float(bits(f64::from_bits(raw))))
}
