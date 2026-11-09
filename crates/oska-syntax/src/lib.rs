//! Lexer for the Oska programming language.
//!
//! The parser and AST will live in this crate too. Diagnostics carry a
//! stable error code and a byte span so tools (and AI agents) can act on
//! them without scraping human-readable text.

use std::fmt;

/// A half-open byte range `start..end` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Keyword(Keyword),
    /// Integer literal, kept as written (digits and `_` separators).
    Int(String),
    /// String literal with escapes resolved. `{name}` interpolation is
    /// preserved verbatim for the parser to handle.
    Str(String),
    /// Sink literal: an identifier immediately followed by a string, e.g.
    /// `sql"SELECT * FROM t WHERE id = ?"`. Escapes are resolved. Sink
    /// literals never interpolate: values must go in as parameters.
    SinkStr {
        tag: String,
        value: String,
    },
    Punct(Punct),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Fn,
    Let,
    Var,
    If,
    Else,
    Match,
    Return,
    True,
    False,
    /// Reserved: v0 has no `unsafe`, but the word is never an identifier.
    Unsafe,
    Cap,
    Requires,
    Ensures,
    Implies,
    Scope,
    Struct,
    Enum,
    Trait,
    Impl,
    Pub,
    Use,
    Test,
}

impl Keyword {
    fn lookup(word: &str) -> Option<Self> {
        let kw = match word {
            "fn" => Self::Fn,
            "let" => Self::Let,
            "var" => Self::Var,
            "if" => Self::If,
            "else" => Self::Else,
            "match" => Self::Match,
            "return" => Self::Return,
            "true" => Self::True,
            "false" => Self::False,
            "unsafe" => Self::Unsafe,
            "cap" => Self::Cap,
            "requires" => Self::Requires,
            "ensures" => Self::Ensures,
            "implies" => Self::Implies,
            "scope" => Self::Scope,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "impl" => Self::Impl,
            "pub" => Self::Pub,
            "use" => Self::Use,
            "test" => Self::Test,
            _ => return None,
        };
        Some(kw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Punct {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Colon,
    Semi,
    Question,
    Arrow,
    FatArrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    Bang,
    BangEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    /// `|`, which delimits closure parameters (`|r| ...`).
    Pipe,
    /// `#`, which starts an attribute (`#[integration]`).
    Hash,
}

/// A lexing error with a stable code, e.g. `E0001`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl LexError {
    /// Render as a single-line JSON object for machine consumers.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"code\":\"{}\",\"message\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}}}}",
            self.code,
            json_escape(&self.message),
            self.span.start,
            self.span.end
        )
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{}]: {} (bytes {}..{})",
            self.code, self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for LexError {}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Tokenize Oska source text. Comments and whitespace are discarded.
pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer { src, pos: 0 };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrKind {
    Plain,
    Sink,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.src[self.pos..].chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn error(&self, code: &'static str, message: String, start: usize) -> LexError {
        LexError {
            code,
            message,
            span: Span {
                start,
                end: self.pos,
            },
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_second() == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_trivia();
        let start = self.pos;
        let Some(c) = self.bump() else {
            return Ok(None);
        };
        let kind = match c {
            c if c.is_ascii_alphabetic() || c == '_' => self.ident_or_sink_str(start)?,
            c if c.is_ascii_digit() => self.int(start),
            '"' => self.string(start)?,
            c => TokenKind::Punct(self.punct(c, start)?),
        };
        Ok(Some(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
        }))
    }

    fn ident_or_sink_str(&mut self, start: usize) -> Result<TokenKind, LexError> {
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            self.bump();
        }
        let word = &self.src[start..self.pos];
        if let Some(kw) = Keyword::lookup(word) {
            return Ok(TokenKind::Keyword(kw));
        }
        if self.peek() == Some('"') {
            let tag = word.to_string();
            self.bump();
            let value = self.string_body(start, StrKind::Sink)?;
            return Ok(TokenKind::SinkStr { tag, value });
        }
        Ok(TokenKind::Ident(word.to_string()))
    }

