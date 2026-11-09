//! Panics: how a bug stops a program.
//!
//! A panic carries a stable `P`-code, a message, and the span of the
//! expression that panicked. It unwinds to [`crate::run`] as
//! [`crate::interp::Unwind::Panic`], which reports it as a diagnostic and
//! exits with code 101. `docs/runtime.md` documents every code; a test
//! keeps the two in step.
//!
//! To add a code: add a constant here, list it in [`codes::ALL`], and add
//! its row to `docs/runtime.md`.

use oska_syntax::{Diagnostic, Span};

/// The stable codes, `P0001` upwards.
pub mod codes {
    /// Integer arithmetic overflowed its type.
    pub const OVERFLOW: &str = "P0001";
    /// `todo()` was reached.
    pub const TODO: &str = "P0003";
    /// An integer was divided by zero, or its remainder taken.
    pub const DIVISION_BY_ZERO: &str = "P0004";

    /// Every panic code, in order.
    pub const ALL: &[&str] = &[OVERFLOW, TODO, DIVISION_BY_ZERO];
}

/// A panic: the program has a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl Panic {
    pub fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Panic {
            code,
            message: message.into(),
            span,
        }
    }

    /// The panic in the shape of a diagnostic, as `--format=json` prints
    /// it.
    pub fn into_diagnostic(self) -> Diagnostic {
        Diagnostic::new(self.code, self.message, self.span)
    }
}
