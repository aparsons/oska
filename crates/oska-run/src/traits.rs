// SPDX-License-Identifier: Apache-2.0
//! Intrinsics for the prelude's own trait impls, such as `Eq for Int` or
//! `Display for IoError`, named `Type.method` like any prelude method.
//!
//! Generic code reaches them by a value's runtime type tag (ADR 0007),
//! and so do the impls for `Option`, `Result`, and `List` for their
//! items: `Some(p) == Some(q)` compares `p` and `q` through the impl for
//! their own type, which may be one the program declares.

use std::rc::Rc;

use oska_check::builtins;
use oska_check::hashing;
use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Interpreter;
use crate::intrinsics::Intrinsic::{HigherOrder, Plain};
use crate::intrinsics::{Args, Table};
use crate::panics::Panic;
use crate::value::Value;

/// Every trait intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    // `()` is `Eq` without an impl: generic code compares it by its tag.
    ("().eq", Plain(same)),
    ("Bool.eq", Plain(same)),
    ("I16.eq", Plain(same)),
    ("I32.eq", Plain(same)),
    ("I64.eq", Plain(same)),
    ("I8.eq", Plain(same)),
    ("Int.eq", Plain(same)),
    ("IoError.eq", Plain(same)),
    ("IoError.to_string", Plain(io_error_to_string)),
    ("Path.eq", Plain(same)),
    ("String.eq", Plain(same)),
    ("U16.eq", Plain(same)),
    ("U32.eq", Plain(same)),
    ("U64.eq", Plain(same)),
    ("U8.eq", Plain(same)),
    ("Bool.cmp", Plain(cmp)),
    ("I16.cmp", Plain(cmp)),
    ("I32.cmp", Plain(cmp)),
    ("I64.cmp", Plain(cmp)),
    ("I8.cmp", Plain(cmp)),
    ("Int.cmp", Plain(cmp)),
    ("String.cmp", Plain(cmp)),
    ("List.cmp", HigherOrder(items_cmp)),
    ("Option.cmp", HigherOrder(items_cmp)),
    ("U16.cmp", Plain(cmp)),
    ("U32.cmp", Plain(cmp)),
    ("U64.cmp", Plain(cmp)),
    ("U8.cmp", Plain(cmp)),
    ("Bool.hash", Plain(word_hash)),
    ("I16.hash", Plain(word_hash)),
    ("I32.hash", Plain(word_hash)),
    ("I64.hash", Plain(word_hash)),
    ("I8.hash", Plain(word_hash)),
    ("Int.hash", Plain(word_hash)),
    ("U16.hash", Plain(word_hash)),
    ("U32.hash", Plain(word_hash)),
    ("U64.hash", Plain(word_hash)),
    ("U8.hash", Plain(word_hash)),
    ("String.hash", Plain(text_hash)),
    ("Path.hash", Plain(path_hash)),
    // A struct or enum without an `impl Hash` derives it.
    (DERIVED_HASH, HigherOrder(items_hash)),
    ("List.hash", HigherOrder(items_hash)),
    ("Option.hash", HigherOrder(items_hash)),
    ("List.eq", HigherOrder(items_eq)),
    ("Option.eq", HigherOrder(items_eq)),
    ("Result.eq", HigherOrder(items_eq)),
];

/// The intrinsic a struct or enum that derives `Hash` is hashed by
/// (`Interpreter::dispatch` finds it when the type has no impl).
pub const DERIVED_HASH: &str = "(derived).hash";

/// `hash(self)` for an integer or `Bool`: the hash of its 64-bit value
/// (`oska_check::hashing`).
fn word_hash(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(hashed(hashing::word(word_of(&args[0]))))
}

/// `String.hash(self)`: the FNV-1a hash of its UTF-8 bytes.
fn text_hash(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(hashed(hashing::bytes(args[0].as_str().as_bytes())))
}

