//! Helpers shared by the `oskac` integration tests.

#![allow(dead_code)]

/// A minimal JSON reader, enough to walk the export. The crate has no
/// dependencies, so the test brings its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(u64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Json {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = reader.value();
        reader.skip_ws();
        assert_eq!(reader.pos, text.len(), "trailing text after the JSON value");
        value
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn next(&mut self) -> u8 {
        let b = *self.bytes.get(self.pos).expect("unexpected end of JSON");
        self.pos += 1;
        b
    }

    fn expect(&mut self, word: &str) {
        for b in word.bytes() {
            assert_eq!(self.next(), b, "malformed JSON literal");
        }
    }

    fn value(&mut self) -> Json {
        self.skip_ws();
        match self.bytes.get(self.pos).copied().expect("expected a value") {
            b'{' => {
                self.pos += 1;
                let mut fields = Vec::new();
                loop {
                    self.skip_ws();
                    if self.bytes[self.pos] == b'}' {
                        self.pos += 1;
                        return Json::Obj(fields);
                    }
                    if !fields.is_empty() {
                        assert_eq!(self.next(), b',', "expected `,` between fields");
                        self.skip_ws();
                    }
                    let key = self.string();
                    self.skip_ws();
                    assert_eq!(self.next(), b':', "expected `:` after a key");
                    let value = self.value();
                    assert!(
                        !fields.iter().any(|(k, _)| *k == key),
                        "duplicate key `{key}`"
                    );
                    fields.push((key, value));
                }
            }
            b'[' => {
                self.pos += 1;
                let mut elements = Vec::new();
                loop {
                    self.skip_ws();
                    if self.bytes[self.pos] == b']' {
                        self.pos += 1;
                        return Json::Arr(elements);
                    }
                    if !elements.is_empty() {
                        assert_eq!(self.next(), b',', "expected `,` between elements");
                    }
                    elements.push(self.value());
                }
            }
            b'"' => Json::Str(self.string()),
            b't' => {
                self.expect("true");
                Json::Bool(true)
            }
            b'f' => {
                self.expect("false");
                Json::Bool(false)
            }
            b'n' => {
                self.expect("null");
                Json::Null
            }
            _ => {
                let start = self.pos;
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                let digits = std::str::from_utf8(&self.bytes[start..self.pos]).expect("ASCII");
                Json::Num(digits.parse().expect("expected a non-negative integer"))
            }
        }
    }

    fn string(&mut self) -> String {
        assert_eq!(self.next(), b'"', "expected a string");
        let mut out = Vec::new();
        loop {
            match self.next() {
                b'"' => return String::from_utf8(out).expect("strings should be UTF-8"),
                b'\\' => match self.next() {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'u' => {
                        let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
                            .expect("ASCII");
                        self.pos += 4;
                        let c = u32::from_str_radix(hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .expect("valid \\u escape");
                        out.extend_from_slice(c.to_string().as_bytes());
                    }
                    other => out.push(other),
                },
                b => out.push(b),
            }
        }
    }
}
