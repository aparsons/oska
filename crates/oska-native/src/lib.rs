// SPDX-License-Identifier: Apache-2.0
//! The native backend: a checked program to a static executable for a
//! target platform ([`Target`]), with no assembler, linker or C library
//! (ADR 0011). `linux-aarch64` and `linux-x86_64` have backends so far.
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
//!    `constant_time.rs` checks the result: no branch and no memory
//!    address depends on a secret, or the build fails with an internal
//!    error (ADR 0012).
//! 2. The target's code generator (`target.rs`): `aarch64.rs` or
//!    `x86_64.rs`, with register allocation (`regalloc.rs`), instruction
//!    selection and encoding.
//! 3. `elf.rs`: the file, with the target's machine and page size.
//!
//! The crate reads only the checked program, never the syntax tree, has
//! no `unsafe`, and its output depends on the program and the target
//! alone, never on the host.

use oska_check::limits::Limits;
use oska_check::program::Program;
use oska_syntax::{Diagnostic, Frame, Frames, SnapshotRecord, Span};

pub use target::Target;

mod aarch64;
mod constant_time;
mod elf;
mod intrinsics;
mod ir;
mod layout;
mod lower;
mod macho;
mod optimize;
mod pe;
mod regalloc;
mod sha256;
mod target;
mod x86_64;

/// The codes the native backend reports.
pub mod codes {
    /// The program imports a `std` module that has no runtime yet, such
    /// as `std.sql`: the interpreter's code for the same refusal.
    pub const UNIMPLEMENTED_MODULE: &str = "E0371";
    /// The program uses something the native backend cannot compile yet.
    pub const UNSUPPORTED: &str = "E0390";
    /// The executable could not be written, or, for `oskac run` and
    /// `oskac test`, written or started in the temporary directory.
    pub const UNWRITABLE_OUTPUT: &str = "E0391";
    /// A native test process ended in a way no test can make it end (a
    /// signal killed it, or it exited with no report of its own), or the
    /// program `oskac run` ran natively died of a signal only a fault
    /// raises. A fault in the native backend, never in the program.
    pub const PROCESS_FAULT: &str = "E0392";
    /// `--target` names no supported platform.
    pub const UNKNOWN_PLATFORM: &str = "E0393";
    /// `--target` names a supported platform with no native backend yet.
    pub const NO_BACKEND: &str = "E0394";
    /// `oskac release` has no `--version`, or one that is not `X.Y.Z`.
    pub const BAD_VERSION: &str = "E0395";
    /// `oskac release` has no `--key`, or one that is not an SSH signing
    /// key, or `ssh-keygen` could not sign with it.
    pub const NOT_A_SIGNING_KEY: &str = "E0396";
    /// `oskac release` on a program whose name cannot name a release.
    pub const BAD_RELEASE_NAME: &str = "E0397";
    /// The backend's own code would make a branch or a memory address
    /// depend on a secret (`constant_time.rs`): an internal error, a bug
    /// in the backend, never in the program.
    pub const NOT_CONSTANT_TIME: &str = "E0398";

    /// The backend's own codes, in the order `docs/runtime.md` lists
    /// them. `E0371` is the interpreter's, shared.
    pub const ALL: [&str; 9] = [
        UNSUPPORTED,
        UNWRITABLE_OUTPUT,
        PROCESS_FAULT,
        UNKNOWN_PLATFORM,
        NO_BACKEND,
        BAD_VERSION,
        NOT_A_SIGNING_KEY,
        BAD_RELEASE_NAME,
        NOT_CONSTANT_TIME,
    ];

    /// The `P`-codes native code panics with: the interpreter's.
    /// `P0039` is never the program's own: `oskac` stops a native program
    /// at its `--timeout` and reports it, and `P0014` is the test
    /// runner's, after a test has run (`oska_run::snapshots`).
    pub const PANICS: [&str; 27] = [
        "P0001",
        "P0002",
        "P0003",
        "P0004",
        "P0005",
        "P0006",
        "P0007",
        "P0008",
        "P0010",
        "P0011",
        "P0012",
        "P0013",
        "P0014",
        "P0020",
        "P0021",
        "P0035",
        "P0036",
        "P0037",
        oska_check::limits::codes::STEP_LIMIT,
        oska_check::limits::codes::TIME_LIMIT,
        oska_check::limits::codes::MEMORY_LIMIT,
        oska_check::limits::codes::OUTPUT_LIMIT,
        "P0042",
        "P0043",
        "P0044",
        "P0045",
        "P0046",
    ];
}

/// The span of a refusal that is about no construct in particular.
const NOWHERE: Span = Span { start: 0, end: 0 };

/// The `std` modules that have a runtime on some backend. A program that
/// imports any other, such as `std.sql`, is refused, as the interpreter
/// refuses it (`oska-run`'s `IMPLEMENTED_MODULES`); a call into one the
/// native backend has no runtime for yet is refused where it is made.
const MODULES_WITH_A_RUNTIME: &[&str] = &["std.html"];

