//! `oska.toml`: the package manifest, read by our own parser for a strict
//! subset of TOML (ADR 0010).
//!
//! The interface is [`parse_manifest`]: the manifest's path and text in,
//! a [`Manifest`] or every diagnostic out. Each diagnostic names the
//! manifest's path, line and column. Nothing here reads the file system,
//! and nothing knows about modules, so milestone 5 can extend the table
//! of accepted keys and tables without touching the rest.
//!
//! The subset: `[table]` headers, `key = value` pairs with bare keys,
//! basic (`"..."`) and literal (`'...'`) single-line strings, arrays of
//! strings (which may span lines), and `#` comments. Anything else in
//! TOML is `E0401`, naming the feature. Milestone 1 accepts only
//! `[package]`, with a required `name` and `version`.
//!
//! - `E0400`: malformed text.
//! - `E0401`: valid TOML outside the subset.
//! - `E0402`: an unknown key or table, with a "did you mean" fix.
//! - `E0403`: `[dependencies]`, which arrives in milestone 5.
//! - `E0404`: a missing `[package]`, `name` or `version`.
//! - `E0405`: a value of the wrong type, a name that is not snake_case,
//!   or a version that is not semver.
//! - `E0406`: a repeated key or table.

use oska_syntax::{Diagnostic, Edit, LineIndex, Span};

use crate::{codes, suggest};

/// A valid manifest: the `[package]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The package's name: snake_case.
    pub name: String,
    /// The package's version: semver, as written.
    pub version: String,
}

/// Parse `text`, the manifest at `path`. A manifest with any error gives
/// every error found, in the order they are found, followed by what is
/// missing.
pub fn parse_manifest(path: &str, text: &str) -> Result<Manifest, Vec<Diagnostic>> {
    let malformed = text_errors(text);
    if !malformed.is_empty() {
        let lines = LineIndex::new(text);
        return Err(malformed
            .into_iter()
            .map(|mut d| {
                d.location = Some(Box::new(lines.location(path, d.span.start)));
                d
            })
            .collect());
    }
    let mut parser = Parser {
        path,
        text,
        pos: 0,
        errors: Vec::new(),
        table: Table::None,
        package: None,
        name: None,
        version: None,
        keys_seen: Vec::new(),
    };
    parser.run();
    parser.finish()
}

/// The manifest at `path` as text: `bytes` decoded as UTF-8, or an
/// `E0400` at the first byte that is not.
pub fn decode_manifest(path: &str, bytes: &[u8]) -> Result<String, Diagnostic> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(err) => {
            let valid = err.valid_up_to();
            let before = String::from_utf8_lossy(&bytes[..valid]);
            Err(Diagnostic::new(
                codes::MANIFEST_SYNTAX,
                "`oska.toml` is not UTF-8 text; save it as UTF-8",
                Span {
                    start: valid,
                    end: valid + err.error_len().unwrap_or(1),
                },
            )
            .locate(path, &before))
        }
    }
}

/// What is wrong with `text` as a whole, before it is read line by line:
/// a byte-order mark, which TOML allows and Oska does not, so that a
/// manifest is the same bytes however it was saved; and a carriage return
/// that is not followed by a line feed, which would otherwise end a line
/// that no reader of the file sees end.
fn text_errors(text: &str) -> Vec<Diagnostic> {
    if text.starts_with(BOM) {
        let span = Span {
            start: 0,
            end: BOM.len_utf8(),
        };
        return vec![Diagnostic::new(
            codes::MANIFEST_SYNTAX,
            "`oska.toml` starts with a byte-order mark; save it as UTF-8 without one",
            span,
        )
        .with_fix("remove the byte-order mark", vec![Edit::delete(span)])];
    }
    text.match_indices('\r')
        .filter(|(at, _)| !text[at + 1..].starts_with('\n'))
        .map(|(at, _)| {
            Diagnostic::new(
                codes::MANIFEST_SYNTAX,
                "a carriage return must be followed by a line feed: lines end with `\\n` or `\\r\\n`",
                Span {
                    start: at,
                    end: at + 1,
                },
            )
        })
        .collect()
}

/// The byte-order mark.
const BOM: char = '\u{feff}';

