//! Recursive-descent parser for Oska expressions, statements, and types.
//!
//! Operator precedence, loosest first:
//!
//! | Level | Operators | Associativity |
//! |---|---|---|
//! | 1 | `implies` | right |
//! | 2 | `..` | none |
//! | 3 | `\|\|` | left |
//! | 4 | `&&` | left |
//! | 5 | `==` `!=` `<` `<=` `>` `>=` | none (chaining is error E0105) |
//! | 6 | `+` `-` | left |
//! | 7 | `*` `/` `%` | left |
//! | 8 | prefix `-` `!` | prefix |
//! | 9 | call `f(..)`, field `.name`, `?` | postfix |
//!
//! Diagnostic codes:
//!
//! - `E0100`: expected an expression or type, found something else
//! - `E0101`: expected a specific token
//! - `E0102`: string interpolation holds something other than a name or
//!   field path
//! - `E0103`: a bare `{ ... }` block used as a statement
//! - `E0104`: `let` or `var` without an initializer
//! - `E0105`: chained comparison such as `a < b < c`
//! - `E0106`: invalid assignment target

use crate::ast::{
    Arg, BinaryOp, Block, ClosureParam, Expr, ExprKind, Ident, Stmt, StmtKind, StrPart, Type,
    TypeKind, UnaryOp,
};
use crate::{lex, resolve_escape, Diagnostic, Keyword, Punct, Span, Token, TokenKind};

/// Parse a single expression that spans the whole input.
pub fn parse_expr(src: &str) -> Result<Expr, Diagnostic> {
    let mut parser = Parser::new(src)?;
    let expr = parser.expr()?;
    parser.expect_end()?;
    Ok(expr)
}

/// Parse a sequence of statements, as they would appear inside a block.
pub fn parse_stmts(src: &str) -> Result<Vec<Stmt>, Diagnostic> {
    let mut parser = Parser::new(src)?;
    parser.stmts_until(None)
}

/// Parse a single type that spans the whole input.
pub fn parse_type(src: &str) -> Result<Type, Diagnostic> {
    let mut parser = Parser::new(src)?;
    let ty = parser.ty()?;
    parser.expect_end()?;
    Ok(ty)
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    /// End of the most recently consumed token.
    prev_end: usize,
}

fn error(code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic {
        code,
        message: message.into(),
        span,
    }
}

fn bare_block(span: Span) -> Diagnostic {
    error(
        "E0103",
        "a bare `{ ... }` block cannot be a statement; \
         bind it with `let x = { ... }` or remove the braces",
        span,
    )
}

fn describe(kind: Option<&TokenKind>) -> String {
    match kind {
        None => "end of input".into(),
        Some(TokenKind::Ident(name)) => format!("`{name}`"),
        Some(TokenKind::Keyword(kw)) => format!("`{}`", kw.text()),
        Some(TokenKind::Int(digits)) => format!("`{digits}`"),
        Some(TokenKind::Str(_)) => "a string".into(),
        Some(TokenKind::SinkStr { tag, .. }) => format!("a `{tag}` sink literal"),
        Some(TokenKind::Punct(p)) => format!("`{}`", p.text()),
        Some(TokenKind::Newline) => "a line break".into(),
    }
}

fn join(start: Span, end: Span) -> Span {
    Span {
        start: start.start,
        end: end.end,
    }
}

