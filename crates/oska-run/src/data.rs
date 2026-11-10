//! Intrinsics for data: the body-less methods of `List` and `String`.
//!
//! They are listed apart from the capability intrinsics in
//! `intrinsics.rs`, whose `lookup` searches every module's table. A list is a
//! reference-counted vector (ADR 0001): reading one shares it.

use std::rc::Rc;

use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Interpreter;
use crate::intrinsics::Intrinsic::{HigherOrder, Mutating, Plain};
use crate::intrinsics::Table;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every data intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("List.at", Plain(list_at)),
    ("List.count", Plain(list_len)),
    ("List.get", Plain(list_get)),
    ("List.len", Plain(list_len)),
    ("List.new", Plain(list_new)),
    ("String.byte_len", Plain(string_byte_len)),
    ("String.chars", Plain(string_chars)),
    ("String.contains", Plain(string_contains)),
    ("String.ends_with", Plain(string_ends_with)),
    ("String.join", Plain(string_join)),
    ("String.len", Plain(string_len)),
    ("String.lines", Plain(string_lines)),
    ("String.parse_int", Plain(string_parse_int)),
    ("String.repeat", Plain(string_repeat)),
    ("String.replace", Plain(string_replace)),
    ("String.slice", Plain(string_slice)),
    ("String.split", Plain(string_split)),
    ("String.starts_with", Plain(string_starts_with)),
    ("String.trim", Plain(string_trim)),
    ("List.map", HigherOrder(list_map)),
    ("List.push", Mutating(list_push)),
];

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

/// `String.starts_with(self, prefix)`.
fn string_starts_with(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].as_str().starts_with(args[1].as_str())))
}

/// `String.ends_with(self, suffix)`.
fn string_ends_with(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(Value::Bool(args[0].as_str().ends_with(args[1].as_str())))
}

/// `String.replace(self, from, to)`: every occurrence of `from`, left to
/// right and without overlap, replaced by `to`.
fn string_replace(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let (text, from, to) = (args[0].as_str(), args[1].as_str(), args[2].as_str());
    // An empty `from` matches between every two characters; replacing it
    // would only inflate the text, so it replaces nothing.
    if from.is_empty() {
        return Ok(args[0].clone());
    }
    Ok(Value::str(&text.replace(from, to)))
}

/// `String.join(self, parts)`: `parts` with `self` between each two.
fn string_join(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let parts: Vec<&str> = args[1].as_list().iter().map(Value::as_str).collect();
    Ok(Value::str(&parts.join(args[0].as_str())))
}

/// `String.chars(self)`: each character as a text of its own.
fn string_chars(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let chars = args[0]
        .as_str()
        .chars()
        .map(|c| Value::str(c.encode_utf8(&mut [0; 4])))
        .collect();
    Ok(Value::List(Rc::new(chars)))
}

/// `String.slice(self, start, end)`: the characters from `start` up to,
/// but excluding, `end`. Indices out of bounds, or `start` past `end`,
/// panic with `P0035`.
fn string_slice(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let text = args[0].as_str();
    let (start, end) = (args[1].as_int(), args[2].as_int());
    let length = text.chars().count();
    let bounds = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .filter(|&(start, end)| start <= end && end <= length);
    let Some((start, end)) = bounds else {
        return Err(Panic::new(
            codes::SLICE_OUT_OF_BOUNDS,
            format!("cannot slice characters {start}..{end} of a text of {length} characters"),
            span,
        ));
    };
    // The byte offset of character `n`, or the end for the one past the
    // last.
    let offset = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
    Ok(Value::str(&text[offset(start)..offset(end)]))
}

/// The longest text `String.repeat` builds, in bytes.
const MAX_REPEATED_BYTES: usize = 256 * 1024 * 1024;

/// `String.repeat(self, times)`: the text `times` times over. A negative
/// count panics with `P0036`, a result over [`MAX_REPEATED_BYTES`] with
/// `P0037`.
fn string_repeat(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    let text = args[0].as_str();
    let times = args[1].as_int();
    if times < 0 {
        return Err(Panic::new(
            codes::NEGATIVE_REPEAT,
            format!("cannot repeat a text {times} times"),
            span,
        ));
    }
    let fits = usize::try_from(times)
        .ok()
        .and_then(|n| n.checked_mul(text.len()))
        .is_some_and(|bytes| bytes <= MAX_REPEATED_BYTES);
    // An empty text repeats to an empty text however often.
    if text.is_empty() {
        return Ok(args[0].clone());
    }
    if !fits {
        return Err(Panic::new(
            codes::TEXT_TOO_LONG,
            format!(
                "repeating a text of {} bytes {times} times is too long",
                text.len()
            ),
            span,
        ));
    }
    Ok(Value::str(&text.repeat(times as usize)))
}