/// Whether `name` is a snake_case name: lowercase letters and digits in
/// words joined by single underscores, starting with a letter.
pub(crate) fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.split('_').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// Whether `version` is a semantic version: `MAJOR.MINOR.PATCH`, with an
/// optional `-prerelease` and `+build`.
fn is_semver(version: &str) -> bool {
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let numeric = |s: &str| {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    let identifier =
        |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let core_ok = {
        let parts: Vec<&str> = core.split('.').collect();
        parts.len() == 3 && parts.iter().all(|p| numeric(p))
    };
    let pre_ok = pre.map_or(true, |p| {
        p.split('.')
            .all(|id| identifier(id) && (!id.chars().all(|c| c.is_ascii_digit()) || numeric(id)))
    });
    let build_ok = build.map_or(true, |b| b.split('.').all(identifier));
    core_ok && pre_ok && build_ok
}

/// The table the parser is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    /// Before any header.
    None,
    /// `[package]`.
    Package,
    /// A table already reported: its keys are skipped.
    Skipped,
}

/// A parsed value.
enum Value {
    Str(String),
    Strings,
}

struct Parser<'a> {
    path: &'a str,
    text: &'a str,
    pos: usize,
    errors: Vec<Diagnostic>,
    table: Table,
    /// Where `[package]` was written.
    package: Option<Span>,
    name: Option<String>,
    version: Option<String>,
    /// The keys of `[package]` seen so far, and where each was written.
    keys_seen: Vec<(String, Span)>,
}