fn comparison_op(kind: Option<&TokenKind>) -> Option<BinaryOp> {
    match kind {
        Some(TokenKind::Punct(p)) => match p {
            Punct::EqEq => Some(BinaryOp::Eq),
            Punct::BangEq => Some(BinaryOp::Ne),
            Punct::Lt => Some(BinaryOp::Lt),
            Punct::Le => Some(BinaryOp::Le),
            Punct::Gt => Some(BinaryOp::Gt),
            Punct::Ge => Some(BinaryOp::Ge),
            _ => None,
        },
        _ => None,
    }
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Result<Self, Diagnostic> {
        Ok(Self {
            src,
            tokens: lex(src)?,
            pos: 0,
            prev_end: 0,
        })
    }

    // ----- token helpers -----

    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn peek_at(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|t| &t.kind)
    }

    /// Span of the next token, or an empty span at end of input.
    fn peek_span(&self) -> Span {
        self.tokens.get(self.pos).map_or(
            Span {
                start: self.src.len(),
                end: self.src.len(),
            },
            |t| t.span,
        )
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        self.prev_end = token.span.end;
        Some(token)
    }

    fn at_punct(&self, p: Punct) -> bool {
        self.peek() == Some(&TokenKind::Punct(p))
    }

    fn at_kw(&self, kw: Keyword) -> bool {
        self.peek() == Some(&TokenKind::Keyword(kw))
    }

    fn eat_punct(&mut self, p: Punct) -> bool {
        let found = self.at_punct(p);
        if found {
            self.bump();
        }
        found
    }

    fn expected(&self, what: &str) -> Diagnostic {
        error(
            "E0101",
            format!("expected {what}, found {}", describe(self.peek())),
            self.peek_span(),
        )
    }

    fn expect_punct(&mut self, p: Punct) -> Result<Span, Diagnostic> {
        if self.at_punct(p) {
            Ok(self
                .bump()
                .map(|t| t.span)
                .unwrap_or_else(|| self.peek_span()))
        } else {
            Err(self.expected(&format!("`{}`", p.text())))
        }
    }

    fn expect_kw(&mut self, kw: Keyword) -> Result<Span, Diagnostic> {
        if self.at_kw(kw) {
            Ok(self
                .bump()
                .map(|t| t.span)
                .unwrap_or_else(|| self.peek_span()))
        } else {
            Err(self.expected(&format!("`{}`", kw.text())))
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<Ident, Diagnostic> {
        match self.peek() {
            Some(TokenKind::Ident(name)) => {
                let name = name.clone();
                let span = self.peek_span();
                self.bump();
                Ok(Ident { name, span })
            }
            _ => Err(self.expected(what)),
        }
    }

    fn expect_end(&self) -> Result<(), Diagnostic> {
        match self.peek() {
            None => Ok(()),
            Some(_) => Err(self.expected("end of input")),
        }
    }

    fn span_from(&self, start: Span) -> Span {
        Span {
            start: start.start,
            end: self.prev_end,
        }
    }

    // ----- statements -----

    /// Statements separated by newlines, up to `close` (or end of input).
    fn stmts_until(&mut self, close: Option<Punct>) -> Result<Vec<Stmt>, Diagnostic> {
        let mut stmts = Vec::new();
        loop {
            while self.peek() == Some(&TokenKind::Newline) {
                self.bump();
            }
            let at_close = match close {
                Some(p) => self.at_punct(p) || self.peek().is_none(),
                None => self.peek().is_none(),
            };
            if at_close {
                return Ok(stmts);
            }
            stmts.push(self.stmt()?);
            match self.peek() {
                Some(TokenKind::Newline) => {
                    self.bump();
                }
                None => {}
                Some(TokenKind::Punct(p)) if Some(*p) == close => {}
                // A `{` at the start of a line continues the previous line,
                // so a bare block written on its own line lands here.
                Some(TokenKind::Punct(Punct::LBrace)) => return Err(bare_block(self.peek_span())),
                Some(_) => {
                    return Err(self.expected(match close {
                        Some(_) => "a line break or `}` after the statement",
                        None => "a line break after the statement",
                    }))
                }
            }
        }
    }

    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek_span();
        let kind = match self.peek() {
            Some(TokenKind::Keyword(kw @ (Keyword::Let | Keyword::Var))) => {
                let mutable = *kw == Keyword::Var;
                let keyword = kw.text();
                self.bump();
                let name = self.expect_ident("a name")?;
                let ty = if self.eat_punct(Punct::Colon) {
                    Some(self.ty()?)
                } else {
                    None
                };
                if !self.eat_punct(Punct::Eq) {
                    return Err(error(
                        "E0104",
                        format!(
                            "`{keyword}` needs an initializer: write `{keyword} {} = <value>`",
                            name.name
                        ),
                        self.span_from(start),
                    ));
                }
                let value = self.expr()?;
                StmtKind::Let {
                    mutable,
                    name,
                    ty,
                    value,
                }
            }
            Some(TokenKind::Keyword(Keyword::Return)) => {
                self.bump();
                let ends = matches!(
                    self.peek(),
                    None | Some(TokenKind::Newline | TokenKind::Punct(Punct::RBrace))
                );
                StmtKind::Return(if ends { None } else { Some(self.expr()?) })
            }
            Some(TokenKind::Keyword(Keyword::Break)) => {
                self.bump();
                StmtKind::Break
            }
            Some(TokenKind::Keyword(Keyword::Continue)) => {
                self.bump();
                StmtKind::Continue
            }
            Some(TokenKind::Keyword(Keyword::For)) => {
                self.bump();
                let binding = self.expect_ident("a loop variable")?;
                self.expect_kw(Keyword::In)?;
                let iter = self.expr()?;
                let body = self.block()?;
                StmtKind::For {
                    binding,
                    iter,
                    body,
                }
            }
            Some(TokenKind::Keyword(Keyword::While)) => {
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            Some(TokenKind::Punct(Punct::LBrace)) => return Err(bare_block(start)),
            _ => {
                let expr = self.expr()?;
                if self.at_punct(Punct::Eq) {
                    if !matches!(
                        expr.kind,
                        ExprKind::Name(_) | ExprKind::SelfValue | ExprKind::Field { .. }
                    ) {
                        return Err(error(
                            "E0106",
                            "only a name, `self`, or a field can be assigned to",
                            expr.span,
                        ));
                    }
                    self.bump();
                    let value = self.expr()?;
                    StmtKind::Assign {
                        target: expr,
                        value,
                    }
                } else {
                    StmtKind::Expr(expr)
                }
            }
        };
        Ok(Stmt {
            kind,
            span: self.span_from(start),
        })
    }

    fn block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.expect_punct(Punct::LBrace)?;
        let stmts = self.stmts_until(Some(Punct::RBrace))?;
        self.expect_punct(Punct::RBrace)?;
        Ok(Block {
            stmts,
            span: self.span_from(start),
        })
    }

    // ----- expressions -----

    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        self.implies()
    }

    fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr {
            span: join(lhs.span, rhs.span),
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        }
    }

    fn implies(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.range()?;
        if self.at_kw(Keyword::Implies) {
            self.bump();
            let rhs = self.implies()?;
            return Ok(Self::binary(BinaryOp::Implies, lhs, rhs));
        }
        Ok(lhs)
    }

    fn range(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.or()?;
        if self.eat_punct(Punct::DotDot) {
            let end = self.or()?;
            return Ok(Expr {
                span: join(start.span, end.span),
                kind: ExprKind::Range {
                    start: Box::new(start),
                    end: Box::new(end),
                },
            });
        }
        Ok(start)
    }

    fn or(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.and()?;
        while self.eat_punct(Punct::OrOr) {
            let rhs = self.and()?;
            lhs = Self::binary(BinaryOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.comparison()?;
        while self.eat_punct(Punct::AndAnd) {
            let rhs = self.comparison()?;
            lhs = Self::binary(BinaryOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn comparison(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.additive()?;
        let Some(op) = comparison_op(self.peek()) else {
            return Ok(lhs);
        };
        self.bump();
        let rhs = self.additive()?;
        if comparison_op(self.peek()).is_some() {
            return Err(error(
                "E0105",
                "comparisons cannot be chained; combine them with `&&`, \
                 e.g. `a < b && b < c`",
                join(lhs.span, self.peek_span()),
            ));
        }
        Ok(Self::binary(op, lhs, rhs))
    }

    fn additive(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(TokenKind::Punct(Punct::Plus)) => BinaryOp::Add,
                Some(TokenKind::Punct(Punct::Minus)) => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.multiplicative()?;
            lhs = Self::binary(op, lhs, rhs);
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(TokenKind::Punct(Punct::Star)) => BinaryOp::Mul,
                Some(TokenKind::Punct(Punct::Slash)) => BinaryOp::Div,
                Some(TokenKind::Punct(Punct::Percent)) => BinaryOp::Rem,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Self::binary(op, lhs, rhs);
        }
    }

    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        let op = match self.peek() {
            Some(TokenKind::Punct(Punct::Minus)) => UnaryOp::Neg,
            Some(TokenKind::Punct(Punct::Bang)) => UnaryOp::Not,
            _ => return self.postfix(),
        };
        let start = self.peek_span();
        self.bump();
        let operand = self.unary()?;
        Ok(Expr {
            span: join(start, operand.span),
            kind: ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
        })
    }

    fn postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.primary()?;
        loop {
            let start = expr.span;
            if self.at_punct(Punct::LParen) {
                let args = self.args()?;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span: self.span_from(start),
                };
            } else if self.eat_punct(Punct::Dot) {
                let name = self.expect_ident("a name after `.`")?;
                expr = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expr),
                        name,
                    },
                    span: self.span_from(start),
                };
            } else if self.eat_punct(Punct::Question) {
                expr = Expr {
                    kind: ExprKind::Try(Box::new(expr)),
                    span: self.span_from(start),
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn args(&mut self) -> Result<Vec<Arg>, Diagnostic> {
        self.expect_punct(Punct::LParen)?;
        let mut args = Vec::new();
        while !self.at_punct(Punct::RParen) {
            let label = match (self.peek(), self.peek_at(1)) {
                (Some(TokenKind::Ident(_)), Some(TokenKind::Punct(Punct::Colon))) => {
                    let label = self.expect_ident("an argument label")?;
                    self.bump();
                    Some(label)
                }
                _ => None,
            };
            let value = self.expr()?;
            args.push(Arg { label, value });
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.peek_span();
        let kind = match self.peek().cloned() {
            Some(TokenKind::Int(digits)) => {
                self.bump();
                ExprKind::Int(digits)
            }
            Some(TokenKind::Str(_)) => {
                self.bump();
                ExprKind::Str(self.str_parts(start)?)
            }
            Some(TokenKind::SinkStr { tag, value }) => {
                self.bump();
                ExprKind::SinkStr { tag, value }
            }
            Some(TokenKind::Keyword(Keyword::True)) => {
                self.bump();
                ExprKind::Bool(true)
            }
            Some(TokenKind::Keyword(Keyword::False)) => {
                self.bump();
                ExprKind::Bool(false)
            }
            Some(TokenKind::Keyword(Keyword::SelfValue)) => {
                self.bump();
                ExprKind::SelfValue
            }
            Some(TokenKind::Ident(name)) => {
                self.bump();
                ExprKind::Name(Ident { name, span: start })
            }
            Some(TokenKind::Punct(Punct::LParen)) => {
                self.bump();
                if self.eat_punct(Punct::RParen) {
                    ExprKind::Unit
                } else {
                    let inner = self.expr()?;
                    self.expect_punct(Punct::RParen)?;
                    // Parentheses only group; keep the inner node but widen
                    // its span to include them.
                    return Ok(Expr {
                        kind: inner.kind,
                        span: self.span_from(start),
                    });
                }
            }
            Some(TokenKind::Punct(Punct::LBracket)) => {
                self.bump();
                let mut items = Vec::new();
                while !self.at_punct(Punct::RBracket) {
                    items.push(self.expr()?);
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBracket)?;
                ExprKind::List(items)
            }
            Some(TokenKind::Punct(Punct::LBrace)) => ExprKind::Block(self.block()?),
            Some(TokenKind::Punct(Punct::Pipe | Punct::OrOr)) => self.closure()?,
            Some(TokenKind::Keyword(Keyword::If)) => self.if_expr()?,
            Some(TokenKind::Keyword(Keyword::Scope)) => {
                self.bump();
                self.expect_punct(Punct::Pipe)?;
                let handle = self.expect_ident("a scope handle name")?;
                self.expect_punct(Punct::Pipe)?;
                let body = self.block()?;
                ExprKind::Scope { handle, body }
            }
            other => {
                return Err(error(
                    "E0100",
                    format!("expected an expression, found {}", describe(other.as_ref())),
                    start,
                ));
            }
        };
        Ok(Expr {
            kind,
            span: self.span_from(start),
        })
    }

    fn closure(&mut self) -> Result<ExprKind, Diagnostic> {
        let mut params = Vec::new();
        if !self.eat_punct(Punct::OrOr) {
            self.expect_punct(Punct::Pipe)?;
            while !self.at_punct(Punct::Pipe) {
                let name = self.expect_ident("a closure parameter")?;
                let ty = if self.eat_punct(Punct::Colon) {
                    Some(self.ty()?)
                } else {
                    None
                };
                params.push(ClosureParam { name, ty });
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::Pipe)?;
        }
        let body = self.expr()?;
        Ok(ExprKind::Closure {
            params,
            body: Box::new(body),
        })
    }

    fn if_expr(&mut self) -> Result<ExprKind, Diagnostic> {
        self.expect_kw(Keyword::If)?;
        let cond = self.expr()?;
        let then_block = self.block()?;
        let else_branch = if self.at_kw(Keyword::Else) {
            self.bump();
            let start = self.peek_span();
            let kind = if self.at_kw(Keyword::If) {
                self.if_expr()?
            } else {
                ExprKind::Block(self.block()?)
            };
            Some(Box::new(Expr {
                kind,
                span: self.span_from(start),
            }))
        } else {
            None
        };
        Ok(ExprKind::If {
            cond: Box::new(cond),
            then_block,
            else_branch,
        })
    }

    /// Split a string literal into text and interpolation holes. The token
    /// value has escapes resolved, which loses the difference between `\{`
    /// and `{`, so this re-reads the raw source.
    fn str_parts(&self, span: Span) -> Result<Vec<StrPart>, Diagnostic> {
        let base = span.start + 1;
        let raw = &self.src[base..span.end - 1];
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut chars = raw.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some(resolved) = chars.next().and_then(|(_, e)| resolve_escape(e)) {
                        text.push(resolved);
                    }
                }
                '{' => {
                    let hole_start = base + i;
                    let Some(len) = raw[i + 1..].find('}') else {
                        return Err(error(
                            "E0102",
                            "unclosed `{` in string; write `\\{` for a literal brace",
                            Span {
                                start: hole_start,
                                end: span.end - 1,
                            },
                        ));
                    };
                    let inner = &raw[i + 1..i + 1 + len];
                    let hole_span = Span {
                        start: hole_start,
                        end: hole_start + len + 2,
                    };
                    let path = hole_path(inner, hole_start + 1).ok_or_else(|| {
                        error(
                            "E0102",
                            format!(
                                "string interpolation only accepts a name or field path \
                                 such as `{{user.name}}`, found `{{{inner}}}`; \
                                 compute the value with `let` first"
                            ),
                            hole_span,
                        )
                    })?;
                    if !text.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut text)));
                    }
                    parts.push(StrPart::Hole {
                        path,
                        span: hole_span,
                    });
                    // Skip the hole's contents and closing brace.
                    for _ in 0..=inner.chars().count() {
                        chars.next();
                    }
                }
                '}' => {
                    return Err(error(
                        "E0102",
                        "unmatched `}` in string; write `\\}` for a literal brace",
                        Span {
                            start: base + i,
                            end: base + i + 1,
                        },
                    ));
                }
                c => text.push(c),
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(StrPart::Text(text));
        }
        Ok(parts)
    }

    // ----- types -----

    fn ty(&mut self) -> Result<Type, Diagnostic> {
        let start = self.peek_span();
        let kind = match self.peek() {
            Some(TokenKind::Punct(Punct::LParen)) => {
                self.bump();
                self.expect_punct(Punct::RParen)?;
                TypeKind::Unit
            }
            Some(TokenKind::Keyword(Keyword::Cap | Keyword::Fn)) => {
                let cap = self.at_kw(Keyword::Cap);
                if cap {
                    self.bump();
                }
                self.expect_kw(Keyword::Fn)?;
                self.expect_punct(Punct::LParen)?;
                let mut params = Vec::new();
                while !self.at_punct(Punct::RParen) {
                    params.push(self.ty()?);
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RParen)?;
                let ret = if self.eat_punct(Punct::Arrow) {
                    Some(Box::new(self.ty()?))
                } else {
                    None
                };
                TypeKind::Fn { cap, params, ret }
            }
            Some(TokenKind::Ident(_)) => {
                let mut segments = vec![self.expect_ident("a type name")?];
                while self.eat_punct(Punct::Dot) {
                    segments.push(self.expect_ident("a type name after `.`")?);
                }
                let mut args = Vec::new();
                if self.eat_punct(Punct::Lt) {
                    loop {
                        args.push(self.ty()?);
                        if !self.eat_punct(Punct::Comma) || self.at_punct(Punct::Gt) {
                            break;
                        }
                    }
                    self.expect_punct(Punct::Gt)?;
                }
                TypeKind::Path { segments, args }
            }
            other => {
                return Err(error(
                    "E0100",
                    format!("expected a type, found {}", describe(other)),
                    start,
                ));
            }
        };
        Ok(Type {
            kind,
            span: self.span_from(start),
        })
    }
}

