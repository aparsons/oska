// SPDX-License-Identifier: Apache-2.0
//! Intrinsics for `Bytes`, the prelude's packed buffer of bytes, and
//! `String.to_bytes`.
//!
//! A `Bytes` is a reference-counted vector of bytes (ADR 0001), as a list
//! is one of values: reading one shares it, and a mutating method writes
//! through [`std::rc::Rc::make_mut`], in place when nothing else holds it
//! and on a copy of its own when something does (`memory::write_bytes`).

use std::fmt::Write as _;

use oska_check::program::IntType;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::{Mutating, Plain};
use crate::intrinsics::{Args, Table};
use crate::memory;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every `Bytes` intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("Bytes.append", Mutating(bytes_append)),
    ("Bytes.assume_trusted", Plain(bytes_assume_trusted)),
    ("Bytes.at", Plain(bytes_at)),
    ("Bytes.eq", Plain(bytes_eq)),
    ("Bytes.from_hex", Plain(bytes_from_hex)),
    ("Bytes.get", Plain(bytes_get)),
    ("Bytes.hash", Plain(bytes_hash)),
    ("Bytes.len", Plain(bytes_len)),
    ("Bytes.new", Plain(bytes_new)),
    ("Bytes.push", Mutating(bytes_push)),
    ("Bytes.push_u16_be", Mutating(push_u16_be)),
    ("Bytes.push_u16_le", Mutating(push_u16_le)),
    ("Bytes.push_u32_be", Mutating(push_u32_be)),
    ("Bytes.push_u32_le", Mutating(push_u32_le)),
    ("Bytes.push_u64_be", Mutating(push_u64_be)),
    ("Bytes.push_u64_le", Mutating(push_u64_le)),
    ("Bytes.read_u16_be", Plain(read_u16_be)),
    ("Bytes.read_u16_le", Plain(read_u16_le)),
    ("Bytes.read_u32_be", Plain(read_u32_be)),
    ("Bytes.read_u32_le", Plain(read_u32_le)),
    ("Bytes.read_u64_be", Plain(read_u64_be)),
    ("Bytes.read_u64_le", Plain(read_u64_le)),
    ("Bytes.set", Mutating(bytes_set)),
    ("Bytes.slice", Plain(bytes_slice)),
    ("Bytes.to_hex", Plain(bytes_to_hex)),
    ("Bytes.to_text", Plain(bytes_to_text)),
    ("Bytes.zeroed", Plain(bytes_zeroed)),
    ("String.to_bytes", Plain(string_to_bytes)),
];

/// The longest `Bytes`, in bytes: 256 MiB, the longest text
/// `String.repeat` builds.
pub const MAX_BYTES: usize = 256 * 1024 * 1024;

/// `P0043` for a buffer that would be `len` bytes long, at `span`.
fn too_long(len: impl std::fmt::Display, span: Span) -> Panic {
    Panic::new(
        codes::BYTES_TOO_LONG,
        format!("a buffer of {len} bytes is too long"),
        span,
    )
}

/// The length `len` plus `extra`, or `P0043` when that is past
/// [`MAX_BYTES`].
fn grown(len: usize, extra: usize, span: Span) -> Result<(), Panic> {
    let total = len as u128 + extra as u128;
    if total > MAX_BYTES as u128 {
        return Err(too_long(total, span));
    }
    Ok(())
}

/// The bytes of the receiver of a mutating method.
fn receiver(bytes: &mut Value) -> &mut std::rc::Rc<Vec<u8>> {
    match bytes {
        Value::Bytes(data) => data,
        other => unreachable!("the checker guarantees bytes, found {other:?}"),
    }
}

/// `Bytes.new()`: an empty buffer.
fn bytes_new(_: &mut dyn Host, _: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::bytes_of_len(0, span, Vec::new)
}

/// `Bytes.zeroed(len)`: `len` zero bytes. A negative length panics with
/// `P0042`, one past [`MAX_BYTES`] with `P0043`.
fn bytes_zeroed(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let len = checked_len(args[0].as_int(), span)?;
    memory::bytes_of_len(len, span, || vec![0; len])
}

/// `len` as a buffer length, or `P0042` for a negative one and `P0043` for
/// one past [`MAX_BYTES`]: the one place `Bytes.zeroed` and
/// `Random.secure_bytes` refuse a length.
pub(crate) fn checked_len(len: i128, span: Span) -> Result<usize, Panic> {
    if len < 0 {
        return Err(Panic::new(
            codes::BYTES_OUT_OF_RANGE,
            format!("cannot make a buffer of {len} bytes"),
            span,
        ));
    }
    if len > MAX_BYTES as i128 {
        return Err(too_long(len, span));
    }
    Ok(len as usize)
}

