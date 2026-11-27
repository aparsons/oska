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
use crate::memory;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// The bits of the canonical NaN: the quiet NaN with a zero payload and
/// a clear sign.
pub const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

/// Every intrinsic this module implements, by qualified name.
pub const INTRINSICS: Table = &[
    ("Float.approx_eq", Plain(approx_eq)),
    ("Float.eq", Plain(eq)),
    ("Float.fmt", Plain(fmt)),
    ("Float.from_bits", Plain(from_bits)),
    ("Float.sqrt", Plain(sqrt)),
    ("Float.to_bits", Plain(to_bits)),
    ("Float.to_int", Plain(to_int)),
    ("Float.to_string", Plain(to_string)),
    ("Float.total_cmp", Plain(total_cmp)),
    ("Int.to_float", Plain(int_to_float)),
    ("Int.to_float_rounded", Plain(int_to_float_rounded)),
    ("String.parse_float", Plain(parse_float)),
    ("String.parse_float_lenient", Plain(parse_float_lenient)),
];

/// The longest text `parse_float` takes, and `parse_float_lenient`.
pub const PARSE_LIMIT: usize = 400;
pub const PARSE_LIMIT_LENIENT: usize = 65_536;

/// The most digits `fmt` prints after the point.
pub const MAX_FMT_DIGITS: i128 = 340;

/// The largest magnitude `Int.to_float` converts: 2^53.
const EXACT_LIMIT: i128 = 1 << 53;

/// The text of `x`: the shortest digits that parse back to `x`, in plain
/// notation for a decimal exponent from -5 to 15 and as `d.ddde<exp>`
/// otherwise. `NaN`, `inf`, `-inf`, `0.0` and `-0.0` are as written.
/// Rust's `{:e}` gives the shortest digits and is pure Rust, with no call
/// into the C library.
pub fn text(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x.is_infinite() {
        return format!("{sign}inf");
    }
    if x == 0.0 {
        return format!("{sign}0.0");
    }
    let shortest = format!("{:e}", x.abs());
    let (mantissa, exponent) = shortest
        .split_once('e')
        .expect("`{:e}` of a finite number has an exponent");
    let exponent: i32 = exponent.parse().expect("an exponent is a number");
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let body = if (-5..16).contains(&exponent) {
        if exponent >= 0 {
            let point = exponent as usize + 1;
            let whole = format!("{digits:0<point$}");
            let fraction = digits.get(point..).unwrap_or("");
            format!(
                "{}.{}",
                &whole[..point],
                if fraction.is_empty() { "0" } else { fraction }
            )
        } else {
            format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
        }
    } else if digits.len() == 1 {
        format!("{digits}e{exponent}")
    } else {
        format!("{}.{}e{exponent}", &digits[..1], &digits[1..])
    };
    format!("{sign}{body}")
}

/// The number `text` spells, correctly rounded, or `None`. The grammar is
/// an optional `-`, digits, an optional `.` and digits, and an optional
/// exponent. `lenient` also takes a leading `+`, `nan`, `inf` and
/// `infinity`, and a number beyond `Float` as an infinity. Otherwise a
/// number beyond `Float` is `None`.
pub fn parse(text: &str, lenient: bool) -> Option<f64> {
    if text.len()
        > if lenient {
            PARSE_LIMIT_LENIENT
        } else {
            PARSE_LIMIT
        }
    {
        return None;
    }
    let (negative, rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' if lenient => (false, &text[1..]),
        _ => (false, text),
    };
    if lenient {
        let word = rest.to_ascii_lowercase();
        if word == "nan" {
            return Some(f64::NAN);
        }
        if word == "inf" || word == "infinity" {
            return Some(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
    }
    let bytes = rest.as_bytes();
    let digits = |at: usize| {
        bytes[at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut at = digits(0);
    if at == 0 {
        return None;
    }
    if bytes.get(at) == Some(&b'.') {
        let fraction = digits(at + 1);
        if fraction == 0 {
            return None;
        }
        at += 1 + fraction;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent == 0 {
            return None;
        }
        at += exponent;
    }
    if at != bytes.len() {
        return None;
    }
    let magnitude: f64 = rest.parse().ok()?;
    if !lenient && !magnitude.is_finite() {
        return None;
    }
    Some(if negative { -magnitude } else { magnitude })
}

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

fn to_string(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::text(&text(args[0].as_float()), span)
}

fn fmt(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let digits = args[1].as_int();
    if !(0..=MAX_FMT_DIGITS).contains(&digits) {
        return Err(Panic::new(
            codes::FMT_DIGITS,
            format!("`fmt` was asked for {digits} digits, and takes from 0 to {MAX_FMT_DIGITS}"),
            span,
        ));
    }
    let x = args[0].as_float();
    let shown = if x.is_finite() {
        format!("{x:.*}", digits as usize)
    } else {
        text(x)
    };
    memory::text(&shown, span)
}

fn to_int(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let x = args[0].as_float();
    // Both bounds are exact doubles: -2^63 and 2^63.
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&x) {
        return Ok(Value::int(i128::from(x as i64)));
    }
    Err(Panic::new(
        codes::FLOAT_TO_INT,
        format!(
            "`to_int()` of {} does not fit `Int`, which ranges from {} to {}",
            text(x),
            i64::MIN,
            i64::MAX
        ),
        span,
    ))
}

fn int_to_float(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let n = args[0].as_int();
    if n.abs() > EXACT_LIMIT {
        return Err(Panic::new(
            codes::INT_TO_FLOAT,
            format!(
                "`to_float()` of {n} would round: it is beyond 2^53 ({EXACT_LIMIT}); `to_float_rounded()` rounds it"
            ),
            span,
        ));
    }
    Ok(Value::Float(bits(n as f64)))
}

fn int_to_float_rounded(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    // An `as` cast from an integer rounds to nearest, ties to even. This
    // relies on `Int` being 64-bit: the `as i64` keeps every bit, and the
    // native `std.rt` version below is written for 64 bits.
    Ok(Value::Float(bits(args[0].as_int() as i64 as f64)))
}

fn total_cmp(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let order = args[0].as_float().total_cmp(&args[1].as_float());
    Ok(Value::int(order as i128))
}

fn approx_eq(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let (a, b) = (args[0].as_float(), args[1].as_float());
    let (relative, absolute) = (args[2].as_float(), args[3].as_float());
    let close = if a == b {
        true
    } else if !a.is_finite() || !b.is_finite() {
        false
    } else {
        let gap = (a - b).abs();
        gap <= relative * a.abs().max(b.abs()) || gap <= absolute
    };
    Ok(Value::Bool(close))
}

fn parse_float(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let parsed = parse(args[0].as_str(), false);
    Ok(Value::option(parsed.map(|x| Value::Float(bits(x)))))
}

fn parse_float_lenient(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let parsed = parse(args[0].as_str(), true);
    Ok(Value::option(parsed.map(|x| Value::Float(bits(x)))))
}
