//! Wrapping arithmetic and conversions between integer types.
//!
//! `wrapping_add`, `wrapping_sub` and `wrapping_mul` on `U8`, `U32` and
//! `U64` keep the low bits of the exact result: arithmetic modulo 2 to
//! the type's width. `to_<type>()` converts between `Int`, `U8`, `U16`,
//! `U32` and `U64`, and panics with `P0001` when the value does not fit
//! its target; `wrapping_to_<type>()`, from a wider unsigned type to a
//! narrower one, keeps the value's low bits instead. Each intrinsic reads
//! its receiver's type from the value, so one function serves every type
//! that has the method.

use oska_check::program::IntType;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::Plain;
use crate::intrinsics::Table;
use crate::ops::mask;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every intrinsic this module implements, by qualified name.
pub const INTRINSICS: Table = &[
    ("Int.to_u16", Plain(to_u16)),
    ("Int.to_u32", Plain(to_u32)),
    ("Int.to_u64", Plain(to_u64)),
    ("Int.to_u8", Plain(to_u8)),
    ("U16.to_int", Plain(to_int)),
    ("U16.to_u32", Plain(to_u32)),
    ("U16.to_u64", Plain(to_u64)),
    ("U16.to_u8", Plain(to_u8)),
    ("U16.wrapping_to_u8", Plain(wrapping_to_u8)),
    ("U32.to_int", Plain(to_int)),
    ("U32.to_u16", Plain(to_u16)),
    ("U32.to_u64", Plain(to_u64)),
    ("U32.to_u8", Plain(to_u8)),
    ("U32.wrapping_add", Plain(wrapping_add)),
    ("U32.wrapping_mul", Plain(wrapping_mul)),
    ("U32.wrapping_sub", Plain(wrapping_sub)),
    ("U32.wrapping_to_u16", Plain(wrapping_to_u16)),
    ("U32.wrapping_to_u8", Plain(wrapping_to_u8)),
    ("U64.to_int", Plain(to_int)),
    ("U64.to_u16", Plain(to_u16)),
    ("U64.to_u32", Plain(to_u32)),
    ("U64.to_u8", Plain(to_u8)),
    ("U64.wrapping_add", Plain(wrapping_add)),
    ("U64.wrapping_mul", Plain(wrapping_mul)),
    ("U64.wrapping_sub", Plain(wrapping_sub)),
    ("U64.wrapping_to_u16", Plain(wrapping_to_u16)),
    ("U64.wrapping_to_u32", Plain(wrapping_to_u32)),
    ("U64.wrapping_to_u8", Plain(wrapping_to_u8)),
    ("U8.to_int", Plain(to_int)),
    ("U8.to_u16", Plain(to_u16)),
    ("U8.to_u32", Plain(to_u32)),
    ("U8.to_u64", Plain(to_u64)),
    ("U8.wrapping_add", Plain(wrapping_add)),
    ("U8.wrapping_mul", Plain(wrapping_mul)),
    ("U8.wrapping_sub", Plain(wrapping_sub)),
];

fn wrapping_add(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping(args, |a, b| a.wrapping_add(b)))
}

fn wrapping_sub(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping(args, |a, b| a.wrapping_sub(b)))
}

fn wrapping_mul(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping(args, |a, b| a.wrapping_mul(b)))
}

/// `op` on the receiver and the argument, `args`, as `u128`s, whose
/// operations wrap modulo 2 to 128, a multiple of every width here, so
/// the low bits are the result.
fn wrapping(args: Vec<Value>, op: fn(u128, u128) -> u128) -> Value {
    let [a, b]: [Value; 2] = args.try_into().expect("a receiver and an argument");
    let Value::Int(a, int) = a else {
        unreachable!("the checker allows wrapping arithmetic on integers only");
    };
    // Both are unsigned, so non-negative.
    let exact = op(a as u128, b.as_int() as u128);
    Value::Int(exact as i128 & mask(int), int)
}

fn to_int(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    convert(args, IntType::Int, span)
}

fn to_u8(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    convert(args, IntType::U8, span)
}

fn to_u16(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    convert(args, IntType::U16, span)
}

fn to_u32(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    convert(args, IntType::U32, span)
}

fn to_u64(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    convert(args, IntType::U64, span)
}

/// The receiver, `args`, as an integer of type `to`: `P0001` when it does
/// not fit.
fn convert(args: Vec<Value>, to: IntType, span: Span) -> Result<Value, Panic> {
    let n = args[0].as_int();
    if (to.min()..=to.max()).contains(&n) {
        return Ok(Value::Int(n, to));
    }
    Err(Panic::new(
        codes::OVERFLOW,
        format!(
            "`to_{}()` of {n} overflows `{}`, which ranges from {} to {}",
            to.name().to_lowercase(),
            to.name(),
            to.min(),
            to.max()
        ),
        span,
    ))
}

fn wrapping_to_u8(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping_to(args, IntType::U8))
}

fn wrapping_to_u16(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping_to(args, IntType::U16))
}

fn wrapping_to_u32(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(wrapping_to(args, IntType::U32))
}

/// The low bits of the receiver, `args`, as an integer of type `to`.
fn wrapping_to(args: Vec<Value>, to: IntType) -> Value {
    Value::Int(args[0].as_int() & mask(to), to)
}
