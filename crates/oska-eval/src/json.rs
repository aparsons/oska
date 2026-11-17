//! A JSON reader and writer. The harness reads task files and `oskac`'s
//! JSON output, and writes its reports, with no crate from outside the
//! repo. Reading never panics: malformed text is an error naming the line
//! and column.
//!
//! When the toolchain's shared JSON reader lands (#194), the harness should
//! read with it instead.

use std::fmt::Write as _;

/// A JSON value. Numbers keep their text, so nothing is lost or rounded;
/// an object keeps its keys in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Parses `text`, which must hold exactly one JSON value. The error is
    /// `line:column: message`.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            text,
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = reader.value(0)?;
        reader.skip_ws();
        if reader.pos < reader.bytes.len() {
            return Err(reader.error("expected the end of the file after the value"));
        }
        Ok(value)
    }

    /// The value of `key`, if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// A number that is a non-negative integer that fits in a `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// The JSON text, indented by two spaces, with a final line break.
    /// Empty containers, and short arrays and objects whose values are all
    /// scalars, stay on one line.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        write_value(self, 0, &mut out);
        out.push('\n');
        out
    }
}

/// How deeply arrays and objects may nest before the reader refuses.
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn error(&self, message: &str) -> String {
        let before = &self.text[..self.pos.min(self.text.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        format!("{line}:{column}: {message}")
    }

    fn found(&self) -> String {
        match self.text[self.pos..].chars().next() {
            None => "the end of the file".to_string(),
            Some(c) => format!("`{}`", c.escape_default()),
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, byte: u8, what: &str) -> Result<(), String> {
        self.skip_ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected {what}, found {}", self.found())))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        self.skip_ws();
        if depth > MAX_DEPTH {
            return Err(self.error("arrays and objects nest too deeply"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error(&format!("expected a value, found {}", self.found()))),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error(&format!("expected a value, found {}", self.found())))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1;
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.error(&format!("expected a key, found {}", self.found())));
            }
            let start = self.pos;
            let key = self.string()?;
            if fields.iter().any(|(k, _)| *k == key) {
                self.pos = start;
                return Err(self.error(&format!("the key `{key}` appears twice")));
            }
            self.eat(b':', "`:` after the key")?;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => {
                    return Err(self.error(&format!("expected `,` or `}}`, found {}", self.found())))
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => {
                    return Err(self.error(&format!("expected `,` or `]`, found {}", self.found())))
                }
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits = |r: &mut Self| {
            let from = r.pos;
            while let Some(b'0'..=b'9') = r.peek() {
                r.pos += 1;
            }
            r.pos > from
        };
        if !digits(self) {
            return Err(self.error(&format!("expected a digit, found {}", self.found())));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(self.error(&format!("expected a digit, found {}", self.found())));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(self.error(&format!("expected a digit, found {}", self.found())));
            }
        }
        Ok(Json::Num(self.text[start..self.pos].to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.pos..];
            let Some(c) = rest.chars().next() else {
                return Err(self.error("the string does not end"));
            };
            match c {
                '"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                '\\' => {
                    self.pos += 1;
                    out.push(self.escape()?);
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.error("a control character in a string must be escaped"))
                }
                c => {
                    out.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self) -> Result<char, String> {
        let Some(b) = self.peek() else {
            return Err(self.error("the string does not end"));
        };
        self.pos += 1;
        Ok(match b {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let high = self.hex4()?;
                if (0xD800..0xDC00).contains(&high) {
                    if !self.text[self.pos..].starts_with("\\u") {
                        return Err(self.error("a high surrogate must be followed by a low one"));
                    }
                    self.pos += 2;
                    let low = self.hex4()?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return Err(self.error("a high surrogate must be followed by a low one"));
                    }
                    let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    char::from_u32(code).ok_or_else(|| self.error("not a character"))?
                } else {
                    char::from_u32(high).ok_or_else(|| self.error("a lone low surrogate"))?
                }
            }
            _ => {
                self.pos -= 1;
                return Err(self.error(&format!(
                    "unknown escape `\\{}`",
                    self.found().trim_matches('`')
                )));
            }
        })
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.text.get(self.pos..self.pos + 4).unwrap_or("");
        match u32::from_str_radix(digits, 16) {
            Ok(n) if digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
                self.pos += 4;
                Ok(n)
            }
            _ => Err(self.error("expected four hex digits after `\\u`")),
        }
    }
}

/// The widest array of scalars written on one line.
const INLINE_ARRAY: usize = 40;
/// The widest object of scalars written on one line.
const INLINE_OBJECT: usize = 100;

fn is_scalar(value: &Json) -> bool {
    !matches!(value, Json::Arr(_) | Json::Obj(_))
}

fn inline_array(items: &[Json]) -> String {
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

fn inline_object(fields: &[(String, Json)]) -> String {
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

fn write_value(value: &Json, indent: usize, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(n),
        Json::Str(s) => write_string(s, out),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Arr(items)
            if items.iter().all(is_scalar) && inline_array(items).len() <= INLINE_ARRAY =>
        {
            out.push_str(&inline_array(items));
        }
        Json::Arr(items) => {
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
        Json::Obj(fields) if fields.is_empty() => out.push_str("{}"),
        Json::Obj(fields)
            if fields.iter().all(|(_, v)| is_scalar(v))
                && inline_object(fields).len() <= INLINE_OBJECT =>
        {
            out.push_str(&inline_object(fields));
        }
        Json::Obj(fields) => {
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