impl Parser<'_> {
    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.errors.push(Diagnostic::new(code, message, span));
    }

    /// Record `diagnostic`; [`Parser::finish`] locates every one at once.
    fn push(&mut self, diagnostic: Diagnostic) {
        self.errors.push(diagnostic);
    }

    fn rest(&self) -> &str {
        &self.text[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }

    /// Skip a comment, if one starts here, up to the line's end.
    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while !matches!(self.peek(), None | Some('\n')) {
                self.bump();
            }
        }
    }

    fn at_line_end(&self) -> bool {
        matches!(self.peek(), None | Some('\n' | '\r' | '#'))
    }

    /// Move to the start of the next line.
    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            self.bump();
            if c == '\n' {
                break;
            }
        }
    }

    /// The span of the rest of the line, without a comment or trailing
    /// blanks, from `start`.
    fn line_span_from(&self, start: usize) -> Span {
        let line = self.text[start..].split('\n').next().unwrap_or("");
        let line = line.split('#').next().unwrap_or(line).trim_end();
        Span {
            start,
            end: start + line.len(),
        }
    }

    fn run(&mut self) {
        loop {
            self.skip_blanks();
            match self.peek() {
                None => break,
                Some('\n' | '\r') => self.bump(),
                Some('#') => self.skip_comment(),
                Some('[') => self.header(),
                Some(_) => self.pair(),
            }
        }
    }

    // ---- Headers ------------------------------------------------------

    fn header(&mut self) {
        let start = self.pos;
        if self.rest().starts_with("[[") {
            let span = self.line_span_from(start);
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support arrays of tables (`[[...]]`)",
                span,
            );
            self.table = Table::Skipped;
            self.skip_line();
            return;
        }
        self.bump();
        let name_start = self.pos;
        let line_end = self.text[name_start..]
            .find('\n')
            .map_or(self.text.len(), |at| name_start + at);
        let Some(close) = self.text[name_start..line_end].find(']') else {
            let span = self.line_span_from(start);
            self.error(
                codes::MANIFEST_SYNTAX,
                "a table header is missing its closing `]`",
                span,
            );
            self.table = Table::Skipped;
            self.skip_line();
            return;
        };
        let inner = &self.text[name_start..name_start + close];
        let lead = inner.len() - inner.trim_start().len();
        let name = inner.trim();
        let span = Span {
            start: name_start + lead,
            end: name_start + lead + name.len(),
        };
        self.pos = name_start + close + 1;
        self.table = Table::Skipped;
        if name.contains('.') {
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support dotted keys, such as `a.b` in a table header",
                span,
            );
        } else if name.starts_with(['"', '\'']) {
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support quoted keys",
                span,
            );
        } else if name.is_empty() || !name.chars().all(is_bare_key_char) {
            self.error(
                codes::MANIFEST_SYNTAX,
                "a table header needs a name made of letters, digits, `_` and `-`",
                span,
            );
        } else {
            self.table_named(name, span);
        }
        self.skip_blanks();
        if !self.at_line_end() {
            let extra = self.token_span();
            self.error(
                codes::MANIFEST_SYNTAX,
                "unexpected text after a table header",
                extra,
            );
        }
        self.skip_line();
    }

    fn table_named(&mut self, name: &str, span: Span) {
        match (name, self.package) {
            ("package", Some(first)) => self.push(
                Diagnostic::new(
                    codes::MANIFEST_DUPLICATE,
                    "`[package]` is written twice",
                    span,
                )
                .with_related("first written here", first),
            ),
            ("package", None) => {
                self.package = Some(span);
                self.table = Table::Package;
            }
            ("dependencies", _) => self.error(
                codes::MANIFEST_DEPENDENCIES,
                "`[dependencies]` is not supported yet: dependencies arrive in milestone 5",
                span,
            ),
            _ => {
                let d = suggest::unknown(
                    codes::MANIFEST_UNKNOWN,
                    format!("unknown table `{name}`; `oska.toml` has only `[package]` for now"),
                    name,
                    span,
                    &["package"],
                );
                self.push(d);
            }
        }
    }

    /// The span of the text up to the next blank, comma or bracket.
    fn token_span(&self) -> Span {
        let rest = self.rest();
        let len = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ',' | ']' | '#'))
            .unwrap_or(rest.len());
        Span {
            start: self.pos,
            end: self.pos + len.max(rest.chars().next().map_or(0, char::len_utf8)),
        }
    }

    // ---- Pairs --------------------------------------------------------

    fn pair(&mut self) {
        let key_start = self.pos;
        if self.rest().starts_with(['"', '\'']) {
            let span = self.token_span_quoted();
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support quoted keys",
                span,
            );
            self.skip_line();
            return;
        }
        while self.peek().is_some_and(is_bare_key_char) {
            self.bump();
        }
        let key_span = Span {
            start: key_start,
            end: self.pos,
        };
        if key_span.start == key_span.end {
            let span = self.token_span();
            self.error(
                codes::MANIFEST_SYNTAX,
                "expected a key or a `[table]` header",
                span,
            );
            self.skip_line();
            return;
        }
        if self.peek() == Some('.') {
            while self.peek().is_some_and(|c| is_bare_key_char(c) || c == '.') {
                self.bump();
            }
            let span = Span {
                start: key_start,
                end: self.pos,
            };
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support dotted keys",
                span,
            );
            // The key was written, so it is not also reported missing.
            let key = &self.text[key_span.start..key_span.end];
            if self.table == Table::Package && ["name", "version"].contains(&key) {
                self.keys_seen.push((key.to_string(), key_span));
            }
            self.skip_line();
            return;
        }
        self.skip_blanks();
        if self.peek() != Some('=') {
            self.error(codes::MANIFEST_SYNTAX, "expected `=` after a key", key_span);
            self.skip_line();
            return;
        }
        self.bump();
        self.skip_blanks();
        let value_start = self.pos;
        let Some(value) = self.value(false) else {
            // The key was written, so it is not also reported missing.
            let key = &self.text[key_span.start..key_span.end];
            if self.table == Table::Package && ["name", "version"].contains(&key) {
                self.keys_seen.push((key.to_string(), key_span));
            }
            self.skip_line();
            return;
        };
        let value_span = Span {
            start: value_start,
            end: self.pos,
        };
        self.skip_blanks();
        if !self.at_line_end() {
            let extra = self.token_span();
            self.error(
                codes::MANIFEST_SYNTAX,
                "unexpected text after a value",
                extra,
            );
        }
        self.skip_line();
        let key = self.text[key_span.start..key_span.end].to_string();
        self.assign(&key, key_span, value, value_span);
    }

    /// The span of a quoted token, quotes included, or the rest of the
    /// line when it is not closed.
    fn token_span_quoted(&self) -> Span {
        let quote = self.rest().chars().next().unwrap_or('"');
        let body = &self.rest()[1..];
        let line_end = body.find('\n').unwrap_or(body.len());
        let len = body[..line_end]
            .find(quote)
            .map_or_else(|| self.line_span_from(self.pos).end - self.pos, |at| at + 2);
        Span {
            start: self.pos,
            end: self.pos + len,
        }
    }

    fn assign(&mut self, key: &str, key_span: Span, value: Value, value_span: Span) {
        match self.table {
            Table::Skipped => {}
            Table::None => self.error(
                codes::MANIFEST_UNKNOWN,
                format!("`{key}` is outside any table; milestone 1 keys go under `[package]`"),
                key_span,
            ),
            Table::Package => {
                if !["name", "version"].contains(&key) {
                    let d = suggest::unknown(
                        codes::MANIFEST_UNKNOWN,
                        format!(
                            "unknown key `{key}` in `[package]`; the keys are `name` and `version`"
                        ),
                        key,
                        key_span,
                        &["name", "version"],
                    );
                    self.push(d);
                    return;
                }
                if let Some(&(_, first)) = self.keys_seen.iter().find(|(k, _)| k == key) {
                    self.push(
                        Diagnostic::new(
                            codes::MANIFEST_DUPLICATE,
                            format!("`{key}` is written twice in `[package]`"),
                            key_span,
                        )
                        .with_related("first written here", first),
                    );
                    return;
                }
                self.keys_seen.push((key.to_string(), key_span));
                let Value::Str(text) = value else {
                    debug_assert!(matches!(value, Value::Strings));
                    self.error(
                        codes::MANIFEST_VALUE,
                        format!("`{key}` must be a string, not an array"),
                        value_span,
                    );
                    return;
                };
                if key == "name" {
                    if is_snake_case(&text) {
                        self.name = Some(text);
                    } else {
                        self.error(
                            codes::MANIFEST_VALUE,
                            format!(
                                "`name` must be snake_case, such as `my_package`, not `{text}`"
                            ),
                            value_span,
                        );
                    }
                } else if is_semver(&text) {
                    self.version = Some(text);
                } else {
                    self.error(
                        codes::MANIFEST_VALUE,
                        format!(
                            "`version` must be a semver version, such as `0.1.0`, not `{text}`"
                        ),
                        value_span,
                    );
                }
            }
        }
    }

    // ---- Values -------------------------------------------------------

    /// Parse a value at the cursor. `None` after reporting why not. In
    /// `in_array`, only a string is a value.
    fn value(&mut self, in_array: bool) -> Option<Value> {
        let start = self.pos;
        match self.peek() {
            None | Some('\n' | '\r' | '#') => {
                self.error(
                    codes::MANIFEST_SYNTAX,
                    "expected a value after `=`",
                    Span { start, end: start },
                );
                None
            }
            Some(quote @ ('"' | '\'')) => self.string(quote).map(Value::Str),
            Some('[') if in_array => {
                self.error(
                    codes::MANIFEST_UNSUPPORTED,
                    "`oska.toml` does not support nested arrays",
                    Span {
                        start,
                        end: start + 1,
                    },
                );
                None
            }
            Some('[') => self.array(),
            Some('{') => {
                self.error(
                    codes::MANIFEST_UNSUPPORTED,
                    "`oska.toml` does not support inline tables",
                    Span {
                        start,
                        end: start + 1,
                    },
                );
                None
            }
            Some(_) => {
                let span = self.token_span();
                let token = &self.text[span.start..span.end];
                let (code, message) = match classify(token) {
                    Some(feature) => (
                        codes::MANIFEST_UNSUPPORTED,
                        format!("`oska.toml` does not support {feature}: values are strings or arrays of strings"),
                    ),
                    None => (
                        codes::MANIFEST_SYNTAX,
                        format!("expected a quoted string, found `{token}`"),
                    ),
                };
                self.error(code, message, span);
                None
            }
        }
    }

    fn string(&mut self, quote: char) -> Option<String> {
        let start = self.pos;
        let triple: String = quote.to_string().repeat(3);
        if self.rest().starts_with(&triple) {
            self.error(
                codes::MANIFEST_UNSUPPORTED,
                "`oska.toml` does not support multiline strings",
                Span {
                    start,
                    end: start + 3,
                },
            );
            return None;
        }
        self.bump();
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('\n' | '\r') => {
                    let span = self.line_span_from(start);
                    self.error(codes::MANIFEST_SYNTAX, "a string is not closed", span);
                    return None;
                }
                Some(c) if c == quote => {
                    self.bump();
                    return Some(out);
                }
                Some('\\') if quote == '"' => {
                    let escape_start = self.pos;
                    self.bump();
                    match self.escape() {
                        Some(c) => out.push(c),
                        None => {
                            let end = self.pos.max(escape_start + 2).min(self.text.len());
                            self.error(
                                codes::MANIFEST_SYNTAX,
                                "unknown escape in a string",
                                Span {
                                    start: escape_start,
                                    end,
                                },
                            );
                            return None;
                        }
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.bump();
                }
            }
        }
    }

    /// The character an escape stands for, after its backslash.
    fn escape(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.bump();
        let digits = match c {
            '"' => return Some('"'),
            '\\' => return Some('\\'),
            'n' => return Some('\n'),
            't' => return Some('\t'),
            'r' => return Some('\r'),
            'b' => return Some('\u{8}'),
            'f' => return Some('\u{c}'),
            'u' => 4,
            'U' => 8,
            _ => return None,
        };
        let hex = self.rest().get(..digits)?;
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let code = u32::from_str_radix(hex, 16).ok()?;
        self.pos += digits;
        char::from_u32(code)
    }

    /// An array of strings, which may span lines and hold comments.
    fn array(&mut self) -> Option<Value> {
        self.bump();
        let mut ok = true;
        loop {
            while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                self.bump();
            }
            match self.peek() {
                Some('#') => self.skip_comment(),
                Some(']') => {
                    self.bump();
                    return ok.then_some(Value::Strings);
                }
                Some(',') => self.bump(),
                None => {
                    let end = self.text.len();
                    self.error(
                        codes::MANIFEST_SYNTAX,
                        "an array is not closed",
                        Span { start: end, end },
                    );
                    return None;
                }
                Some(_) => {
                    if self.value(true).is_none() {
                        ok = false;
                        // Skip the rest of the element to find the next.
                        let span = self.token_span();
                        self.pos = span.end.max(self.pos);
                        if self
                            .errors
                            .last()
                            .is_some_and(|e| e.code == codes::MANIFEST_SYNTAX)
                        {
                            return None;
                        }
                    }
                }
            }
        }
    }

    // ---- The end ------------------------------------------------------

    fn finish(mut self) -> Result<Manifest, Vec<Diagnostic>> {
        let start = Span { start: 0, end: 0 };
        match self.package {
            None => self.error(
                codes::MANIFEST_MISSING,
                "`oska.toml` needs a `[package]` table with a `name` and a `version`",
                start,
            ),
            Some(at) => {
                for key in ["name", "version"] {
                    if !self.keys_seen.iter().any(|(k, _)| k == key) {
                        self.error(
                            codes::MANIFEST_MISSING,
                            format!("`[package]` needs a `{key}`"),
                            at,
                        );
                    }
                }
            }
        }
        let lines = LineIndex::new(self.text);
        let path = self.path;
        match (self.name, self.version) {
            (Some(name), Some(version)) if self.errors.is_empty() => Ok(Manifest { name, version }),
            _ => Err(self
                .errors
                .into_iter()
                .map(|d| d.locate_in(&lines, path))
                .collect()),
        }
    }
}

fn is_bare_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// The TOML feature a bare token is a value of, when it is one.
fn classify(token: &str) -> Option<&'static str> {
    let digits =
        |s: &str, n: usize| s.len() >= n && s.as_bytes()[..n].iter().all(u8::is_ascii_digit);
    if matches!(token, "true" | "false") {
        Some("booleans")
    } else if digits(token, 4) && token[4..].starts_with('-')
        || token.as_bytes().get(2) == Some(&b':')
    {
        Some("dates")
    } else if matches!(token.trim_start_matches(['+', '-']), "inf" | "nan")
        || token.parse::<f64>().is_ok() && token.contains(['.', 'e', 'E'])
    {
        Some("floats")
    } else if token.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
        Some("integers")
    } else {
        None
    }
}