/// How the caller places a panic in its source, as `oskac` reports it. A
/// native panic report names the place its span is, and its frames', as
/// the interpreter's does, so the executable carries what this gives for
/// every panic site and every call.
pub trait Locate {
    /// `diagnostic`, whose span is an offset into the program, with its
    /// file, line and column, and its message worded as code there
    /// words it.
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic;

    /// `frame`, whose span is an offset into the program, placed as
    /// [`Locate::locate`] places a diagnostic's frames.
    fn locate_frame(&self, frame: Frame) -> Frame;

    /// Whether it places anything: a report is for a person then, and
    /// otherwise for `oskac` to read back and place ([`test_failure`]).
    fn places(&self) -> bool;
}

/// The [`Locate`] that places nothing: a report for `oskac` to read back
/// ([`test_failure`]).
pub struct Unplaced;

impl Locate for Unplaced {
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        diagnostic
    }

    fn locate_frame(&self, frame: Frame) -> Frame {
        frame
    }

    fn places(&self) -> bool {
        false
    }
}

/// How deeply calls of the program's functions nest before it panics
/// with `P0021`: the interpreter's limit.
pub const MAX_CALL_DEPTH: u32 = lower::MAX_CALL_DEPTH;

/// Compile `program`, which checked with no diagnostic, to an executable
/// for `target`: the file's bytes. `locate` places a panic site in the
/// source. A program the backend cannot compile yet is refused with the
/// diagnostic that says why.
pub fn build(
    program: &Program,
    target: Target,
    locate: &dyn Locate,
) -> Result<Vec<u8>, Diagnostic> {
    build_limited(program, target, locate, Limits::NONE)
}

/// [`build`], for `oskac run` under the run `limits` (spec #446): the
/// executable stops with a limit's panic (`P0038` for `--max-steps`,
/// `P0041` for `--max-output`, `P0040` for `--max-memory`) when the
/// program reaches it, at the same step or write as the interpreter.
/// The checks are in the generated code and `std.rt`, behind the
/// intrinsics that exist (ADR 0013); with no limit the executable is
/// [`build`]'s, byte for byte.
pub fn build_limited(
    program: &Program,
    target: Target,
    locate: &dyn Locate,
    limits: Limits,
) -> Result<Vec<u8>, Diagnostic> {
    compile(program, target, locate, lower::Entry::Main, limits)
}

/// Compile the `test` blocks of `program`, which checked with no
/// diagnostic, to one **test executable** for `target`: the file's
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
/// A test's snapshot assertions each write a record of what they were
/// given to stdout as they run, which [`snapshot_records`] reads back.
/// Then a test that passes exits with 0 and writes nothing more to stdout. A test
/// that panics, an assertion included, exits with 101 after writing its
/// panic's report to stdout, unlocated: `error[P....]: message (bytes
/// a..b)` and a newline, the span in the program, which
/// [`test_failure`] reads back. The caller places it in the source, as it
/// places the interpreter's.
pub fn build_tests(program: &Program, target: Target) -> Result<Vec<u8>, Diagnostic> {
    compile(
        program,
        target,
        &Unplaced,
        lower::Entry::Tests,
        Limits::NONE,
    )
}

/// What starts each snapshot record a test executable writes to stdout.
pub const SNAPSHOT_MARK: char = '\x0e';

/// The snapshot records at the start of a test executable's `stdout`
/// ([`build_tests`]), each the span of its assertion, the value's `dbg`
/// text and the call stack there, in the order they were written, and
/// the rest of `stdout`: `None` when a record is cut short or malformed.
///
/// A record is [`SNAPSHOT_MARK`], the span, `start..end`, a space, the
/// text's length in bytes, a newline, the text, and then the call stack
/// as a panic's report ends with it ([`test_failure`]): each frame
/// [`encode_frame`]d, innermost first, `\x1f+n` when frames were left
/// out, and a newline.
pub fn snapshot_records(stdout: &[u8]) -> Option<(Vec<SnapshotRecord>, &[u8])> {
    let mut records = Vec::new();
    let mut rest = stdout;
    while let Some(record) = rest.strip_prefix(&[SNAPSHOT_MARK as u8]) {
        let line_end = record.iter().position(|&b| b == b'\n')?;
        let head = std::str::from_utf8(&record[..line_end]).ok()?;
        let (span, length) = head.split_once(' ')?;
        let (start, end) = span.split_once("..")?;
        let span = Span {
            start: start.parse().ok()?,
            end: end.parse().ok()?,
        };
        let length: usize = length.parse().ok()?;
        let text = record.get(line_end + 1..)?;
        let value = std::str::from_utf8(text.get(..length)?).ok()?;
        let after = &text[length..];
        let stack_end = after.iter().position(|&b| b == b'\n')?;
        let stack = std::str::from_utf8(&after[..stack_end]).ok()?;
        let ("", Some(frames)) = split_frames(stack) else {
            return None;
        };
        records.push(SnapshotRecord {
            span,
            value: value.to_string(),
            frames,
        });
        rest = &after[stack_end + 1..];
    }
    Some((records, rest))
}

