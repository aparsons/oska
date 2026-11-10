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
//! - `intrinsics.rs` implements body-less prelude functions,
//!   `data.rs` those of `List` and `String`, `traits.rs` the
//!   prelude's trait impls, such as `Eq for Int`, and `html.rs` the
//!   `std.html` module.
//! - `tasks.rs` runs `scope` blocks and their spawned tasks.
//! - `testing.rs` runs `test` blocks for [`test`] (`oskac test`), and
//!   implements `TestContext`'s methods.
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
mod html;
mod interp;
mod intrinsics;
mod ops;
mod panics;
mod prng;
mod support;
mod tasks;
mod testing;
mod traits;
mod value;

pub use host::{FakeHost, Host, Output, RealHost, FAKE_SEED};
pub use panics::codes;
pub use testing::{test, TestOptions, TestReport, TestResult, TestStatus};

use interp::Interpreter;
use oska_check::builtins;
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

/// The stack of the thread an interpreter runs on. Only what is used is
/// ever touched; the rest is address space. Calls stop with `P0021` at
/// [`interp::MAX_CALL_DEPTH`], or sooner when a program's calls take so
/// much stack each that it would run out first.
pub const STACK_SIZE: usize = 256 * 1024 * 1024;

/// `f`'s result, computed on a thread of its own with a [`STACK_SIZE`]
/// stack, or on this thread when the OS will not start one. A Rust panic
/// in `f` is passed on to the caller.
pub(crate) fn on_interpreter_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    let f = std::sync::Mutex::new(Some(f));
    let take = |f: &std::sync::Mutex<Option<_>>| {
        f.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("`f` runs once")
    };
    std::thread::scope(|scope| {
        let spawned = std::thread::Builder::new()
            .name("oska-interpreter".to_string())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                interp::mark_stack_top(STACK_SIZE);
                take(&f)()
            });
        match spawned {
            Ok(thread) => thread
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            Err(_) => take(&f)(),
        }
    })
}

/// Check and run `src` on `host`. `args` are the program's arguments,
/// the program path first.
///
/// The program runs on a thread of its own with a large stack
/// ([`STACK_SIZE`]), and calls nested deeper than it allows panic with
/// `P0021`, so runaway recursion ends the program with a panic rather
/// than crashing the process.
pub fn run(src: &str, host: &mut dyn Host, args: &[String]) -> RunOutcome {
    on_interpreter_stack(|| check_and_run(src, host, args))
}

fn check_and_run(src: &str, host: &mut dyn Host, args: &[String]) -> RunOutcome {
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
        Ok(()) => RunOutcome::new(0, host),
        Err(Stop::Failed) => RunOutcome::new(1, host),
        Err(Stop::Panicked(panic)) => {
            let mut outcome = RunOutcome::new(PANIC_EXIT_CODE, host);
            outcome.panic = Some(panic.into_diagnostic());
            outcome
        }
    }
}

/// The `std` modules the runtime implements.
const IMPLEMENTED_MODULES: &[&str] = &[html::MODULE];

/// The refusal for the first import of a `std` module that has no runtime
/// yet, whether or not the program calls it.
pub(crate) fn unimplemented_module(program: &Program) -> Option<Diagnostic> {
    let (module, span) = program.imports.iter().find(|(module, _)| {
        builtins::std_module_of(module).is_some_and(|m| !IMPLEMENTED_MODULES.contains(&m))
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
        .find(|p| p.ty != builtins::ARGS && Capability::granted(&p.ty).is_none())?;
    let (code, message) = if builtins::is_capability(&param.ty) {
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

/// Why a program stopped before `main` returned success.
enum Stop {
    /// `main` returned `Err`, whose text is already on stderr: an
    /// expected failure.
    Failed,
    /// A bug.
    Panicked(Panic),
}

impl From<Panic> for Stop {
    fn from(panic: Panic) -> Self {
        Stop::Panicked(panic)
    }
}

/// Run `main`, granting each parameter the capability its type names, and
/// `args` to an `Args` parameter. When it returns `Err(e)`, `e`'s
/// `Display` text goes to stderr.
fn run_main(program: &Program, host: &mut dyn Host, args: &[String]) -> Result<(), Stop> {
    let Some(main) = program.functions.get("main") else {
        return Ok(());
    };
    let values = main
        .params
        .iter()
        .filter_map(|param| {
            if param.ty == builtins::ARGS {
                Some(Value::Args(
                    args.iter().map(|a| Rc::from(a.as_str())).collect(),
                ))
            } else {
                Capability::granted(&param.ty).map(Value::Capability)
            }
        })
        .collect();
    let mut interpreter = Interpreter::new(program, host);
    let value = interpreter.call(main, values)?;
    if !(value.type_tag() == builtins::RESULT && value.is_variant(builtins::ERR)) {
        return Ok(());
    }
    // The checker requires `main`'s error type to implement `Display`.
    let error = value.fields()[0].clone();
    let span = main.body.span;
    let text = interpreter.call_trait(builtins::DISPLAY_TO_STRING, error, Vec::new(), span)?;
    // The program has failed either way: a stderr that cannot be written
    // loses the message, not the exit code.
    let _ = host.write_stderr(&format!("{}\n", text.as_str()));
    Err(Stop::Failed)
}
