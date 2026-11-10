//! Intrinsics for data: the body-less methods of `List` and `String`.
//!
//! They are listed apart from the capability intrinsics in
//! `intrinsics.rs`, which looks in both tables. A list is a
//! reference-counted vector (ADR 0001): reading one shares it.

use std::rc::Rc;

use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Interpreter;
use crate::intrinsics::{HigherOrder, Intrinsic, MutatingIntrinsic};
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every data intrinsic, by qualified name.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    ("List.at", list_at),
    ("List.count", list_len),
    ("List.get", list_get),
    ("List.len", list_len),
    ("List.new", list_new),
    ("String.byte_len", string_byte_len),
    ("String.contains", string_contains),
    ("String.len", string_len),
    ("String.lines", string_lines),
    ("String.parse_int", string_parse_int),
    ("String.split", string_split),
    ("String.trim", string_trim),
];

/// Every higher-order data intrinsic, by qualified name.
pub const HIGHER_ORDER: &[(&str, HigherOrder)] = &[("List.map", list_map)];

/// Every mutating data intrinsic, by qualified name.
pub const MUTATING: &[(&str, MutatingIntrinsic)] = &[("List.push", list_push)];

/// `List.new()`: an empty list.
fn list_new(_: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::List(Rc::new(Vec::new())))
}

/// `List.push(var self, item)`: `item` added at the end. A list that is
/// shared is copied first.
fn list_push(list: &mut Value, mut args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let Value::List(items) = list else {
        unreachable!("the checker guarantees a list, found {list:?}");
    };
    Rc::make_mut(items).push(args.swap_remove(0));
    Ok(Value::Unit)
}

/// `List.len(self)` and `List.count(self)`: the number of items.
fn list_len(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(args[0].as_list().len() as i128))
}

/// `List.get(self, index)`: the item at `index`, or `None` when there is
/// none.
fn list_get(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let item = usize::try_from(args[1].as_int())
        .ok()
        .and_then(|i| args[0].as_list().get(i));
    Ok(Value::option(item.cloned()))
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

/// `List.map(self, f)`: `f` called on each item, in order.
fn list_map(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let mapped = args[0]
        .as_list()
        .iter()
        .map(|item| interp.call_value(&args[1], vec![item.clone()], span))
        .collect::<Result<Vec<Value>, Panic>>()?;
    Ok(Value::List(Rc::new(mapped)))
}

/// `String.len(self)`: the number of characters (Unicode code points).
fn string_len(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(args[0].as_str().chars().count() as i128))
}

/// `String.byte_len(self)`: the number of bytes in its UTF-8 encoding.
fn string_byte_len(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(args[0].as_str().len() as i128))
}

/// `String.parse_int(self)`: the decimal `Int` the whole text spells, or
/// `None` when it spells none, or one out of `Int`'s range.
fn string_parse_int(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let parsed = args[0].as_str().parse::<i64>().ok();
    Ok(Value::option(parsed.map(|n| Value::int(i128::from(n)))))
}

/// `String.trim(self)`: without leading and trailing whitespace.
fn string_trim(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::str(args[0].as_str().trim()))
}

/// `String.split(self, separator)`: the pieces between occurrences of
/// `separator`, empty ones included.
fn string_split(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let pieces = args[0]
        .as_str()
        .split(args[1].as_str())
        .map(Value::str)
        .collect();
    Ok(Value::List(Rc::new(pieces)))
}

/// `String.contains(self, needle)`: whether `needle` occurs in the text.
fn string_contains(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].as_str().contains(args[1].as_str())))
}

/// `String.lines(self)`: the lines, without their line endings.
fn string_lines(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let lines = args[0].as_str().lines().map(Value::str).collect();
    Ok(Value::List(Rc::new(lines)))
}
