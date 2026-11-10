//! Interpreter for the Oska programming language.
//!
//! [`run`] checks a source file, refuses it if the checker reports any
//! diagnostic, and otherwise runs the **checked program** the checker
//! produced (ADR 0007): `main` gets exactly the capabilities it declares.
//! Everything the program does to the outside world goes through a
//! [`Host`]: [`RealHost`] for `oskac run`, [`FakeHost`] for tests.
//!
//! - `support.rs` refuses, before it starts, a program that uses
//!   something the interpreter cannot run yet (`E0380`). [`run`] also
//!   refuses a capability (`E0370`) or `std` module (`E0371`) that has no
//!   runtime yet.
//! - `confine.rs` keeps a restricted filesystem inside its root.
//! - `prng.rs` is the generator behind `Random`.
//! - `interp.rs` evaluates the checked program.
//! - `ops.rs` implements operators, with checked integer arithmetic.
//! - `intrinsics.rs` implements body-less prelude functions, and
//!   `data.rs` those of `List` and `String`.
//! - `tasks.rs` runs `scope` blocks and their spawned tasks.
//! - `panics.rs` defines panics and every runtime code.
//! - `value.rs` defines runtime values.
//! - `host.rs` defines the hosts.
//!
//! # Exit codes
//!
//! - `0`: `main` returned `Ok` or `()`.
//! - `1`: the checker refused the program, or `main` returned `Err`.
//! - `2`: the program uses something the interpreter cannot run yet, so
//!   it was refused before it started (`E0370`, `E0371`, `E0380`).
//! - `101`: the program panicked.

use std::rc::Rc;

use oska_check::program::Program;
use oska_syntax::Diagnostic;

mod confine;
mod data;
mod host;
mod interp;
mod intrinsics;
mod ops;
mod panics;
mod prng;
mod support;
mod tasks;
mod value;

pub use host::{FakeHost, Host, Output, RealHost, FAKE_SEED};
pub use panics::codes;

use interp::Interpreter;
use panics::Panic;
use value::{Capability, Value, UNIMPLEMENTED_CAPABILITIES, UNIMPLEMENTED_MODULES};

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
    let report = oska_check::check_module(src);
    let Some(program) = report.program else {
        let mut outcome = RunOutcome::new(1, host);
        outcome.diagnostics = report.diagnostics;
        return outcome;
    };
    let refusal = unimplemented_module(&program)
        .or_else(|| ungranted(&program))
        .or_else(|| support::unsupported(&program));
    if let Some(refusal) = refusal {
        let mut outcome = RunOutcome::new(UNSUPPORTED_EXIT_CODE, host);
        outcome.diagnostics.push(refusal);
        return outcome;
    }
    match run_main(&program, host, args) {
        Ok(value) => RunOutcome::new(main_exit_code(&value), host),
        Err(panic) => {
            let mut outcome = RunOutcome::new(PANIC_EXIT_CODE, host);
            outcome.panic = Some(panic.into_diagnostic());
            outcome
        }
    }
}

/// The refusal for the first import of a `std` module that has no runtime
/// yet, whether or not the program calls it.
fn unimplemented_module(program: &Program) -> Option<Diagnostic> {
    let (module, span) = program.imports.iter().find(|(module, _)| {
        UNIMPLEMENTED_MODULES
            .iter()
            .any(|m| module == m || module.strip_prefix(m).is_some_and(|r| r.starts_with('.')))
    })?;
    Some(Diagnostic::new(
        codes::UNIMPLEMENTED_MODULE,
        format!("`oskac run` cannot run a program that uses `{module}` yet: it has no runtime"),
        *span,
    ))
}

/// The refusal for a parameter of `main` that the runtime cannot grant:
/// `E0370` for a capability with no runtime yet, `E0380` for anything
/// else.
fn ungranted(program: &Program) -> Option<Diagnostic> {
    let main = program.functions.get("main")?;
    let param = main
        .params
        .iter()
        .find(|p| p.ty != "Args" && Capability::granted(&p.ty).is_none())?;
    let (code, message) = if UNIMPLEMENTED_CAPABILITIES.contains(&param.ty.as_str()) {
        (
            codes::UNIMPLEMENTED_CAPABILITY,
            format!(
                "`oskac run` cannot grant `{}` yet: the capability has no runtime",
                param.ty
            ),
        )
    } else {
        (
            codes::UNSUPPORTED,
            format!(
                "the interpreter cannot run a program given `{}` yet",
                param.ty
            ),
        )
    };
    Some(Diagnostic::new(code, message, main.body.span))
}

/// Run `main`, granting each parameter the capability its type names, and
/// `args` to an `Args` parameter.
fn run_main(program: &Program, host: &mut dyn Host, args: &[String]) -> Result<Value, Panic> {
    let Some(main) = program.functions.get("main") else {
        return Ok(Value::Unit);
    };
    let values = main
        .params
        .iter()
        .filter_map(|param| match param.ty.as_str() {
            "Args" => Some(Value::Args(
                args.iter().map(|a| Rc::from(a.as_str())).collect(),
            )),
            ty => Capability::granted(ty).map(Value::Capability),
        })
        .collect();
    Interpreter::new(program, host).call(main, values)
}

/// `main`'s result as an exit code: `Err` is an expected failure.
fn main_exit_code(value: &Value) -> i32 {
    match value {
        Value::Variant { ty, variant, .. } if &**ty == "Result" && &**variant == "Err" => 1,
        _ => 0,
    }
}
