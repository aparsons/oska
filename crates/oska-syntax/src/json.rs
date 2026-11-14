//! A JSON reader for the toolchain's own inputs, such as an authority
//! baseline. The workspace takes no crate from outside the repository, so
//! it brings its own.
//!
//! [`Json::parse`] reads one JSON value (RFC 8259) and refuses anything
//! else with a message: trailing text, a duplicate key, a lone surrogate,
//! or nesting deeper than [`MAX_DEPTH`], so a hostile file cannot exhaust
//! the stack. Numbers keep their text; [`Json::as_u64`] reads a count.

/// How deeply arrays and objects may nest.
pub const MAX_DEPTH: usize = 128;

/// A JSON value. An object keeps its fields in the order written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A number, as written.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Read `text` as one JSON value, with only whitespace around it.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = reader.value(0)?;
        reader.skip_ws();
        if reader.pos != text.len() {
            return Err(reader.error("trailing text after the JSON value"));
        }
        Ok(value)
    }

    /// The value of `key`, for an object that has it.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// A number written as a non-negative integer that fits in a `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(text) if text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok(),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.pos)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Consume `byte`, after any whitespace.
    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.skip_ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected `{}`", byte as char)))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth >= MAX_DEPTH {
            return Err(self.error("JSON nested too deeply"));
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("expected a JSON value")),
            None => Err(self.error("unexpected end of JSON")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("expected a JSON value"))
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
                return Err(self.error("expected a string key"));
            }
            let key = self.string()?;
            if fields.iter().any(|(k, _)| *k == key) {
                return Err(self.error(&format!("duplicate key `{key}`")));
            }
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(self.error("expected `,` or `}`")),
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
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("malformed number")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("malformed number"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("malformed number"));
            }
            self.digits();
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
        Ok(Json::Num(text))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(self.error("unterminated string"));
                    };
                    self.pos += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return Err(self.error("unknown escape in string")),
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                0x00..=0x1f => return Err(self.error("control character in string")),
                _ => out.push(byte),
            }
        }
        String::from_utf8(out).map_err(|_| self.error("string is not UTF-8"))
    }

    /// The character of a `\u` escape, whose `\u` has been read, joining
    /// a surrogate pair.
    fn unicode_escape(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&high) {
            if !self.bytes[self.pos..].starts_with(b"\\u") {
                return Err(self.error("lone surrogate in string"));
            }
            self.pos += 2;
            let low = self.hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.error("lone surrogate in string"));
            }
            0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
        } else {
            high
        };
        char::from_u32(code).ok_or_else(|| self.error("lone surrogate in string"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("malformed \\u escape"))?;
        let value =
            u32::from_str_radix(digits, 16).map_err(|_| self.error("malformed \\u escape"))?;
        self.pos += 4;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn reads_every_kind_of_value() {
        let value =
            Json::parse(r#" {"a": [1, -2.5e3, true, false, null], "b": "x\"\u00e9\ud83d\ude00"} "#)
                .expect("valid JSON");
        assert_eq!(
            value.get("a"),
            Some(&Json::Arr(vec![
                Json::Num("1".into()),
                Json::Num("-2.5e3".into()),
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
            ]))
        );
        assert_eq!(value.get("b").and_then(Json::as_str), Some("x\"é😀"));
    }

    #[test]
    fn refuses_what_is_not_one_json_value() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "{\"a\":1} x",
            "{\"a\":1,\"a\":2}",
            "\"\\ud800\"",
            "\"\u{1}\"",
            "01",
            "-",
            "tru",
            "{a:1}",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn refuses_deep_nesting_without_overflowing_the_stack() {
        let deep = "[".repeat(100_000);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn counts_are_non_negative_integers() {
        assert_eq!(Json::parse("12").ok().and_then(|j| j.as_u64()), Some(12));
        assert_eq!(Json::parse("-1").ok().and_then(|j| j.as_u64()), None);
        assert_eq!(Json::parse("1.0").ok().and_then(|j| j.as_u64()), None);
    }
}