/// The value of the hexadecimal digit `digit`, in either case.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// `Bytes.from_hex(text)`: the bytes two hexadecimal digits each spell,
/// or `None` for an odd number of digits or anything that is not one.
fn bytes_from_hex(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let text = args[0].as_str().as_bytes();
    if !text.len().is_multiple_of(2) {
        return Ok(Value::none());
    }
    let decoded: Option<Vec<u8>> = text
        .chunks(2)
        .map(|pair| Some(hex_value(pair[0])? << 4 | hex_value(pair[1])?))
        .collect();
    match decoded {
        Some(data) => Ok(Value::some(memory::bytes_of_len(data.len(), span, || {
            data
        })?)),
        None => Ok(Value::none()),
    }
}

/// `Bytes.len(self)`: the number of bytes.
fn bytes_len(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(args[0].as_bytes().len() as i128))
}

/// A `U8`.
fn byte(b: u8) -> Value {
    Value::Int(i128::from(b), IntType::U8)
}

/// `Bytes.get(self, index)`: the byte at `index`, or `None`.
fn bytes_get(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let found = usize::try_from(args[1].as_int())
        .ok()
        .and_then(|i| args[0].as_bytes().get(i).copied());
    Ok(Value::option(found.map(byte)))
}

/// `P0002` for `index` into a buffer of `len` bytes, at `span`.
fn index_out_of_bounds(index: i128, len: usize, span: Span) -> Panic {
    Panic::new(
        codes::INDEX_OUT_OF_BOUNDS,
        format!("index {index} is out of bounds for bytes of length {len}"),
        span,
    )
}

/// `Bytes.at(self, index)`: the byte at `index`. Out of bounds, it panics
/// with `P0002`.
fn bytes_at(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let data = args[0].as_bytes();
    let index = args[1].as_int();
    usize::try_from(index)
        .ok()
        .and_then(|i| data.get(i).copied())
        .map(byte)
        .ok_or_else(|| index_out_of_bounds(index, data.len(), span))
}

/// `Bytes.slice(self, start, end)`: the bytes from `start` up to `end`.
/// Bounds out of range, or reversed, panic with `P0042`.
fn bytes_slice(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let data = args[0].as_bytes();
    let (start, end) = (args[1].as_int(), args[2].as_int());
    let bounds = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .filter(|&(start, end)| start <= end && end <= data.len());
    let Some((start, end)) = bounds else {
        return Err(Panic::new(
            codes::BYTES_OUT_OF_RANGE,
            format!(
                "cannot slice bytes {start}..{end} of a buffer of {} bytes",
                data.len()
            ),
            span,
        ));
    };
    memory::bytes_of_len(end - start, span, || data[start..end].to_vec())
}

/// The `width` bytes at `offset` of the receiver in `args`, or `P0042`
/// when they do not all lie inside it.
fn read(args: &[Value], width: usize, span: Span) -> Result<&[u8], Panic> {
    let data = args[0].as_bytes();
    let offset = args[1].as_int();
    usize::try_from(offset)
        .ok()
        .filter(|&at| at <= data.len() && data.len() - at >= width)
        .map(|at| &data[at..at + width])
        .ok_or_else(|| {
            Panic::new(
                codes::BYTES_OUT_OF_RANGE,
                format!(
                    "cannot read {width} bytes at offset {offset} of a buffer of {} bytes",
                    data.len()
                ),
                span,
            )
        })
}

/// The unsigned integer of type `int` that `bytes` store, most
/// significant byte first when `big`.
fn unsigned(bytes: &[u8], big: bool, int: IntType) -> Value {
    let fold = |n: u128, b: &u8| n << 8 | u128::from(*b);
    let n = if big {
        bytes.iter().fold(0, fold)
    } else {
        bytes.iter().rev().fold(0, fold)
    };
    Value::Int(n as i128, int)
}

fn read_u16_be(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 2, span)?, true, IntType::U16))
}

fn read_u16_le(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 2, span)?, false, IntType::U16))
}

fn read_u32_be(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 4, span)?, true, IntType::U32))
}

fn read_u32_le(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 4, span)?, false, IntType::U32))
}

fn read_u64_be(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 8, span)?, true, IntType::U64))
}

fn read_u64_le(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    Ok(unsigned(read(args, 8, span)?, false, IntType::U64))
}

