//! Panics: how a bug stops a program.
//!
//! A panic carries a stable `P`-code, a message, and the span of the
//! expression that panicked. It unwinds to [`crate::run`] as
//! [`crate::interp::Unwind::Panic`], which reports it as a diagnostic and
//! exits with code 101. `docs/runtime.md` documents every code; a test
//! keeps the two in step.
//!
//! To add a code: add a constant to [`codes`], list it in [`codes::ALL`],
//! and add its row to `docs/runtime.md`.

use oska_syntax::{Diagnostic, Span};

/// The stable codes the runtime reports: refusals (`E038x`) and panics
/// (`P0001` upwards).
pub mod codes {
    /// `main` declares a capability that has no runtime yet, such as
    /// `Net`. The program is refused before it starts.
    pub const UNIMPLEMENTED_CAPABILITY: &str = "E0370";
    /// The program imports a `std` module that has no runtime yet, such
    /// as `std.sql`. It is refused before it starts.
    pub const UNIMPLEMENTED_MODULE: &str = "E0371";
    /// The program uses something the interpreter cannot run yet. It is
    /// refused before it starts.
    pub const UNSUPPORTED: &str = "E0380";

    /// Integer arithmetic overflowed its type.
    pub const OVERFLOW: &str = "P0001";
    /// An index was out of bounds.
    pub const INDEX_OUT_OF_BOUNDS: &str = "P0002";
    /// `todo()` was reached.
    pub const TODO: &str = "P0003";
    /// An integer was divided by zero, or its remainder taken.
    pub const DIVISION_BY_ZERO: &str = "P0004";
    /// `Env.secret` asked for a variable the environment does not set.
    pub const MISSING_SECRET: &str = "P0020";

    /// Every code, in the order `docs/runtime.md` lists them.
    pub const ALL: &[&str] = &[
        UNIMPLEMENTED_CAPABILITY,
        UNIMPLEMENTED_MODULE,
        UNSUPPORTED,
        OVERFLOW,
        INDEX_OUT_OF_BOUNDS,
        TODO,
        DIVISION_BY_ZERO,
        MISSING_SECRET,
    ];
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
