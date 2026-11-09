//! Intrinsics for data: the body-less methods of `List` and `String`.
//!
//! They are listed apart from the capability intrinsics in
//! `intrinsics.rs`, which looks in both tables. A list is a
//! reference-counted vector (ADR 0001): reading one shares it.

use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every data intrinsic, by qualified name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    ("List.at", list_at),
    ("List.count", list_len),
    ("List.get", list_get),
    ("List.len", list_len),
];

/// `List.len(self)` and `List.count(self)`: the number of items.
fn list_len(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Int(args[0].as_list().len() as i128))
}

/// `List.get(self, index)`: the item at `index`, or `None` when there is
/// none.
fn list_get(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let item = usize::try_from(args[1].as_int())
        .ok()
        .and_then(|i| args[0].as_list().get(i));
    Ok(match item {
        Some(item) => Value::variant("Option", "Some", vec![item.clone()]),
        None => Value::variant("Option", "None", Vec::new()),
    })
}

/// `List.at(self, index)`: the item at `index`. Out of bounds, it panics
/// with `P0002`.
fn list_at(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let items = args[0].as_list();
    let index = args[1].as_int();
    usize::try_from(index)
        .ok()
        .and_then(|i| items.get(i))
        .cloned()
        .ok_or_else(|| {
            Panic::new(
                codes::INDEX_OUT_OF_BOUNDS,
                format!(
                    "index {index} is out of bounds for a list of length {}",
                    items.len()
                ),
                span,
            )
        })
}
