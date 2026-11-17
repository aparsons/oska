//! The crate's JSON: one writer for every JSON string `oskac` prints
//! ([`quote`], [`quote_option`]), and a strict reader for the install
//! receipt. The crate has no dependencies, so it brings its own. The
//! reader fails on anything it cannot read for certain: malformed JSON, a
//! repeated key in an object, nesting past [`MAX_DEPTH`], or text after
//! the value. It keeps every field it finds, so a receipt with fields this
//! `oskac` does not know (a newer installer's) reads and prints unchanged.

const MAX_DEPTH: usize = 32;

/// A JSON value. A number keeps its text, so it prints as it was written.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = reader.value(0)?;
        reader.skip_ws();
        if reader.pos != reader.bytes.len() {
            return Err("text after the JSON value".to_string());
        }
        Ok(value)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A number's value, however it is spelled: `1`, `1.0` and `1e0` are
    /// all 1.
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    /// Append the value as compact JSON text.
    pub(crate) fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(n),
            Json::Str(s) => write_str(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// `text` as a JSON string literal.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::new();
    write_str(text, &mut out);
    out
}

/// `text` as a JSON string literal, or `null`.
pub(crate) fn quote_option(text: Option<&str>) -> String {
    text.map_or_else(|| "null".to_string(), quote)
}

fn write_str(text: &str, out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.pos))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("malformed JSON at byte {}", self.pos))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("JSON nested too deeply".to_string());
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("malformed JSON at byte {}", self.pos)),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "malformed number".to_string())?;
        // JSON forbids a leading zero before more digits and a trailing
        // dot; parsing as f64 catches the rest.
        let digits = text.strip_prefix('-').unwrap_or(text);
        let leading_zero = digits.len() > 1
            && digits.starts_with('0')
            && !digits.starts_with("0.")
            && !digits.starts_with("0e")
            && !digits.starts_with("0E");
        if leading_zero || text.ends_with('.') || text.parse::<f64>().is_err() {
            return Err(format!("malformed number at byte {start}"));
        }
        Ok(Json::Num(text.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while !matches!(self.peek(), Some(b'"' | b'\\') | None) {
                if self.bytes[self.pos] < 0x20 {
                    return Err("control character in a string".to_string());
                }
                self.pos += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "invalid UTF-8 in a string".to_string())?,
            );
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escape = self.peek().ok_or("unterminated string")?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err("unknown escape in a string".to_string()),
                    }
                }
                _ => return Err("unterminated string".to_string()),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .and_then(|b| std::str::from_utf8(b).ok())
            .filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("malformed \\u escape")?;
        let value = u32::from_str_radix(digits, 16).map_err(|_| "malformed \\u escape")?;
        self.pos += 4;
        Ok(value)
    }

    fn unicode(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if !self.bytes[self.pos..].starts_with(b"\\u") {
                return Err("unpaired surrogate escape".to_string());
            }
            self.pos += 2;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err("unpaired surrogate escape".to_string());
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        char::from_u32(code).ok_or_else(|| "unpaired surrogate escape".to_string())
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.eat(b'[')?;
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
                _ => return Err(format!("malformed array at byte {}", self.pos)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            if fields.iter().any(|(k, _)| *k == key) {
                return Err(format!("repeated key \"{key}\""));
            }
            self.skip_ws();
            self.eat(b':')?;
            fields.push((key, self.value(depth + 1)?));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(format!("malformed object at byte {}", self.pos)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        let mut out = String::new();
        Json::parse(text).expect("should parse").write(&mut out);
        out
    }

    #[test]
    fn values_round_trip_with_unknown_fields_kept() {
        assert_eq!(
            round_trip(r#" {"a": [1, -2.5e3, true, null], "z": {"k": "v\n\u00e9\ud83d\ude00"}} "#),
            "{\"a\":[1,-2.5e3,true,null],\"z\":{\"k\":\"v\\né😀\"}}"
        );
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_control_characters() {
        assert_eq!(quote("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
        assert_eq!(quote_option(None), "null");
    }

    #[test]
    fn a_number_has_one_value_however_it_is_spelled() {
        for one in ["1", "1.0", "1e0", "10e-1"] {
            assert_eq!(Json::parse(one).ok().and_then(|n| n.as_f64()), Some(1.0));
        }
    }

    #[test]
    fn malformed_json_is_refused() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,\"a\":2}",
            "01",
            "1.",
            "\"a",
            "{} x",
            "\"\\ud800\"",
            "\"a\u{1}\"",
            "nul",
            "{\"a\" 1}",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(Json::parse(&"[".repeat(100)).is_err());
    }
}
