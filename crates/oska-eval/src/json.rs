//! The harness's JSON report writer. Reading uses the toolchain's shared
//! reader, `oska_syntax::parse_json`; this module only prints a
//! [`JsonValue`] indented, for the reports and their golden files.

use oska_syntax::JsonValue;
use std::fmt::Write as _;
use std::process::ExitCode;

pub fn field(key: &str, value: JsonValue) -> (String, JsonValue) {
    (key.to_string(), value)
}

pub fn strings(items: &[String]) -> JsonValue {
    JsonValue::Array(items.iter().map(|s| JsonValue::Str(s.clone())).collect())
}

/// A `parse_json` error for `text` with its `at byte N` said as `at line L,
/// column C` (both from 1, the column in characters), where a person looks.
/// Any other message is returned as it is.
pub fn locate(text: &str, message: &str) -> String {
    const MARK: &str = " at byte ";
    let Some(start) = message.rfind(MARK) else {
        return message.to_string();
    };
    let digits = &message[start + MARK.len()..];
    let Ok(offset) = digits.parse::<usize>() else {
        return message.to_string();
    };
    let before = text.get(..offset.min(text.len())).unwrap_or("");
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    format!("{} at line {line}, column {column}", &message[..start])
}

/// Bad input is reported the same way by every subcommand: a report on
/// standard output holding `errors`, and exit code 2. Nothing goes to
/// standard error.
pub fn bad_input(errors: &[String]) -> ExitCode {
    let report = JsonValue::Object(vec![
        field("schema", JsonValue::Number("1".to_string())),
        field("valid", JsonValue::Bool(false)),
        field("errors", strings(errors)),
    ]);
    print!("{}", pretty(&report));
    ExitCode::from(2)
}

/// The JSON text, indented by two spaces, with a final line break. Empty
/// containers, and short arrays and objects whose values are all scalars,
/// stay on one line.
pub fn pretty(value: &JsonValue) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out.push('\n');
    out
}

/// The widest array of scalars written on one line.
const INLINE_ARRAY: usize = 40;
/// The widest object of scalars written on one line.
const INLINE_OBJECT: usize = 100;

fn is_scalar(value: &JsonValue) -> bool {
    !matches!(value, JsonValue::Array(_) | JsonValue::Object(_))
}

fn inline_array(items: &[JsonValue]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_value(item, 0, &mut out);
    }
    out.push(']');
    out
}

fn inline_object(fields: &[(String, JsonValue)]) -> String {
    let mut out = String::from("{");
    for (i, (key, item)) in fields.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_string(key, &mut out);
        out.push_str(": ");
        write_value(item, 0, &mut out);
    }
    out.push('}');
    out
}

fn write_value(value: &JsonValue, indent: usize, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Number(n) => out.push_str(n),
        JsonValue::Str(s) => write_string(s, out),
        JsonValue::Array(items) if items.is_empty() => out.push_str("[]"),
        JsonValue::Array(items)
            if items.iter().all(is_scalar) && inline_array(items).len() <= INLINE_ARRAY =>
        {
            out.push_str(&inline_array(items));
        }
        JsonValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i > 0 { ",\n" } else { "\n" });
                pad(indent + 1, out);
                write_value(item, indent + 1, out);
            }
            out.push('\n');
            pad(indent, out);
            out.push(']');
        }
        JsonValue::Object(fields) if fields.is_empty() => out.push_str("{}"),
        JsonValue::Object(fields)
            if fields.iter().all(|(_, v)| is_scalar(v))
                && inline_object(fields).len() <= INLINE_OBJECT =>
        {
            out.push_str(&inline_object(fields));
        }
        JsonValue::Object(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                out.push_str(if i > 0 { ",\n" } else { "\n" });
                pad(indent + 1, out);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, indent + 1, out);
            }
            out.push('\n');
            pad(indent, out);
            out.push('}');
        }
    }
}

fn pad(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::locate;

    #[test]
    fn a_byte_offset_becomes_a_line_and_column() {
        let text = "{\n  \"a\": 1,\n  oops\n}";
        let offset = text.find("oops").expect("in the text");
        let message = format!("expected `\"` at byte {offset}");
        assert_eq!(locate(text, &message), "expected `\"` at line 3, column 3");
    }

    #[test]
    fn the_column_counts_characters_not_bytes() {
        let text = "\"é\" x";
        let offset = text.find('x').expect("in the text");
        assert_eq!(
            locate(text, &format!("trailing text at byte {offset}")),
            "trailing text at line 1, column 5"
        );
    }

    #[test]
    fn another_message_is_left_alone() {
        assert_eq!(locate("{}", "unexpected end"), "unexpected end");
        assert_eq!(locate("{}", "a thing at byte x"), "a thing at byte x");
    }
}
