//! Interpreter for the Oska programming language.
//!
//! [`run`] checks a source file, refuses it if the checker reports any
//! diagnostic, and otherwise runs the **checked program** the checker
//! produced (ADR 0007): `main` gets exactly the capabilities it declares.
//! Everything the program does to the outside world goes through a
//! [`Host`]: [`RealHost`] for `oskac run`, [`FakeHost`] for tests.
//!
//! - `interp.rs` evaluates the checked program.
//! - `intrinsics.rs` implements body-less prelude functions.
//! - `value.rs` defines runtime values.
//! - `host.rs` defines the hosts.
//!
//! # Exit codes
//!
//! - `0`: `main` returned `Ok` or `()`.
//! - `1`: the program was refused, or `main` returned `Err`.
//! - `2`: the program reached something the interpreter does not
//!   support yet.

use oska_check::program::Program;
use oska_syntax::Diagnostic;

mod host;
mod interp;
mod intrinsics;
mod panics;
mod value;

pub use host::{FakeHost, Host, Output, RealHost};
pub use panics::codes;

use interp::{Interpreter, Unwind};
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

/// The exit code of a program that panicked.
pub const PANIC_EXIT_CODE: i32 = 101;

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
    let exit_code = match run_main(&program, host) {
        Ok(value) => main_exit_code(&value),
        Err(Unwind::Unsupported(span)) => {
            let message = format!(
                "error: the interpreter does not support this yet (bytes {}..{})\n",
                span.start, span.end
            );
            let mut outcome = RunOutcome::new(2, host);
            outcome.stderr.push_str(&message);
            return outcome;
        }
        Err(Unwind::Panic(panic)) => {
            let mut outcome = RunOutcome::new(PANIC_EXIT_CODE, host);
            outcome.panic = Some(panic.into_diagnostic());
            return outcome;
        }
    };
    RunOutcome::new(exit_code, host)
}

/// Run `main`, granting each parameter the capability its type names.
fn run_main(program: &Program, host: &mut dyn Host) -> Result<Value, Unwind> {
    let Some(main) = program.functions.get("main") else {
        return Ok(Value::Unit);
    };
    let mut args = Vec::with_capacity(main.params.len());
    for param in &main.params {
        match Capability::granted(&param.ty) {
            Some(capability) => args.push(Value::Capability(capability)),
            None => return Err(Unwind::Unsupported(main.body.span)),
        }
    }
    Interpreter::new(program, host).call(main, args)
}

/// `main`'s result as an exit code: `Err` is an expected failure.
fn main_exit_code(value: &Value) -> i32 {
    match value {
        Value::Variant { ty, variant, .. } if &**ty == "Result" && &**variant == "Err" => 1,
        _ => 0,
    }
}