/// `Bytes.to_hex(self)`: two lower-case hexadecimal digits a byte.
fn bytes_to_hex(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let data = args[0].as_bytes();
    memory::text_of_len(2 * data.len(), span, || hex(data))
}

/// `data` in lower-case hexadecimal, as `to_hex` and `dbg` show it.
pub(crate) fn hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(2 * data.len());
    for b in data {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `Bytes.to_text(self)`: the text the bytes encode in UTF-8, or `None`.
fn bytes_to_text(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    match std::str::from_utf8(args[0].as_bytes()) {
        Ok(text) => Ok(Value::some(memory::text(text, span)?)),
        Err(_) => Ok(Value::none()),
    }
}

/// `Untrusted<Bytes>.assume_trusted(self)`: the bytes themselves. Trust
/// is a property the checker tracks; at run time untrusted bytes are
/// plain bytes.
fn bytes_assume_trusted(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args.swap_remove(0))
}

/// `String.to_bytes(self)`: its UTF-8 encoding.
fn string_to_bytes(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let text = args[0].as_str();
    memory::bytes_of_len(text.len(), span, || text.as_bytes().to_vec())
}

/// `Bytes.eq(self, other)`: the same bytes.
fn bytes_eq(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].as_bytes() == args[1].as_bytes()))
}

/// The value `ct_select` picks: `if_true` when `cond` holds, else
/// `if_false`. Two `Bytes` of different lengths panic with `P0042`.
pub fn select(cond: bool, if_true: Value, if_false: Value, span: Span) -> Result<Value, Panic> {
    if let (Value::Bytes(a), Value::Bytes(b)) = (&if_true, &if_false) {
        if a.len() != b.len() {
            return Err(Panic::new(
                codes::BYTES_OUT_OF_RANGE,
                format!(
                    "cannot pick between buffers of {} and {} bytes without revealing which",
                    a.len(),
                    b.len()
                ),
                span,
            ));
        }
    }
    Ok(if cond { if_true } else { if_false })
}

/// `Bytes.hash(self)`: the 64-bit FNV-1a hash of the bytes, as an `Int`.
fn bytes_hash(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let hash = args[0]
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    Ok(Value::int(i128::from(hash as i64)))
}

/// Add `extra` to the end of the receiver, copying it first when it is
/// shared; `P0043` when it would grow past [`MAX_BYTES`].
fn add(bytes: &mut Value, extra: &[u8], span: Span) -> Result<Value, Panic> {
    let data = receiver(bytes);
    grown(data.len(), extra.len(), span)?;
    memory::write_bytes(data, extra.len(), span, |data| {
        data.extend_from_slice(extra);
    })?;
    Ok(Value::Unit)
}

/// The low byte of the `U8` argument.
fn byte_arg(value: &Value) -> u8 {
    value.as_int() as u8
}

/// `Bytes.push(var self, byte)`: `byte` added at the end.
fn bytes_push(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &[byte_arg(&args[0])], span)
}

/// `Bytes.append(var self, other)`: `other`'s bytes added at the end.
fn bytes_append(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let other = std::rc::Rc::clone(args[0].as_bytes());
    add(bytes, &other, span)
}

/// `Bytes.set(var self, index, byte)`: the byte at `index` replaced.
/// Out of bounds, it panics with `P0002` and changes nothing.
fn bytes_set(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let data = receiver(bytes);
    let index = args[0].as_int();
    let Some(at) = usize::try_from(index).ok().filter(|&i| i < data.len()) else {
        return Err(index_out_of_bounds(index, data.len(), span));
    };
    let value = byte_arg(&args[1]);
    memory::write_bytes(data, 0, span, |data| data[at] = value)?;
    Ok(Value::Unit)
}

/// The value of the unsigned integer argument.
fn word(value: &Value) -> u64 {
    value.as_int() as u64
}

fn push_u16_be(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &(word(&args[0]) as u16).to_be_bytes(), span)
}

fn push_u16_le(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &(word(&args[0]) as u16).to_le_bytes(), span)
}

fn push_u32_be(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &(word(&args[0]) as u32).to_be_bytes(), span)
}

fn push_u32_le(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &(word(&args[0]) as u32).to_le_bytes(), span)
}

fn push_u64_be(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &word(&args[0]).to_be_bytes(), span)
}

fn push_u64_le(bytes: &mut Value, args: &mut Args, span: Span) -> Result<Value, Panic> {
    add(bytes, &word(&args[0]).to_le_bytes(), span)
}
