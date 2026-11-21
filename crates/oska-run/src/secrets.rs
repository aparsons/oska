//! Intrinsics for the methods of secret integers, `Secret<Bool>` and
//! `Secret<Bytes>` (#224).
//!
//! A secret is the prelude struct `Secret { value: T }`. Each method is
//! the public type's own intrinsic on the value inside, such as `U32.rotl`
//! for `Secret<U32>.rotl`, found from the value's runtime type tag, with
//! its result wrapped again; `ct_eq` compares the values, and `and`, `or`
//! and `ct_select` pick as [`crate::bytes::select`] does, after every
//! operand is evaluated. The interpreter makes no timing promise: the
//! constant-time guarantee is a property of native executables (ADR 0012).
//! `expose` and the comparisons are in `intrinsics.rs`.

use std::rc::Rc;

use oska_check::builtins;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::{self, Mutating, Plain};
use crate::intrinsics::{lookup, Table};
use crate::memory;
use crate::panics::Panic;
use crate::value::Value;

/// Every secret intrinsic here, by qualified name.
pub const INTRINSICS: Table = &[
    (
        "Secret.new",
        Plain(|_, mut args, span| seal(args.remove(0), span)),
    ),
    ("Secret.rotl", Plain(|h, a, s| lifted("rotl", h, a, s))),
    ("Secret.rotr", Plain(|h, a, s| lifted("rotr", h, a, s))),
    ("Secret.to_u32", Plain(|h, a, s| lifted("to_u32", h, a, s))),
    ("Secret.to_u64", Plain(|h, a, s| lifted("to_u64", h, a, s))),
    (
        "Secret.wrapping_to_u8",
        Plain(|h, a, s| lifted("wrapping_to_u8", h, a, s)),
    ),
    (
        "Secret.wrapping_to_u32",
        Plain(|h, a, s| lifted("wrapping_to_u32", h, a, s)),
    ),
    ("Secret.at", Plain(|h, a, s| lifted("at", h, a, s))),
    ("Secret.slice", Plain(|h, a, s| lifted("slice", h, a, s))),
    (
        "Secret.read_u32_be",
        Plain(|h, a, s| lifted("read_u32_be", h, a, s)),
    ),
    (
        "Secret.read_u32_le",
        Plain(|h, a, s| lifted("read_u32_le", h, a, s)),
    ),
    (
        "Secret.read_u64_be",
        Plain(|h, a, s| lifted("read_u64_be", h, a, s)),
    ),
    (
        "Secret.read_u64_le",
        Plain(|h, a, s| lifted("read_u64_le", h, a, s)),
    ),
    // A secret buffer's length is public.
    (
        "Secret.len",
        Plain(|_, a, _| Ok(Value::int(open(&a[0]).as_bytes().len() as i128))),
    ),
    (
        "Secret.ct_eq",
        Plain(|_, a, s| seal(Value::Bool(open(&a[0]) == open(&a[1])), s)),
    ),
    (
        "Secret.xor",
        Plain(|_, a, s| seal(Value::Bool(bit(&a[0]) != bit(&a[1])), s)),
    ),
    (
        "Secret.not",
        Plain(|_, a, s| seal(Value::Bool(!bit(&a[0])), s)),
    ),
    (
        "Secret.and",
        Plain(|_, a, s| pick(&a[0], open(&a[1]), Value::Bool(false), s)),
    ),
    (
        "Secret.or",
        Plain(|_, a, s| pick(&a[0], Value::Bool(true), open(&a[1]), s)),
    ),
    (
        "Secret.ct_select",
        Plain(|_, a, s| pick(&a[0], open(&a[1]), open(&a[2]), s)),
    ),
    ("Secret.push", Mutating(|r, a, s| mutated("push", r, a, s))),
    (
        "Secret.append",
        Mutating(|r, a, s| mutated("append", r, a, s)),
    ),
    (
        "Secret.push_u32_be",
        Mutating(|r, a, s| mutated("push_u32_be", r, a, s)),
    ),
    (
        "Secret.push_u32_le",
        Mutating(|r, a, s| mutated("push_u32_le", r, a, s)),
    ),
    (
        "Secret.push_u64_be",
        Mutating(|r, a, s| mutated("push_u64_be", r, a, s)),
    ),
    (
        "Secret.push_u64_le",
        Mutating(|r, a, s| mutated("push_u64_le", r, a, s)),
    ),
];

/// The value inside the secret `value`, or `value` itself when it is
/// public.
pub(crate) fn open(value: &Value) -> Value {
    match value {
        Value::Struct { ty, fields } if &**ty == builtins::SECRET => fields[0].clone(),
        other => other.clone(),
    }
}

/// `value`, made secret.
fn seal(value: Value, span: Span) -> Result<Value, Panic> {
    memory::wrap(builtins::SECRET, value, span)
}

/// The `Bool` inside the secret `value`.
fn bit(value: &Value) -> bool {
    open(value).as_bool()
}

/// The secret `if_true` when the secret `cond` holds, otherwise
/// `if_false`.
fn pick(cond: &Value, if_true: Value, if_false: Value, span: Span) -> Result<Value, Panic> {
    seal(
        crate::bytes::select(bit(cond), if_true, if_false, span)?,
        span,
    )
}

/// The name a value's type's intrinsics are qualified by.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Int(_, int) => int.name(),
        Value::Bytes(_) => "Bytes",
        other => unreachable!("the checker gives secrets of this type no methods: {other:?}"),
    }
}

/// The public intrinsic of the value's type that implements `method`.
fn public(method: &str, value: &Value) -> Intrinsic {
    let name = format!("{}.{method}", type_name(value));
    lookup(&name).unwrap_or_else(|| unreachable!("`{name}` is an intrinsic"))
}

/// The secret method `method` on `args`: the public type's method on the
/// values inside, made secret.
fn lifted(method: &str, host: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let args: Vec<Value> = args.iter().map(open).collect();
    let Plain(implementation) = public(method, &args[0]) else {
        unreachable!("`{method}` needs only the host")
    };
    seal(implementation(host, args, span)?, span)
}

/// The mutating method `method` of `Secret<Bytes>`, on the buffer inside
/// `receiver`.
fn mutated(
    method: &str,
    receiver: &mut Value,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let Value::Struct { fields, .. } = receiver else {
        unreachable!("the checker guarantees a secret receiver")
    };
    let inside = &mut Rc::make_mut(fields)[0];
    let Mutating(implementation) = public(method, inside) else {
        unreachable!("`{method}` is a mutating method")
    };
    implementation(inside, args.iter().map(open).collect(), span)
}
