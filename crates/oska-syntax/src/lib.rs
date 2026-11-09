//! Lexer, parser, and syntax tree for the Oska programming language.
//!
//! Diagnostics carry a stable error code and a byte span so tools (and AI
//! agents) can act on them without scraping human-readable text.
//!
//! Lexer codes are `E00xx`; parser codes are `E01xx`.

use std::fmt;

pub mod ast;
mod export;
mod parser;

pub use export::{module_to_json, module_to_tree, AST_VERSION};
pub use parser::{parse_expr, parse_module, parse_stmts, parse_type};

/// A half-open byte range `start..end` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// The span from the start of `start` to the end of `end`.
pub(crate) fn join(start: Span, end: Span) -> Span {
    Span {
        start: start.start,
        end: end.end,
    }
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
    /// A statement terminator, inserted for a significant newline. See
    /// [`lex`] for the rules.
    Newline,
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
    /// `self`, the explicit receiver parameter.
    SelfValue,
    For,
    In,
    While,
    Break,
    Continue,
    Const,
}

impl Keyword {
    /// The keyword as written in source.
    pub fn text(self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Let => "let",
            Self::Var => "var",
            Self::If => "if",
            Self::Else => "else",
            Self::Match => "match",
            Self::Return => "return",
            Self::True => "true",
            Self::False => "false",
            Self::Unsafe => "unsafe",
            Self::Cap => "cap",
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::Implies => "implies",
            Self::Scope => "scope",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Pub => "pub",
            Self::Use => "use",
            Self::Test => "test",
            Self::SelfValue => "self",
            Self::For => "for",
            Self::In => "in",
            Self::While => "while",
            Self::Break => "break",
            Self::Continue => "continue",
            Self::Const => "const",
        }
    }

    pub(crate) fn lookup(word: &str) -> Option<Self> {
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
            "self" => Self::SelfValue,
            "for" => Self::For,
            "in" => Self::In,
            "while" => Self::While,
            "break" => Self::Break,
            "continue" => Self::Continue,
            "const" => Self::Const,
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
    /// `..`, the (exclusive) range operator.
    DotDot,
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
    /// `@`. Oska has no `@` syntax; it is lexed only so the parser can
    /// reject `@` bindings in patterns with a clear error.
    At,
}

impl Punct {
    /// The punctuation as written in source.
    pub fn text(self) -> &'static str {
        match self {
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::Comma => ",",
            Self::Dot => ".",
            Self::Colon => ":",
            Self::DotDot => "..",
            Self::Question => "?",
            Self::Arrow => "->",
            Self::FatArrow => "=>",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Eq => "=",
            Self::EqEq => "==",
            Self::Bang => "!",
            Self::BangEq => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::AndAnd => "&&",
            Self::OrOr => "||",
            Self::Pipe => "|",
            Self::Hash => "#",
            Self::At => "@",
        }
    }
}

/// An error with a stable code, e.g. `E0001`, and the byte span it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    /// A machine-applicable fix, when the right edit is unambiguous.
    pub fix: Option<Fix>,
}

/// Source edits that resolve a diagnostic. Applying all of them to the
/// original source removes the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What the fix does, e.g. "remove `pub`".
    pub message: String,
    /// Non-overlapping edits, each against the original source.
    pub edits: Vec<Edit>,
}

/// Replace the source text at `span` with `replacement`. An empty span
/// inserts; an empty replacement deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub span: Span,
    pub replacement: String,
}

/// The character a backslash escape stands for, e.g. `n` for `\n`.
pub(crate) fn resolve_escape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        '\\' => '\\',
        '"' => '"',
        '{' => '{',
        '}' => '}',
        _ => return None,
    })
}

/// Errors produced by [`lex`].
pub type LexError = Diagnostic;