/// Parse the inside of an interpolation hole: `name` or `name.field...`,
/// with no whitespace. `offset` is the byte offset of `inner` in the source.
fn hole_path(inner: &str, offset: usize) -> Option<Vec<Ident>> {
    let mut path = Vec::new();
    let mut at = offset;
    for (n, segment) in inner.split('.').enumerate() {
        let mut chars = segment.chars();
        let valid_start = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        let keyword = Keyword::lookup(segment);
        let allowed_keyword = n == 0 && keyword == Some(Keyword::SelfValue);
        if !valid_start || !valid_rest || (keyword.is_some() && !allowed_keyword) {
            return None;
        }
        path.push(Ident {
            name: segment.to_string(),
            span: Span {
                start: at,
                end: at + segment.len(),
            },
        });
        at += segment.len() + 1;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render an expression as a compact S-expression.
    fn sexp(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(d) => d.clone(),
            ExprKind::Str(parts) => {
                let inner: Vec<String> = parts
                    .iter()
                    .map(|p| match p {
                        StrPart::Text(t) => format!("{t:?}"),
                        StrPart::Hole { path, .. } => format!(
                            "{{{}}}",
                            path.iter()
                                .map(|i| i.name.as_str())
                                .collect::<Vec<_>>()
                                .join(".")
                        ),
                    })
                    .collect();
                format!("(str {})", inner.join(" "))
            }
            ExprKind::SinkStr { tag, value } => format!("({tag} {value:?})"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Unit => "()".into(),
            ExprKind::Name(i) => i.name.clone(),
            ExprKind::SelfValue => "self".into(),
            ExprKind::Field { base, name } => format!("(. {} {})", sexp(base), name.name),
            ExprKind::Call { callee, args } => {
                let mut out = format!("(call {}", sexp(callee));
                for a in args {
                    out.push(' ');
                    if let Some(l) = &a.label {
                        out.push_str(&format!("{}:", l.name));
                    }
                    out.push_str(&sexp(&a.value));
                }
                out + ")"
            }
            ExprKind::Unary { op, operand } => {
                let o = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                };
                format!("({o} {})", sexp(operand))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                format!("({} {} {})", op.text(), sexp(lhs), sexp(rhs))
            }
            ExprKind::Try(inner) => format!("(? {})", sexp(inner)),
            ExprKind::Range { start, end } => format!("(.. {} {})", sexp(start), sexp(end)),
            ExprKind::List(items) => {
                let items: Vec<String> = items.iter().map(sexp).collect();
                format!("[{}]", items.join(" "))
            }
            ExprKind::Closure { params, body } => {
                let params: Vec<String> = params
                    .iter()
                    .map(|p| match &p.ty {
                        Some(t) => format!("{}:{}", p.name.name, ty_str(t)),
                        None => p.name.name.clone(),
                    })
                    .collect();
                format!("(fn [{}] {})", params.join(" "), sexp(body))
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let mut out = format!("(if {} {}", sexp(cond), block_str(then_block));
                if let Some(e) = else_branch {
                    out.push(' ');
                    out.push_str(&sexp(e));
                }
                out + ")"
            }
            ExprKind::Block(b) => block_str(b),
            ExprKind::Scope { handle, body } => {
                format!("(scope {} {})", handle.name, block_str(body))
            }
        }
    }

    fn block_str(b: &Block) -> String {
        let stmts: Vec<String> = b.stmts.iter().map(stmt_str).collect();
        format!("{{{}}}", stmts.join("; "))
    }

    fn stmt_str(s: &Stmt) -> String {
        match &s.kind {
            StmtKind::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let kw = if *mutable { "var" } else { "let" };
                let ty = ty
                    .as_ref()
                    .map(|t| format!(":{}", ty_str(t)))
                    .unwrap_or_default();
                format!("({kw} {}{ty} {})", name.name, sexp(value))
            }
            StmtKind::Assign { target, value } => format!("(= {} {})", sexp(target), sexp(value)),
            StmtKind::Return(None) => "(return)".into(),
            StmtKind::Return(Some(e)) => format!("(return {})", sexp(e)),
            StmtKind::Break => "(break)".into(),
            StmtKind::Continue => "(continue)".into(),
            StmtKind::For {
                binding,
                iter,
                body,
            } => format!("(for {} {} {})", binding.name, sexp(iter), block_str(body)),
            StmtKind::While { cond, body } => {
                format!("(while {} {})", sexp(cond), block_str(body))
            }
            StmtKind::Expr(e) => sexp(e),
        }
    }

    fn ty_str(t: &Type) -> String {
        match &t.kind {
            TypeKind::Unit => "()".into(),
            TypeKind::Path { segments, args } => {
                let name = segments
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                if args.is_empty() {
                    name
                } else {
                    let args: Vec<String> = args.iter().map(ty_str).collect();
                    format!("{name}<{}>", args.join(","))
                }
            }
            TypeKind::Fn { cap, params, ret } => {
                let params: Vec<String> = params.iter().map(ty_str).collect();
                let ret = ret
                    .as_ref()
                    .map(|r| format!("->{}", ty_str(r)))
                    .unwrap_or_default();
                let cap = if *cap { "cap " } else { "" };
                format!("{cap}fn({}){ret}", params.join(","))
            }
        }
    }

    fn e(src: &str) -> String {
        match parse_expr(src) {
            Ok(expr) => sexp(&expr),
            Err(err) => panic!("{src:?} should parse: {err}"),
        }
    }

    fn s(src: &str) -> String {
        match parse_stmts(src) {
            Ok(stmts) => stmts.iter().map(stmt_str).collect::<Vec<_>>().join("; "),
            Err(err) => panic!("{src:?} should parse: {err}"),
        }
    }

    fn t(src: &str) -> String {
        match parse_type(src) {
            Ok(ty) => ty_str(&ty),
            Err(err) => panic!("{src:?} should parse: {err}"),
        }
    }

    fn expr_err(src: &str) -> Diagnostic {
        parse_expr(src).expect_err("should fail to parse")
    }

    fn stmts_err(src: &str) -> Diagnostic {
        parse_stmts(src).expect_err("should fail to parse")
    }

    // ----- precedence -----

    #[test]
    fn arithmetic_precedence() {
        assert_eq!(e("a + b * c"), "(+ a (* b c))");
        assert_eq!(e("a * b + c"), "(+ (* a b) c)");
        assert_eq!(e("a - b - c"), "(- (- a b) c)");
        assert_eq!(e("(a + b) * c"), "(* (+ a b) c)");
        assert_eq!(e("a % b / c"), "(/ (% a b) c)");
    }

    #[test]
    fn logical_precedence() {
        assert_eq!(e("a || b && c"), "(|| a (&& b c))");
        assert_eq!(e("a == b && c < d"), "(&& (== a b) (< c d))");
        assert_eq!(e("!a == b"), "(== (! a) b)");
    }

    #[test]
    fn implies_is_loosest_and_right_associative() {
        assert_eq!(e("a implies b implies c"), "(implies a (implies b c))");
        assert_eq!(e("a || b implies c"), "(implies (|| a b) c)");
        assert_eq!(
            e("r.is_ok() implies r.unwrap().balance >= 0"),
            "(implies (call (. r is_ok)) (>= (. (call (. r unwrap)) balance) 0))"
        );
    }

    #[test]
    fn range_binds_looser_than_arithmetic() {
        assert_eq!(e("0..n + 1"), "(.. 0 (+ n 1))");
    }

    #[test]
    fn prefix_and_postfix() {
        assert_eq!(e("-a.b()?"), "(- (? (call (. a b))))");
        assert_eq!(e("--a"), "(- (- a))");
        assert_eq!(
            e("fs.read_to_string(path)?"),
            "(? (call (. fs read_to_string) path))"
        );
    }

    #[test]
    fn rejects_chained_comparison() {
        let err = expr_err("a < b < c");
        assert_eq!(err.code, "E0105");
        assert_eq!(err.span, Span { start: 0, end: 7 });
    }

    // ----- primaries -----

    #[test]
    fn literals() {
        assert_eq!(e("1_000"), "1_000");
        assert_eq!(e("true"), "true");
        assert_eq!(e("()"), "()");
        assert_eq!(e("[1, 2, 3,]"), "[1 2 3]");
        assert_eq!(e("[]"), "[]");
        assert_eq!(e("sql\"SELECT ?\""), "(sql \"SELECT ?\")");
        assert_eq!(e("self.balance"), "(. self balance)");
    }

    #[test]
    fn calls_with_labels_and_multiline_args() {
        assert_eq!(e("copy(src: a, dst: b)"), "(call copy src:a dst:b)");
        assert_eq!(e("Point(x: 1, y: 2)"), "(call Point x:1 y:2)");
        assert_eq!(e("f(\n    a,\n    b,\n)"), "(call f a b)");
        assert_eq!(
            e("proc.run(\"git\", [\"log\", branch])"),
            "(call (. proc run) (str \"git\") [(str \"log\") branch])"
        );
    }

    #[test]
    fn closures() {
        assert_eq!(e("|x| x + 1"), "(fn [x] (+ x 1))");
        assert_eq!(e("|| work()"), "(fn [] (call work))");
        assert_eq!(e("|a: Int, b| { a }"), "(fn [a:Int b] {a})");
        assert_eq!(
            e("xs.map(|x| {\n    let y = x * 2\n    y\n})"),
            "(call (. xs map) (fn [x] {(let y (* x 2)); y}))"
        );
    }

    #[test]
    fn if_else_chains() {
        assert_eq!(
            e("if a { 1 } else if b { 2 } else { 3 }"),
            "(if a {1} (if b {2} {3}))"
        );
        assert_eq!(e("if a {\n    b\n}\nelse {\n    c\n}"), "(if a {b} {c})");
    }

    #[test]
    fn scope_blocks() {
        assert_eq!(
            e("scope |s| {\n    let h = s.spawn(|| work())\n    h.join()\n}"),
            "(scope s {(let h (call (. s spawn) (fn [] (call work)))); (call (. h join))})"
        );
    }

    // ----- strings -----

    #[test]
    fn string_interpolation() {
        assert_eq!(e("\"lines: {n}\""), "(str \"lines: \" {n})");
        assert_eq!(e("\"hi {user.name}!\""), "(str \"hi \" {user.name} \"!\")");
        assert_eq!(e("\"{self.id}\""), "(str {self.id})");
        assert_eq!(e("\"\""), "(str \"\")");
    }

    #[test]
    fn escaped_braces_are_text() {
        assert_eq!(e("\"a \\{b\\} \\n\""), "(str \"a {b} \\n\")");
    }

    #[test]
    fn hole_spans_point_into_source() {
        let expr = parse_expr("\"x {user.name}\"").expect("should parse");
        let ExprKind::Str(parts) = expr.kind else {
            panic!("expected a string")
        };
        let StrPart::Hole { path, span } = &parts[1] else {
            panic!("expected a hole")
        };
        assert_eq!(*span, Span { start: 3, end: 14 });
        assert_eq!(path[0].span, Span { start: 4, end: 8 });
        assert_eq!(path[1].span, Span { start: 9, end: 13 });
    }

    #[test]
    fn rejects_expressions_in_interpolation() {
        for (src, hole) in [
            ("\"{a + b}\"", "{a + b}"),
            ("\"x {f()}\"", "{f()}"),
            ("\"{ a }\"", "{ a }"),
            ("\"{}\"", "{}"),
            ("\"{a.}\"", "{a.}"),
            ("\"{if}\"", "{if}"),
        ] {
            let err = expr_err(src);
            assert_eq!(err.code, "E0102", "{src}");
            assert_eq!(&src[err.span.start..err.span.end], hole, "{src}");
        }
    }

    #[test]
    fn rejects_unbalanced_braces_in_strings() {
        assert_eq!(expr_err("\"a {b\"").code, "E0102");
        let err = expr_err("\"a } b\"");
        assert_eq!(err.code, "E0102");
        assert_eq!(err.span, Span { start: 3, end: 4 });
    }

    // ----- statements -----

    #[test]
    fn let_and_var() {
        assert_eq!(s("let x = 1"), "(let x 1)");
        assert_eq!(
            s("var xs: List<Int> = List.new()"),
            "(var xs:List<Int> (call (. List new)))"
        );
    }

    #[test]
    fn requires_initializer() {
        let err = stmts_err("let x: List<Int>");
        assert_eq!(err.code, "E0104");
        assert_eq!(err.span, Span { start: 0, end: 16 });
        assert_eq!(stmts_err("var count").code, "E0104");
    }

    #[test]
    fn assignment() {
        assert_eq!(s("x = 1"), "(= x 1)");
        assert_eq!(
            s("self.balance = self.balance + n"),
            "(= (. self balance) (+ (. self balance) n))"
        );
        let err = stmts_err("f() = 1");
        assert_eq!(err.code, "E0106");
        assert_eq!(err.span, Span { start: 0, end: 3 });
    }

    #[test]
    fn control_flow_statements() {
        assert_eq!(
            s("for i in 0..n {\n    if i == 3 { continue }\n    total = total + i\n}"),
            "(for i (.. 0 n) {(if (== i 3) {(continue)}); (= total (+ total i))})"
        );
        assert_eq!(s("while true {\n    break\n}"), "(while true {(break)})");
        assert_eq!(s("return"), "(return)");
        assert_eq!(s("return x?"), "(return (? x))");
        assert_eq!(s("if done {\n    return\n}"), "(if done {(return)})");
    }

    #[test]
    fn newlines_separate_statements() {
        assert_eq!(s("a()\n\n// gap\nb()\n"), "(call a); (call b)");
    }

    #[test]
    fn rejects_two_statements_on_one_line() {
        let err = stmts_err("a b");
        assert_eq!(err.code, "E0101");
        assert_eq!(err.span, Span { start: 2, end: 3 });
    }

    #[test]
    fn rejects_bare_block_statement() {
        let err = stmts_err("a()\n{ b() }");
        assert_eq!(err.code, "E0103");
        assert_eq!(err.span, Span { start: 4, end: 5 });
        assert_eq!(s("let x = { b() }"), "(let x {(call b)})");
    }

    #[test]
    fn rejects_missing_expression() {
        let err = expr_err("a +");
        assert_eq!(err.code, "E0100");
        assert_eq!(err.span, Span { start: 3, end: 3 });
        assert_eq!(expr_err("match x {}").code, "E0100");
    }

    #[test]
    fn parses_example_bodies() {
        // The bodies of `count_lines` and `main` in examples/capabilities.osk.
        assert_eq!(
            s("let text = fs.read_to_string(path)?\nOk(text.lines().count())"),
            "(let text (? (call (. fs read_to_string) path))); \
             (call Ok (call (. (call (. text lines)) count)))"
        );
        let main = r#"
    let local = fs.restrict_to(".")
    let path = args.get(1).ok_or(AppError.usage("usage: capabilities <file>"))?
    let n = count_lines(local, Path.from(path))?
    io.println("lines: {n}")
"#;
        assert_eq!(parse_stmts(main).expect("main body should parse").len(), 4);
    }

    // ----- types -----

    #[test]
    fn types() {
        assert_eq!(t("Int"), "Int");
        assert_eq!(t("Result<(), IoError>"), "Result<(),IoError>");
        assert_eq!(t("List<List<Int>>"), "List<List<Int>>");
        assert_eq!(t("std.sql.Db"), "std.sql.Db");
        assert_eq!(t("fn(Int) -> Bool"), "fn(Int)->Bool");
        assert_eq!(t("cap fn(T)"), "cap fn(T)");
        assert_eq!(parse_type("1").expect_err("should fail").code, "E0100");
    }

    #[test]
    fn spans_cover_source_text() {
        let src = "a + f(b)";
        let expr = parse_expr(src).expect("should parse");
        assert_eq!(expr.span, Span { start: 0, end: 8 });
        let ExprKind::Binary { rhs, .. } = expr.kind else {
            panic!("expected binary")
        };
        assert_eq!(&src[rhs.span.start..rhs.span.end], "f(b)");
        let stmts = parse_stmts("let x = (1)").expect("should parse");
        assert_eq!(stmts[0].span, Span { start: 0, end: 11 });
    }
}