/// `Path.hash(self)`: the hash of its text.
fn path_hash(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(hashed(hashing::bytes(args[0].field(0).as_str().as_bytes())))
}

/// `hash(self)` for a `List`, an `Option`, and a struct or enum that
/// derives `Hash`: the state of the variant's index (a list's length),
/// then each field's or item's hash through `Hash` for its own type, which
/// may be the program's impl.
fn items_hash(
    interp: &mut Interpreter<'_, '_>,
    args: &mut Args,
    span: Span,
) -> Result<Value, Panic> {
    let (mut state, items): (u64, Rc<Vec<Value>>) = match &args[0] {
        Value::List(items) => (hashing::word(items.len() as u64), Rc::clone(items)),
        Value::Struct { fields, .. } => (hashing::word(0), Rc::clone(fields)),
        Value::Variant {
            ty,
            variant,
            fields,
        } => (
            hashing::word(interp.variant_index(ty, variant) as u64),
            Rc::clone(fields),
        ),
        other => unreachable!("the checker guarantees a list, struct or enum, found {other:?}"),
    };
    for item in items.iter() {
        let hash = interp
            .call_trait(builtins::HASH_HASH, item.clone(), Vec::new(), span)?
            .as_int();
        state = hashing::mix(state, hash as u64);
    }
    Ok(hashed(state))
}

/// The 64-bit value of an integer or `Bool`.
fn word_of(value: &Value) -> u64 {
    match value {
        Value::Bool(b) => u64::from(*b),
        other => other.as_int() as u64,
    }
}

/// A hash, as the `Int` it is.
fn hashed(hash: u64) -> Value {
    Value::int(i128::from(hash as i64))
}

/// `eq(self, other)` for a type whose values are equal exactly when
/// they hold the same data: integers, `Bool`, text, `Path`, `IoError`.
fn same(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0] == args[1]))
}

/// `cmp(self, other)` for the integers, `Bool` and `String` (ADR 0034): an
/// `Ordering`.
fn cmp(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(ordering(crate::ops::order(&args[0], &args[1])))
}

/// Wrap an order as the prelude's `Ordering`.
fn ordering(order: std::cmp::Ordering) -> Value {
    let variant = match order {
        std::cmp::Ordering::Less => "Less",
        std::cmp::Ordering::Equal => "Equal",
        std::cmp::Ordering::Greater => "Greater",
    };
    Value::variant(builtins::ORDERING, variant, Vec::new())
}

/// `cmp(self, other)` for `Option` and `List` (ADR 0034): `None` before
/// `Some`; lists item by item through `Ord` for the item's type, a prefix
/// before a longer list.
fn items_cmp(
    interp: &mut Interpreter<'_, '_>,
    args: &mut Args,
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
            // `None` is declared before `Some`.
            if lv != rv {
                return Ok(ordering(if &**lv == "None" {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }));
            }
            (lhs, rhs)
        }
        other => unreachable!("the checker guarantees two lists or two variants, found {other:?}"),
    };
    for (l, r) in lhs.iter().zip(rhs.iter()) {
        let order = interp.call_trait(builtins::ORD_CMP, l.clone(), vec![r.clone()], span)?;
        if !matches!(&order, Value::Variant { variant, .. } if &**variant == "Equal") {
            return Ok(order);
        }
    }
    Ok(ordering(lhs.len().cmp(&rhs.len())))
}

/// `IoError.to_string(self)`: its message.
fn io_error_to_string(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args[0].field(0))
}

/// `eq(self, other)` for `Option`, `Result`, and `List`: the same variant
/// or length, with each item equal to the other's through `Eq` for its
/// type.
fn items_eq(interp: &mut Interpreter<'_, '_>, args: &mut Args, span: Span) -> Result<Value, Panic> {
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
            .call_trait(builtins::EQ_EQ, l.clone(), vec![r.clone()], span)?
            .as_bool()
        {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