impl Diagnostic {
    /// A diagnostic with no fix.
    pub fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            message: message.into(),
            span,
            fix: None,
        }
    }

    /// Attach a fix made of `edits`.
    pub fn with_fix(mut self, message: impl Into<String>, edits: Vec<Edit>) -> Self {
        self.fix = Some(Fix {
            message: message.into(),
            edits,
        });
        self
    }

    /// Render as a single-line JSON object for machine consumers. The
    /// `fix` field follows `span`, and is omitted when there is no fix.
    pub fn to_json(&self) -> String {
        let mut out = format!(
            "{{\"code\":\"{}\",\"message\":\"{}\",\"span\":{}",
            self.code,
            json_escape(&self.message),
            span_json(self.span)
        );
        if let Some(fix) = &self.fix {
            let edits: Vec<String> = fix
                .edits
                .iter()
                .map(|e| {
                    format!(
                        "{{\"span\":{},\"replacement\":\"{}\"}}",
                        span_json(e.span),
                        json_escape(&e.replacement)
                    )
                })
                .collect();
            out.push_str(&format!(
                ",\"fix\":{{\"message\":\"{}\",\"edits\":[{}]}}",
                json_escape(&fix.message),
                edits.join(",")
            ));
        }
        out.push('}');
        out
    }
}

fn span_json(span: Span) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

impl Edit {
    /// Delete the text at `span`.
    pub fn delete(span: Span) -> Self {
        Self {
            span,
            replacement: String::new(),
        }
    }

    /// Insert `text` at byte offset `at`.
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self {
            span: Span { start: at, end: at },
            replacement: text.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{}]: {} (bytes {}..{})",
            self.code, self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for Diagnostic {}

pub(crate) fn json_escape(s: &str) -> String {
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

/// Tokenize Oska source text. Comments and insignificant whitespace are
/// discarded.
///
/// Newlines end statements (there is no `;`). A [`TokenKind::Newline`] is
/// emitted between two tokens on different lines when all of these hold:
///
/// - the innermost open bracket is not `(` or `[` (a `{` opened inside
///   parentheses makes newlines significant again);
/// - the token before the line break can end a statement: an identifier,
///   a literal, `self`, `return`, `break`, `continue`, `)`, `]`, `}`, or `?`;
/// - the first token of the next line is not `requires`, `ensures`, `else`,
///   or `{`, which continue the previous line.
///
/// Blank and comment-only lines produce at most one terminator, and none is
/// emitted at end of input.
pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer { src, pos: 0 };
    let mut tokens: Vec<Token> = Vec::new();
    let mut brackets: Vec<Punct> = Vec::new();
    loop {
        let newline_at = lexer.skip_trivia();
        let Some(token) = lexer.next_token()? else {
            break;
        };
        if let Some(at) = newline_at {
            if is_terminator(tokens.last(), &token, brackets.last()) {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    span: Span {
                        start: at,
                        end: at + 1,
                    },
                });
            }
        }
        match token.kind {
            TokenKind::Punct(open @ (Punct::LParen | Punct::LBracket | Punct::LBrace)) => {
                brackets.push(open);
            }
            TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                // Unbalanced closers are left for the parser to report.
                brackets.pop();
            }
            _ => {}
        }
        tokens.push(token);
    }
    Ok(tokens)
}

