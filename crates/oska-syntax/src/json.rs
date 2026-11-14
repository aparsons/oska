//! A JSON reader, the one the toolchain shares: `oskac hook claude` reads
//! its input with it, so there is no regular expression over JSON and no
//! outside crate. It follows RFC 8259 and is strict: it refuses what is
//! not JSON, such as a trailing comma or a raw control character in a
//! string, and every escape comes out decoded.

/// How deep arrays and objects may nest. A deeper text is refused rather
/// than allowed to exhaust the stack.
const MAX_DEPTH: usize = 128;

/// A JSON value. Object members keep the order they were written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    /// A number, as written: this reader does not need its value.
    Number(String),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// The value of member `key` of an object; the first, if it is given
    /// more than once. `None` for a missing key and for any other value.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The text of a string value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// `text` as JSON, or why it is not.
pub fn parse_json(text: &str) -> Result<JsonValue, String> {
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
    };
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.at == text.len() {
        Ok(value)
    } else {
        Err(format!("trailing text at byte {}", reader.at))
    }
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", byte as char, self.at))
        }
    }

    fn word(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, String> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("unexpected input at byte {}", self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue, String> {
        self.skip_space();
        match self.peek() {
            Some(b'{' | b'[') if depth >= MAX_DEPTH => Err(format!(
                "nesting deeper than {MAX_DEPTH} at byte {}",
                self.at
            )),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(JsonValue::Str(self.string()?)),
            Some(b't') => self.word("true", JsonValue::Bool(true)),
            Some(b'f') => self.word("false", JsonValue::Bool(false)),
            Some(b'n') => self.word("null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(format!("unexpected input at byte {}", self.at)),
            None => Err("unexpected end of text".to_string()),
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonValue, String> {
        self.at += 1;
        let mut members = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.eat(b':')?;
            members.push((key, self.value(depth + 1)?));
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.at)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsonValue, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.at)),
            }
        }
    }

    /// A number as RFC 8259 writes it: `-`, an integer part with no
    /// leading zero, then an optional fraction and exponent.
    fn number(&mut self) -> Result<JsonValue, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(format!("bad number at byte {start}")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.some_digits(start)?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.some_digits(start)?;
        }
        // The text is ASCII digits and signs, so nothing is lost.
        Ok(JsonValue::Number(
            String::from_utf8_lossy(&self.text[start..self.at]).into_owned(),
        ))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
    }

    fn some_digits(&mut self, number_start: usize) -> Result<(), String> {
        let before = self.at;
        self.digits();
        if self.at == before {
            Err(format!("bad number at byte {number_start}"))
        } else {
            Ok(())
        }
    }

    /// Four hex digits after `\u`.
    fn hex4(&mut self) -> Result<u32, String> {
        let value = self
            .text
            .get(self.at..self.at + 4)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or_else(|| format!("bad \\u escape at byte {}", self.at))?;
        self.at += 4;
        Ok(value)
    }

    /// The character a `\u` escape names, with the pair of escapes a
    /// character past U+FFFF is written as.
    fn unicode_escape(&mut self) -> Result<char, String> {
        let at = self.at;
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.text[self.at..].starts_with(b"\\u") {
                    return Err(format!("a lone surrogate escape at byte {at}"));
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(format!("a lone surrogate escape at byte {at}"));
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(format!("a lone surrogate escape at byte {at}")),
            other => other,
        };
        char::from_u32(code).ok_or_else(|| format!("bad \\u escape at byte {at}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "a string that never ends".to_string())?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                0..=0x1f => return Err(format!("raw control byte {byte:#x} in a string")),
                b'\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| "a dangling backslash".to_string())?;
                    self.at += 1;
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
                        other => return Err(format!("bad escape `\\{}`", other as char)),
                    };
                    let mut buffer = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                other => out.push(other),
            }
        }
    }
}
