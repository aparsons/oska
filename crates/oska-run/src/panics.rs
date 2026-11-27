// SPDX-License-Identifier: Apache-2.0
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

use oska_syntax::{Diagnostic, Frames, Span};

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
    /// `spawn` was called on a scope handle whose `scope` block has ended.
    pub const SPAWN_ON_FINISHED_SCOPE: &str = "P0005";
    /// A `requires` clause of the function called was false: the caller
    /// broke the contract.
    pub const PRECONDITION_VIOLATED: &str = "P0006";
    /// An `ensures` clause of the function that returned was false: the
    /// function broke its promise.
    pub const POSTCONDITION_VIOLATED: &str = "P0007";
    /// A shift or rotation amount was below 0, or not below the width of
    /// the integer type shifted.
    pub const SHIFT_OUT_OF_RANGE: &str = "P0008";
    /// `t.assert(cond)` was given `false`: the test fails.
    pub const ASSERT_FAILED: &str = "P0010";
    /// `t.assert_eq(actual, expected)` was given two values that differ.
    pub const ASSERT_EQ_FAILED: &str = "P0011";
    /// `t.assert_ne(actual, other)` was given two equal values.
    pub const ASSERT_NE_FAILED: &str = "P0012";
    /// `t.fail(message)` was called.
    pub const TEST_FAILED: &str = "P0013";
    /// `t.assert_snapshot(value)` found no stored snapshot, or one that
    /// differs from the value, or ran other than once, directly in its
    /// test's body. The test runner reports it, after the test has run.
    pub const SNAPSHOT_FAILED: &str = "P0014";
    /// `Env.secret` asked for a variable the environment does not set.
    pub const MISSING_SECRET: &str = "P0020";
    /// Calls nested deeper than the interpreter allows: runaway recursion.
    pub const CALL_DEPTH_EXCEEDED: &str = "P0021";
    /// `String.slice` was given indices out of bounds, or a start past its
    /// end.
    pub const SLICE_OUT_OF_BOUNDS: &str = "P0035";
    /// `String.repeat` was given a negative count.
    pub const NEGATIVE_REPEAT: &str = "P0036";
    /// `String.repeat` would build a text longer than the interpreter
    /// allows.
    pub const TEXT_TOO_LONG: &str = "P0037";
    /// A `Bytes` slice or endian read that does not lie wholly inside the
    /// buffer, or a negative length for `Bytes.zeroed`.
    pub const BYTES_OUT_OF_RANGE: &str = "P0042";
    /// A `Bytes` would grow longer than the runtime allows (256 MiB).
    pub const BYTES_TOO_LONG: &str = "P0043";
    /// `Float.to_int` of a NaN, an infinity or a value `Int` cannot hold.
    pub const FLOAT_TO_INT: &str = "P0046";
    /// A NaN reached a function that needs a number: an operation with no
    /// answer for NaN (ADR 0027).
    pub const NAN_OPERAND: &str = "P0047";
    /// A statistic was asked of an empty list: a mean, a minimum or a
    /// maximum has no value for no input.
    pub const EMPTY_INPUT: &str = "P0048";
    /// `Int.to_float` of a value above 2^53 in magnitude, which it would
    /// round.
    pub const INT_TO_FLOAT: &str = "P0049";
    /// `Float.fmt` with a number of digits outside 0 to 340.
    pub const FMT_DIGITS: &str = "P0050";
    /// The program would have taken more than `--max-steps` steps
    /// (`oska_check::limits`).
    pub const STEP_LIMIT: &str = oska_check::limits::codes::STEP_LIMIT;
    /// The program ran for longer than `--timeout` milliseconds
    /// (`oska_check::limits`).
    pub const TIME_LIMIT: &str = oska_check::limits::codes::TIME_LIMIT;
    /// The program's values took more than `--max-memory` bytes
    /// (`oska_check::limits`).
    pub const MEMORY_LIMIT: &str = oska_check::limits::codes::MEMORY_LIMIT;
    /// The program would have written more than `--max-output` bytes to
    /// stdout and stderr together (`oska_check::limits`).
    pub const OUTPUT_LIMIT: &str = oska_check::limits::codes::OUTPUT_LIMIT;

    /// Every code, in the order `docs/runtime.md` lists them.
    pub const ALL: &[&str] = &[
        UNIMPLEMENTED_CAPABILITY,
        UNIMPLEMENTED_MODULE,
        UNSUPPORTED,
        OVERFLOW,
        INDEX_OUT_OF_BOUNDS,
        TODO,
        DIVISION_BY_ZERO,
        SPAWN_ON_FINISHED_SCOPE,
        PRECONDITION_VIOLATED,
        POSTCONDITION_VIOLATED,
        SHIFT_OUT_OF_RANGE,
        ASSERT_FAILED,
        ASSERT_EQ_FAILED,
        ASSERT_NE_FAILED,
        TEST_FAILED,
        SNAPSHOT_FAILED,
        MISSING_SECRET,
        CALL_DEPTH_EXCEEDED,
        SLICE_OUT_OF_BOUNDS,
        NEGATIVE_REPEAT,
        TEXT_TOO_LONG,
        STEP_LIMIT,
        TIME_LIMIT,
        MEMORY_LIMIT,
        OUTPUT_LIMIT,
        BYTES_OUT_OF_RANGE,
        BYTES_TOO_LONG,
        FLOAT_TO_INT,
        NAN_OPERAND,
        EMPTY_INPUT,
        INT_TO_FLOAT,
        FMT_DIGITS,
    ];
}

/// A panic: the program has a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    /// The call stack where it happened: filled in by the interpreter as the panic
    /// leaves the call it happened in (`Interpreter::stamped`).
    pub frames: Option<Frames>,
}

impl Panic {
    pub fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Panic {
            code,
            message: message.into(),
            span,
            frames: None,
        }
    }

    /// The panic in the shape of a diagnostic, as `--format=json` prints
    /// it, with its call stack.
    pub fn into_diagnostic(self) -> Diagnostic {
        let diagnostic = Diagnostic::new(self.code, self.message, self.span);
        match self.frames {
            Some(frames) => diagnostic.with_frames(frames),
            None => diagnostic,
        }
    }
}