/// Whether a line break between `prev` and `next` ends a statement.
fn is_terminator(prev: Option<&Token>, next: &Token, innermost: Option<&Punct>) -> bool {
    if matches!(innermost, Some(Punct::LParen | Punct::LBracket)) {
        return false;
    }
    let Some(prev) = prev else {
        return false;
    };
    let prev_ends = matches!(
        prev.kind,
        TokenKind::Ident(_)
            | TokenKind::Int(_)
            | TokenKind::Str(_)
            | TokenKind::SinkStr { .. }
            | TokenKind::Keyword(
                Keyword::True
                    | Keyword::False
                    | Keyword::SelfValue
                    | Keyword::Return
                    | Keyword::Break
                    | Keyword::Continue
            )
            | TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::Question)
    );
    let next_continues = matches!(
        next.kind,
        TokenKind::Keyword(Keyword::Requires | Keyword::Ensures | Keyword::Else)
            | TokenKind::Punct(Punct::LBrace)
    );
    prev_ends && !next_continues
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
        LexError::new(
            code,
            message,
            Span {
                start,
                end: self.pos,
            },
        )
    }

    /// Whether only spaces or a comment follow on the current line.
    fn rest_of_line_is_blank(&self) -> bool {
        let rest = self.src[self.pos..].split('\n').next().unwrap_or("");
        let rest = rest.trim_start();
        rest.is_empty() || rest.starts_with("//")
    }

    /// Skip whitespace and comments. Returns the byte offset of the first
    /// line break skipped, if any.
    fn skip_trivia(&mut self) -> Option<usize> {
        let mut newline_at = None;
        loop {
            match self.peek() {
                Some('\n') => {
                    newline_at.get_or_insert(self.pos);
                    self.bump();
                }
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
        newline_at
    }

    fn next_token(&mut self) -> Result<Option<Token>, LexError> {
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
                    let next = self.bump();
                    let resolved = match next.and_then(resolve_escape) {
                        Some(c) => c,
                        None => {
                            let other = next;
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
            ('.', Some('.')) => (Punct::DotDot, true),
            (';', _) => {
                let err = self.error(
                    "E0005",
                    "unexpected `;`: Oska has no semicolons, newlines end statements".into(),
                    start,
                );
                // At the end of a line, deleting it is enough; between two
                // statements a line break is needed instead.
                return Err(if self.rest_of_line_is_blank() {
                    let span = err.span;
                    err.with_fix("remove `;`", vec![Edit::delete(span)])
                } else {
                    err
                });
            }
            ('(', _) => (Punct::LParen, false),
            (')', _) => (Punct::RParen, false),
            ('{', _) => (Punct::LBrace, false),
            ('}', _) => (Punct::RBrace, false),
            ('[', _) => (Punct::LBracket, false),
            (']', _) => (Punct::RBracket, false),
            (',', _) => (Punct::Comma, false),
            ('.', _) => (Punct::Dot, false),
            (':', _) => (Punct::Colon, false),
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
            ('@', _) => (Punct::At, false),
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
        let err = lex("let x = $").expect_err("should fail");
        assert_eq!(err.code, "E0001");
        assert_eq!(
            err.to_json(),
            "{\"code\":\"E0001\",\"message\":\"unexpected character '$'\",\"span\":{\"start\":8,\"end\":9}}"
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

    /// Render tokens compactly: identifiers and literals as written,
    /// keywords in lower case, punctuation by name, and terminators as `NL`.
    fn shape(src: &str) -> String {
        kinds(src)
            .into_iter()
            .map(|k| match k {
                TokenKind::Ident(name) | TokenKind::Int(name) => name,
                TokenKind::Str(value) => format!("{value:?}"),
                TokenKind::SinkStr { tag, value } => format!("{tag}{value:?}"),
                TokenKind::Keyword(kw) => format!("{kw:?}").to_lowercase(),
                TokenKind::Punct(p) => format!("{p:?}"),
                TokenKind::Newline => "NL".into(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn newline_ends_statement() {
        assert_eq!(shape("let a = 1\nlet b = 2"), "let a Eq 1 NL let b Eq 2");
    }

    #[test]
    fn newline_token_spans_the_line_break() {
        let tokens = lex("a\nb").expect("should lex");
        assert_eq!(
            tokens[1],
            Token {
                kind: TokenKind::Newline,
                span: Span { start: 1, end: 2 },
            }
        );
    }

    #[test]
    fn newlines_inside_parens_and_brackets_are_insignificant() {
        assert_eq!(
            shape("f(\n    a,\n    b\n)\nxs = [\n    1\n]"),
            "f LParen a Comma b RParen NL xs Eq LBracket 1 RBracket"
        );
    }

    #[test]
    fn braces_inside_parens_make_newlines_significant_again() {
        assert_eq!(
            shape("run(|| {\n    a()\n    b()\n})"),
            "run LParen OrOr LBrace a LParen RParen NL b LParen RParen NL RBrace RParen"
        );
    }

    #[test]
    fn contract_clauses_and_body_brace_continue_the_header() {
        let src = "fn f(x: Int) -> Int\n    requires x > 0\n    ensures |r| r > 0\n{\n    x\n}";
        assert_eq!(
            shape(src),
            "fn f LParen x Colon Int RParen Arrow Int \
             requires x Gt 0 ensures Pipe r Pipe r Gt 0 LBrace x NL RBrace"
        );
    }

    #[test]
    fn else_on_next_line_continues_if() {
        assert_eq!(
            shape("if a {\n    b\n}\nelse {\n    c\n}"),
            "if a LBrace b NL RBrace else LBrace c NL RBrace"
        );
    }

    #[test]
    fn trailing_dot_or_operator_continues_the_line() {
        assert_eq!(shape("xs.\n    len()"), "xs Dot len LParen RParen");
        assert_eq!(shape("a +\n    b"), "a Plus b");
    }

    #[test]
    fn blank_and_comment_lines_collapse_to_one_terminator() {
        assert_eq!(shape("a\n\n// note\n\nb"), "a NL b");
    }

    #[test]
    fn no_terminator_at_start_or_end_of_input() {
        assert_eq!(shape("\n\na\n\n"), "a");
    }

    #[test]
    fn jump_keywords_and_self_end_statements() {
        assert_eq!(shape("return\nx"), "return NL x");
        assert_eq!(
            shape("break\ncontinue\nself\nx"),
            "break NL continue NL selfvalue NL x"
        );
    }

    #[test]
    fn lexes_ranges() {
        assert_eq!(shape("a..b"), "a DotDot b");
        assert_eq!(
            shape("for i in 0..n {}"),
            "for i in 0 DotDot n LBrace RBrace"
        );
        assert_eq!(shape("a.b"), "a Dot b");
    }

    #[test]
    fn rejects_semicolons() {
        let err = lex("let a = 1; let b = 2").expect_err("should fail");
        assert_eq!(err.code, "E0005");
        assert_eq!(err.span, Span { start: 9, end: 10 });
    }

    #[test]
    fn lexes_remaining_keywords() {
        assert_eq!(
            shape("self for in while break continue const"),
            "selfvalue for in while break continue const"
        );
        assert_eq!(shape("selfish inner"), "selfish inner");
    }

    #[test]
    fn lexes_design_samples() {
        // The Syntax and Contracts samples from docs/design.md.
        let syntax = r#"
use std.sql.{Db, query}

pub struct Account { pub id: Int, balance: Int }

impl Account {
    pub fn deposit(var self, amount: Int)
        requires amount > 0
    {
        self.balance = self.balance + amount
    }
}

fn describe(s: Shape) -> String {
    match s {
        Shape.Circle(radius: r) if r > 100 => "big circle"
        Shape.Circle(radius: _) => "circle"
        Shape.Square(side: n) => "square"
    }
}

test "deposit adds to the balance" {
    var a = Account(id: 1, balance: 0)
    a.deposit(amount: 5)
    t.assert_eq(a.balance, 5)
}
"#;
        let deposit = shape(syntax);
        assert!(
            deposit.contains(
                "pub fn deposit LParen var selfvalue Comma amount Colon Int RParen \
                 requires amount Gt 0 LBrace self"
            ),
            "{deposit}"
        );
        assert!(
            deposit.contains("FatArrow \"big circle\" NL Shape Dot Circle"),
            "{deposit}"
        );

        let contracts = "fn withdraw(acct: Account, amount: Int) -> Result<Account, Error>
    requires amount > 0
    ensures |r| r.is_ok() implies r.unwrap().balance >= 0
{ x }";
        assert!(
            !shape(contracts).contains("NL"),
            "a multi-line contract header has no terminators: {}",
            shape(contracts)
        );
    }

    #[test]
    fn lexes_examples() {
        lex(include_str!("../../../examples/hello.osk")).expect("hello.osk should lex");
        lex(include_str!("../../../examples/capabilities.osk"))
            .expect("capabilities.osk should lex");
    }
}