/// The panic a test executable's test reported on stdout ([`build_tests`]),
/// or a program built for `oskac run --format=json` on stderr: the
/// diagnostic the interpreter gives for it, not yet placed in the source.
/// `None` for anything else.
///
/// A report is `error[P....]: message (bytes a..b)`, then its call stack:
/// each frame [`encode_frame`]d, innermost first, then, when frames were
/// left out, `\x1f+n`, and a newline.
pub fn test_failure(stdout: &[u8]) -> Option<Diagnostic> {
    let report = std::str::from_utf8(stdout).ok()?.strip_suffix('\n')?;
    let (head, frames) = split_frames(report);
    let rest = head.strip_prefix("error[")?.strip_suffix(')')?;
    let (code, rest) = rest.split_once("]: ")?;
    let (message, span) = rest.rsplit_once(" (bytes ")?;
    let span = parse_span(span)?;
    let code = codes::PANICS.iter().find(|c| **c == code)?;
    let diagnostic = Diagnostic::new(code, message, span);
    Some(match frames {
        Some(frames) => diagnostic.with_frames(frames),
        None => diagnostic,
    })
}

/// A frame of a panic's call stack as a report for `oskac` to read back
/// gives it: `\x1f`, the function [`escape_name`]d, `\x1e`, then
/// `start..end`.
pub fn encode_frame(frame: &Frame) -> String {
    format!(
        "\x1f{}\x1e{}..{}",
        escape_name(&frame.function),
        frame.span.start,
        frame.span.end
    )
}

/// The byte that starts an escape in an encoded frame's function name.
const NAME_ESCAPE: char = '\x1d';

/// `name` with no `\x1f` or `\x1e` left, which end a frame's pieces: a
/// test's name may hold them. Each is written as [`NAME_ESCAPE`] and a
/// digit, as is [`NAME_ESCAPE`] itself, and [`unescape_name`] undoes it.
fn escape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            NAME_ESCAPE => out.push_str("\x1d0"),
            '\x1e' => out.push_str("\x1d1"),
            '\x1f' => out.push_str("\x1d2"),
            c => out.push(c),
        }
    }
    out
}

/// The name [`escape_name`] wrote as `escaped`; `None` for an escape it
/// does not write.
fn unescape_name(escaped: &str) -> Option<String> {
    let mut out = String::with_capacity(escaped.len());
    let mut chars = escaped.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            NAME_ESCAPE => match chars.next()? {
                '0' => NAME_ESCAPE,
                '1' => '\x1e',
                '2' => '\x1f',
                _ => return None,
            },
            c => c,
        });
    }
    Some(out)
}

/// `report` split into the report before its call stack, and its frames,
/// if it has any. The frames are the longest run of `\x1f`-led pieces at
/// its end that read as frames, so a message holding a `\x1f` stays
/// whole.
fn split_frames(report: &str) -> (&str, Option<Frames>) {
    let mut end = report.len();
    let mut frames = Vec::new();
    let mut omitted = 0;
    while let Some(at) = report[..end].rfind('\x1f') {
        let piece = &report[at + 1..end];
        if let Some(count) = piece.strip_prefix('+').filter(|_| frames.is_empty()) {
            if omitted != 0 {
                break;
            }
            let Ok(count) = count.parse() else {
                break;
            };
            omitted = count;
        } else {
            let Some((function, span)) = piece.split_once('\x1e') else {
                break;
            };
            let (Some(function), Some(span)) = (unescape_name(function), parse_span(span)) else {
                break;
            };
            frames.push(Frame::new(function, span));
        }
        end = at;
    }
    if frames.is_empty() {
        return (report, None);
    }
    frames.reverse();
    (&report[..end], Some(Frames { frames, omitted }))
}

/// `start..end`.
fn parse_span(text: &str) -> Option<Span> {
    let (start, end) = text.split_once("..")?;
    Some(Span {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

fn compile(
    program: &Program,
    target: Target,
    locate: &dyn Locate,
    entry: lower::Entry,
    limits: Limits,
) -> Result<Vec<u8>, Diagnostic> {
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
    let module = lower::program(program, &rt, locate, entry, target, limits)?;
    let text = target.emit(&module).map_err(|why| {
        Diagnostic::new(
            codes::UNSUPPORTED,
            format!("`oskac build` cannot compile this program yet: {why}"),
            NOWHERE,
        )
    })?;
    Ok(target.file(
        text.as_ref(),
        &module.rodata,
        module.data_size,
        module.stack,
    ))
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
