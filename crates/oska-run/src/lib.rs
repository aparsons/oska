//! Interpreter for the Oska programming language.
//!
//! [`run`] checks a source file, refuses it if the checker reports any
//! diagnostic, and otherwise runs the **checked program** the checker
//! produced (ADR 0007): `main` gets exactly the capabilities it declares.
//! Everything the program does to the outside world goes through a
//! [`Host`]: [`RealHost`] for `oskac run`, [`FakeHost`] for tests.
//!
//! - `support.rs` refuses, before it starts, a program that uses
//!   something the interpreter cannot run yet (`E0380`).
//! - `interp.rs` evaluates the checked program.
//! - `ops.rs` implements operators, with checked integer arithmetic.
//! - `intrinsics.rs` implements body-less prelude functions, and
//!   `data.rs` those of `List` and `String`.
//! - `panics.rs` defines panics and every runtime code.
//! - `value.rs` defines runtime values.
//! - `host.rs` defines the hosts.
//!
//! # Exit codes
//!
//! - `0`: `main` returned `Ok` or `()`.
//! - `1`: the checker refused the program, or `main` returned `Err`.
//! - `2`: the program uses something the interpreter cannot run yet, so
//!   it was refused before it started (`E0380`).
//! - `101`: the program panicked.

use oska_check::program::Program;
use oska_syntax::Diagnostic;

mod data;
mod host;
mod interp;
mod intrinsics;
mod ops;
mod panics;
mod support;
mod value;

pub use host::{FakeHost, Host, Output, RealHost};
pub use panics::codes;

use interp::Interpreter;
use panics::Panic;
use value::{Capability, Value};

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// What the program wrote to stdout, as the host recorded it: all of
    /// it on [`FakeHost`], nothing on [`RealHost`], which writes straight
    /// to the terminal.
    pub stdout: String,
    /// What the program and the runtime wrote to stderr, as the host
    /// recorded it.
    pub stderr: String,
    /// The process exit code.
    pub exit_code: i32,
    /// The panic that stopped the program, shaped like a diagnostic with
    /// a `P`-code.
    pub panic: Option<Diagnostic>,
    /// The diagnostics that refused the program before it started. When
    /// there are any, nothing ran.
    pub diagnostics: Vec<Diagnostic>,
}

impl RunOutcome {
    fn new(exit_code: i32, host: &mut dyn Host) -> Self {
        let output = host.take_output();
        RunOutcome {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code,
            panic: None,
            diagnostics: Vec::new(),
        }
    }
}

/// The exit code of a program that panicked: it has a bug.
pub const PANIC_EXIT_CODE: i32 = 101;

/// The exit code of a program the interpreter cannot run yet.
pub const UNSUPPORTED_EXIT_CODE: i32 = 2;

/// Check and run `src` on `host`. `args` are the program's arguments,
/// the program path first.
pub fn run(src: &str, host: &mut dyn Host, args: &[String]) -> RunOutcome {
    let _ = args;
    let report = oska_check::check_module(src);
    let Some(program) = report.program else {
        let mut outcome = RunOutcome::new(1, host);
        outcome.diagnostics = report.diagnostics;
        return outcome;
    };
    let refusal = support::unsupported(&program).or_else(|| ungranted(&program));
    if let Some(refusal) = refusal {
        let mut outcome = RunOutcome::new(UNSUPPORTED_EXIT_CODE, host);
        outcome.diagnostics.push(refusal);
        return outcome;
    }
    match run_main(&program, host) {
        Ok(value) => RunOutcome::new(main_exit_code(&value), host),
        Err(panic) => {
            let mut outcome = RunOutcome::new(PANIC_EXIT_CODE, host);
            outcome.panic = Some(panic.into_diagnostic());
            outcome
        }
    }
}

/// The refusal for a capability `main` declares that has no runtime yet.
fn ungranted(program: &Program) -> Option<Diagnostic> {
    let main = program.functions.get("main")?;
    let param = main
        .params
        .iter()
        .find(|p| Capability::granted(&p.ty).is_none())?;
    Some(Diagnostic::new(
        codes::UNSUPPORTED,
        format!(
            "the interpreter cannot run a program given `{}` yet",
            param.ty
        ),
        main.body.span,
    ))
}

/// Run `main`, granting each parameter the capability its type names.
fn run_main(program: &Program, host: &mut dyn Host) -> Result<Value, Panic> {
    let Some(main) = program.functions.get("main") else {
        return Ok(Value::Unit);
    };
    let args = main
        .params
        .iter()
        .filter_map(|param| Capability::granted(&param.ty))
        .map(Value::Capability)
        .collect();
    Interpreter::new(program, host).call(main, args)
}

/// `main`'s result as an exit code: `Err` is an expected failure.
fn main_exit_code(value: &Value) -> i32 {
    match value {
        Value::Variant { ty, variant, .. } if &**ty == "Result" && &**variant == "Err" => 1,
        _ => 0,
    }
}
