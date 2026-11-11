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
//! 2. `aarch64.rs`: instruction selection and encoding.
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

/// The codes the native backend reports.
pub mod codes {
    /// The program imports a `std` module that has no runtime yet, such
    /// as `std.sql`: the interpreter's code for the same refusal.
    pub const UNIMPLEMENTED_MODULE: &str = "E0371";
    /// The program uses something the native backend cannot compile yet.
    pub const UNSUPPORTED: &str = "E0390";
    /// The executable could not be written.
    pub const UNWRITABLE_OUTPUT: &str = "E0391";

    /// The backend's own codes, in the order `docs/runtime.md` lists
    /// them. `E0371` is the interpreter's, shared.
    pub const ALL: [&str; 2] = [UNSUPPORTED, UNWRITABLE_OUTPUT];
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

/// Compile `program`, which checked with no diagnostic, to an executable
/// for [`TARGET`]: the file's bytes. A program the backend cannot compile
/// yet is refused with the diagnostic that says why.
pub fn build(program: &Program) -> Result<Vec<u8>, Diagnostic> {
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
    let module = lower::program(program, &rt)?;
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
