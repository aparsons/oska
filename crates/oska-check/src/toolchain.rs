//! `std.rt` is the toolchain's own (ADR 0013). Its source is embedded here
//! and is the only text that may import `std.rt` or name an intrinsic: a
//! raw system call is ambient authority. Everything else, user code and
//! package modules alike, gets `E0209`.
//!
//! An intrinsic is any identifier starting with [`SYS_PREFIX`] or
//! [`MEM_PREFIX`]. The whole prefix is reserved, whether or not the name
//! is declared, so the rule needs no table and the shell guard
//! (`scripts/intrinsic-guard/guard.sh`) can match it with one pattern.

use oska_syntax::{Diagnostic, Keyword, Punct, Span, Token, TokenKind};

use crate::codes;

/// `std.rt`'s source.
pub const RT_SOURCE: &str = include_str!("std/rt.osk");

/// Names of one system call each: `rt_sys_openat`.
pub const SYS_PREFIX: &str = "rt_sys_";
/// Names of memory and word operations: `rt_mem_load64`.
pub const MEM_PREFIX: &str = "rt_mem_";

/// Whether `name` is reserved for an intrinsic.
pub fn is_intrinsic(name: &str) -> bool {
    name.starts_with(SYS_PREFIX) || name.starts_with(MEM_PREFIX)
}

/// Report `use std.rt` and every intrinsic name in `src`, which is not
/// the toolchain's own `std.rt`. Comments and text are not names.
pub(crate) fn module(src: &str, diagnostics: &mut Vec<Diagnostic>) {
    // A parse that succeeded lexes; a failure is reported elsewhere.
    let Ok(tokens) = oska_syntax::lex(src) else {
        return;
    };
    let mut i = 0;
    while i < tokens.len() {
        if let Some(span) = use_std_rt(&tokens, i) {
            diagnostics.push(Diagnostic::new(
                codes::TOOLCHAIN_ONLY,
                "`std.rt` is the toolchain's own runtime module and cannot be imported: \
                 raw system calls are ambient authority, so a program reaches the \
                 operating system only through the capabilities `main` receives"
                    .to_string(),
                span,
            ));
        }
        if let TokenKind::Ident(name) = &tokens[i].kind {
            if is_intrinsic(name) {
                diagnostics.push(Diagnostic::new(
                    codes::TOOLCHAIN_ONLY,
                    format!(
                        "`{name}` is a system-call or memory intrinsic, and names starting \
                         `{SYS_PREFIX}` or `{MEM_PREFIX}` are reserved for the toolchain's \
                         own `std.rt`"
                    ),
                    tokens[i].span,
                ));
            }
        }
        i += 1;
    }
}

/// The span of the path of a `use std.rt` starting at token `i`, with a
/// brace group if there is one.
fn use_std_rt(tokens: &[Token], i: usize) -> Option<Span> {
    let kind = |n: usize| tokens.get(i + n).map(|t| &t.kind);
    let is_ident = |n: usize, want: &str| matches!(kind(n), Some(TokenKind::Ident(s)) if s == want);
    if !(matches!(kind(0), Some(TokenKind::Keyword(Keyword::Use)))
        && is_ident(1, "std")
        && matches!(kind(2), Some(TokenKind::Punct(Punct::Dot)))
        && is_ident(3, "rt"))
    {
        return None;
    }
    let mut end = tokens[i + 3].span.end;
    // `use std.rt.{a, b}`: the path runs to the closing brace.
    if matches!(kind(4), Some(TokenKind::Punct(Punct::Dot)))
        && matches!(kind(5), Some(TokenKind::Punct(Punct::LBrace)))
    {
        let close = tokens[i + 5..]
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Punct(Punct::RBrace)));
        if let Some(close) = close {
            end = close.span.end;
        }
    }
    Some(Span {
        start: tokens[i + 1].span.start,
        end,
    })
}

/// Every intrinsic `std.rt` declares, in source order.
pub fn declared_intrinsics() -> Vec<&'static str> {
    let Ok(module) = oska_syntax::parse_prelude(RT_SOURCE) else {
        return Vec::new();
    };
    module
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            oska_syntax::ast::ItemKind::Fn(f) => Some(f.name.span),
            _ => None,
        })
        .filter_map(|span| RT_SOURCE.get(span.start..span.end))
        .collect()
}
