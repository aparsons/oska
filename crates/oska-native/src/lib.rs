//! The native backend: a checked program to a static aarch64 Linux
//! executable, with no assembler, linker or C library (ADR 0011).
//!
//! [`build`] runs the pipeline:
//!
//! 1. `lower.rs`: the checked program, and the toolchain's own `std.rt`
//!    (ADR 0013), checked with [`oska_check::check_toolchain_rt`], to the
//!    backend's code (`ir.rs`), from the generated entry point `_start`
//!    through every function it reaches. `layout.rs` says how values are
//!    represented and `intrinsics.rs` how body-less functions are
//!    implemented. Anything the backend cannot compile yet is refused
//!    here, with `E0390`.
//!    `optimize.rs` then cleans up the code and inlines small functions.
//! 2. `aarch64.rs`: register allocation (`regalloc.rs`), instruction
//!    selection and encoding.
//! 3. `elf.rs`: the file.
//!
//! The crate reads only the checked program, never the syntax tree, has
//! no `unsafe`, and its output depends on the program alone, never on the
//! host.

use oska_check::program::Program;
use oska_syntax::{Diagnostic, Span};

mod aarch64;
mod elf;
mod intrinsics;
mod ir;
mod layout;
mod lower;
mod optimize;
mod regalloc;

/// The codes the native backend reports.
pub mod codes {
    /// The program imports a `std` module that has no runtime yet, such
    /// as `std.sql`: the interpreter's code for the same refusal.
    pub const UNIMPLEMENTED_MODULE: &str = "E0371";
    /// The program uses something the native backend cannot compile yet.
    pub const UNSUPPORTED: &str = "E0390";
    /// The executable could not be written.
    pub const UNWRITABLE_OUTPUT: &str = "E0391";
    /// A native test process ended in a way no test can make it end: a
    /// signal killed it, or it exited with no report of its own. A fault
    /// in the native backend, never in the test.
    pub const TEST_PROCESS_FAULT: &str = "E0392";

    /// The backend's own codes, in the order `docs/runtime.md` lists
    /// them. `E0371` is the interpreter's, shared.
    pub const ALL: [&str; 3] = [UNSUPPORTED, UNWRITABLE_OUTPUT, TEST_PROCESS_FAULT];

    /// The `P`-codes native code panics with: the interpreter's.
    pub const PANICS: [&str; 16] = [
        "P0001", "P0002", "P0003", "P0004", "P0005", "P0006", "P0007", "P0010", "P0011", "P0012",
        "P0013", "P0020", "P0021", "P0035", "P0036", "P0037",
    ];
}

/// The span of a refusal that is about no construct in particular.
const NOWHERE: Span = Span { start: 0, end: 0 };

/// The platform the executables run on.
pub const TARGET: &str = "aarch64-linux";

/// The `std` modules that have a runtime on some backend. A program that
/// imports any other, such as `std.sql`, is refused, as the interpreter
/// refuses it (`oska-run`'s `IMPLEMENTED_MODULES`); a call into one the
/// native backend has no runtime for yet is refused where it is made.
const MODULES_WITH_A_RUNTIME: &[&str] = &["std.html"];

/// How the caller places a diagnostic in its source: the diagnostic with
/// its file, line and column, as `oskac` reports it. A native panic
/// report names the place its span is, as the interpreter's does, so the
/// executable carries what this gives for every panic site.
pub type Locate<'a> = &'a dyn Fn(Diagnostic) -> Diagnostic;

/// How deeply calls of the program's functions nest before it panics
/// with `P0021`: the interpreter's limit.
pub const MAX_CALL_DEPTH: u32 = lower::MAX_CALL_DEPTH;

/// Compile `program`, which checked with no diagnostic, to an executable
/// for [`TARGET`]: the file's bytes. `locate` places a panic site in the
/// source. A program the backend cannot compile yet is refused with the
/// diagnostic that says why.
pub fn build(program: &Program, locate: Locate) -> Result<Vec<u8>, Diagnostic> {
    compile(program, locate, lower::Entry::Main)
}

/// Compile the `test` blocks of `program`, which checked with no
/// diagnostic, to one **test executable** for [`TARGET`]: the file's
/// bytes. Its `main` is never run.
///
/// The executable runs one test per process: the one whose index in
/// [`Program::tests`] is its only argument. Run any other way it writes a
/// usage error to stderr and exits with 2. A plain test runs on the test
/// host in `std.rt` (an empty in-memory filesystem, a clock fixed at the
/// Unix epoch, `Random` from the interpreter's fixed seed), an
/// `#[integration]` one on the real OS; either way its stdout is
/// recorded for `t.printed()`, never written, and each line it writes to
/// stderr (`dbg`) starts with `[name] `, as the interpreter's test host
/// has it. Which host a test runs on is fixed when the executable is
/// built, and a program's executable ([`build`]) has no test host at all.
///
/// A test that passes exits with 0 and writes nothing to stdout. A test
/// that panics, an assertion included, exits with 101 after writing its
/// panic's report to stdout, unlocated: `error[P....]: message (bytes
/// a..b)` and a newline, the span in the program, which
/// [`test_failure`] reads back. The caller places it in the source, as it
/// places the interpreter's.
pub fn build_tests(program: &Program) -> Result<Vec<u8>, Diagnostic> {
    compile(program, &|d| d, lower::Entry::Tests)
}

/// The panic a test executable's test reported on stdout ([`build_tests`]):
/// the diagnostic the interpreter's test runner gives for it, not yet
/// placed in the source. `None` for anything else.
pub fn test_failure(stdout: &[u8]) -> Option<Diagnostic> {
    let report = std::str::from_utf8(stdout).ok()?;
    let rest = report.strip_prefix("error[")?.strip_suffix(")\n")?;
    let (code, rest) = rest.split_once("]: ")?;
    let (message, span) = rest.rsplit_once(" (bytes ")?;
    let (start, end) = span.split_once("..")?;
    let code = codes::PANICS.iter().find(|c| **c == code)?;
    let span = Span {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    };
    Some(Diagnostic::new(code, message, span))
}

fn compile(program: &Program, locate: Locate, entry: lower::Entry) -> Result<Vec<u8>, Diagnostic> {
    if let Some(refusal) = unimplemented_module(program) {
        return Err(refusal);
    }
    let runtime = oska_check::check_toolchain_rt();
    let Some(rt) = runtime.program else {
        return Err(Diagnostic::new(
            codes::UNSUPPORTED,
            "`oskac build` cannot compile anything: the runtime, `std.rt`, does not check",
            NOWHERE,
        ));
    };
    let module = lower::program(program, &rt, locate, entry)?;
    let text = aarch64::emit(&module).map_err(|why| {
        Diagnostic::new(
            codes::UNSUPPORTED,
            format!("`oskac build` cannot compile this program yet: {why}"),
            NOWHERE,
        )
    })?;
    Ok(elf::write(&text, &module.rodata, module.data_size))
}

/// The refusal for the first import of a `std` module with no runtime.
fn unimplemented_module(program: &Program) -> Option<Diagnostic> {
    let (module, span) = program.imports.iter().find(|(module, _)| {
        oska_check::builtins::std_module_of(module)
            .is_some_and(|m| !MODULES_WITH_A_RUNTIME.contains(&m))
    })?;
    Some(Diagnostic::new(
        codes::UNIMPLEMENTED_MODULE,
        format!(
            "`oskac build` cannot compile a program that uses `{module}` yet: it has no runtime"
        ),
        *span,
    ))
}