    fn int(&mut self, start: usize) -> TokenKind {
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '_') {
            self.bump();
        }
        TokenKind::Int(self.src[start..self.pos].to_string())
    }

    fn string(&mut self, start: usize) -> Result<TokenKind, LexError> {
        self.string_body(start, StrKind::Plain).map(TokenKind::Str)
    }

    /// Lex the rest of a string literal after its opening quote. `start` is
    /// where the whole literal (including any sink tag) begins.
    fn string_body(&mut self, start: usize, kind: StrKind) -> Result<String, LexError> {
        let mut value = String::new();
        loop {
            match self.bump() {
                None => {
                    return Err(self.error("E0002", "unterminated string literal".into(), start));
                }
                Some('"') => break,
                Some('\\') => {
                    let escape_start = self.pos - 1;
                    let resolved = match self.bump() {
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some('r') => '\r',
                        Some('0') => '\0',
                        Some('\\') => '\\',
                        Some('"') => '"',
                        Some('{') => '{',
                        Some('}') => '}',
                        other => {
                            let shown =
                                other.map_or_else(|| "end of input".into(), |c| format!("{c:?}"));
                            return Err(self.error(
                                "E0003",
                                format!("invalid escape sequence: \\ followed by {shown}"),
                                escape_start,
                            ));
                        }
                    };
                    value.push(resolved);
                }
                Some(c @ ('{' | '}')) if kind == StrKind::Sink => {
                    return Err(self.error(
                        "E0004",
                        format!(
                            "sink literals cannot interpolate: found {c:?}; pass values as \
                             parameters (use \\{c} for a literal brace)"
                        ),
                        self.pos - 1,
                    ));
                }
                Some(c) => value.push(c),
            }
        }
        Ok(value)
    }

    fn punct(&mut self, c: char, start: usize) -> Result<Punct, LexError> {
        let (punct, two_chars) = match (c, self.peek()) {
            ('-', Some('>')) => (Punct::Arrow, true),
            ('=', Some('>')) => (Punct::FatArrow, true),
            ('=', Some('=')) => (Punct::EqEq, true),
            ('!', Some('=')) => (Punct::BangEq, true),
            ('<', Some('=')) => (Punct::Le, true),
            ('>', Some('=')) => (Punct::Ge, true),
            ('&', Some('&')) => (Punct::AndAnd, true),
            ('|', Some('|')) => (Punct::OrOr, true),
            ('(', _) => (Punct::LParen, false),
            (')', _) => (Punct::RParen, false),
            ('{', _) => (Punct::LBrace, false),
            ('}', _) => (Punct::RBrace, false),
            ('[', _) => (Punct::LBracket, false),
            (']', _) => (Punct::RBracket, false),
            (',', _) => (Punct::Comma, false),
            ('.', _) => (Punct::Dot, false),
            (':', _) => (Punct::Colon, false),
            (';', _) => (Punct::Semi, false),
            ('?', _) => (Punct::Question, false),
            ('+', _) => (Punct::Plus, false),
            ('-', _) => (Punct::Minus, false),
            ('*', _) => (Punct::Star, false),
            ('/', _) => (Punct::Slash, false),
            ('%', _) => (Punct::Percent, false),
            ('=', _) => (Punct::Eq, false),
            ('!', _) => (Punct::Bang, false),
            ('<', _) => (Punct::Lt, false),
            ('>', _) => (Punct::Gt, false),
            ('|', _) => (Punct::Pipe, false),
            ('#', _) => (Punct::Hash, false),
            _ => {
                return Err(self.error("E0001", format!("unexpected character {c:?}"), start));
            }
        };
        if two_chars {
            self.bump();
        }
        Ok(punct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src)
            .expect("source should lex")
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn lexes_function_signature() {
        assert_eq!(
            kinds("fn main(io: Stdout) -> Result"),
            vec![
                TokenKind::Keyword(Keyword::Fn),
                TokenKind::Ident("main".into()),
                TokenKind::Punct(Punct::LParen),
                TokenKind::Ident("io".into()),
                TokenKind::Punct(Punct::Colon),
                TokenKind::Ident("Stdout".into()),
                TokenKind::Punct(Punct::RParen),
                TokenKind::Punct(Punct::Arrow),
                TokenKind::Ident("Result".into()),
            ]
        );
    }

    #[test]
    fn skips_comments_and_resolves_escapes() {
        assert_eq!(
            kinds("// hi\n\"a\\n{x}\" 1_000"),
            vec![
                TokenKind::Str("a\n{x}".into()),
                TokenKind::Int("1_000".into()),
            ]
        );
    }

    #[test]
    fn reports_unterminated_string() {
        let err = lex("\"oops").expect_err("should fail");
        assert_eq!(err.code, "E0002");
        assert_eq!(err.span, Span { start: 0, end: 5 });
    }

    #[test]
    fn reports_unexpected_character_as_json() {
        let err = lex("let x = @").expect_err("should fail");
        assert_eq!(err.code, "E0001");
        assert_eq!(
            err.to_json(),
            "{\"code\":\"E0001\",\"message\":\"unexpected character '@'\",\"span\":{\"start\":8,\"end\":9}}"
        );
    }

    #[test]
    fn lexes_new_keywords() {
        let cases = [
            ("cap", Keyword::Cap),
            ("requires", Keyword::Requires),
            ("ensures", Keyword::Ensures),
            ("implies", Keyword::Implies),
            ("scope", Keyword::Scope),
            ("struct", Keyword::Struct),
            ("enum", Keyword::Enum),
            ("trait", Keyword::Trait),
            ("impl", Keyword::Impl),
            ("pub", Keyword::Pub),
            ("use", Keyword::Use),
            ("test", Keyword::Test),
            ("unsafe", Keyword::Unsafe),
        ];
        for (src, kw) in cases {
            assert_eq!(kinds(src), vec![TokenKind::Keyword(kw)], "lexing {src:?}");
        }
    }

    #[test]
    fn keywords_are_not_identifiers_but_prefixed_words_are() {
        assert_eq!(kinds("caps"), vec![TokenKind::Ident("caps".into())]);
        assert_eq!(kinds("testing"), vec![TokenKind::Ident("testing".into())]);
    }

    #[test]
    fn lexes_closure_pipes_distinct_from_oror() {
        assert_eq!(
            kinds("|x| a || b"),
            vec![
                TokenKind::Punct(Punct::Pipe),
                TokenKind::Ident("x".into()),
                TokenKind::Punct(Punct::Pipe),
                TokenKind::Ident("a".into()),
                TokenKind::Punct(Punct::OrOr),
                TokenKind::Ident("b".into()),
            ]
        );
    }

    #[test]
    fn lexes_attribute() {
        assert_eq!(
            kinds("#[integration]"),
            vec![
                TokenKind::Punct(Punct::Hash),
                TokenKind::Punct(Punct::LBracket),
                TokenKind::Ident("integration".into()),
                TokenKind::Punct(Punct::RBracket),
            ]
        );
    }

    #[test]
    fn lexes_contract_clause() {
        assert_eq!(
            kinds("ensures |r| r.ok implies x"),
            vec![
                TokenKind::Keyword(Keyword::Ensures),
                TokenKind::Punct(Punct::Pipe),
                TokenKind::Ident("r".into()),
                TokenKind::Punct(Punct::Pipe),
                TokenKind::Ident("r".into()),
                TokenKind::Punct(Punct::Dot),
                TokenKind::Ident("ok".into()),
                TokenKind::Keyword(Keyword::Implies),
                TokenKind::Ident("x".into()),
            ]
        );
    }

    #[test]
    fn lexes_sink_literal() {
        let tokens = lex("db.query(sql\"SELECT 1 WHERE id = ?\", id)").expect("should lex");
        assert_eq!(
            tokens[4],
            Token {
                kind: TokenKind::SinkStr {
                    tag: "sql".into(),
                    value: "SELECT 1 WHERE id = ?".into(),
                },
                span: Span { start: 9, end: 35 },
            }
        );
    }

    #[test]
    fn sink_literal_allows_escaped_braces() {
        assert_eq!(
            kinds("sql\"SELECT '\\{\\}'\""),
            vec![TokenKind::SinkStr {
                tag: "sql".into(),
                value: "SELECT '{}'".into(),
            }]
        );
    }

    #[test]
    fn rejects_interpolation_in_sink_literal() {
        let src = "sql\"SELECT * FROM t WHERE id = {id}\"";
        let err = lex(src).expect_err("should fail");
        assert_eq!(err.code, "E0004");
        let brace = src.find('{').expect("source contains a brace");
        assert_eq!(
            err.span,
            Span {
                start: brace,
                end: brace + 1
            }
        );
        assert!(err.to_json().contains("\"code\":\"E0004\""));
    }

    #[test]
    fn rejects_closing_brace_in_sink_literal() {
        assert_eq!(lex("sql\"a}\"").expect_err("should fail").code, "E0004");
    }

    #[test]
    fn ident_then_space_then_string_is_not_a_sink_literal() {
        assert_eq!(
            kinds("sql \"{x}\""),
            vec![TokenKind::Ident("sql".into()), TokenKind::Str("{x}".into())]
        );
    }

    #[test]
    fn keyword_then_string_is_not_a_sink_literal() {
        assert_eq!(
            kinds("return\"x\""),
            vec![
                TokenKind::Keyword(Keyword::Return),
                TokenKind::Str("x".into())
            ]
        );
    }

    #[test]
    fn reports_unterminated_sink_literal_from_tag_start() {
        let err = lex("x sql\"oops").expect_err("should fail");
        assert_eq!(err.code, "E0002");
        assert_eq!(err.span, Span { start: 2, end: 10 });
    }

    #[test]
    fn lexes_examples() {
        lex(include_str!("../../../examples/hello.osk")).expect("hello.osk should lex");
        lex(include_str!("../../../examples/capabilities.osk"))
            .expect("capabilities.osk should lex");
    }
}
