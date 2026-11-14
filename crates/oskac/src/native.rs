//! Native by default: on aarch64 Linux, `oskac run` and `oskac test`
//! compile the checked program with the native backend (`oska-native`)
//! and run the executable; `--interpret` selects the interpreter, and
//! every other host keeps it until it has a backend of its own.
//!
//! The executable is written to a private temporary directory: a new
//! directory with a random name under the temporary directory (`TMPDIR`),
//! created with mode 0700 and never reused, so no other user can name,
//! replace or read what runs. The file in it is created new, mode 0700,
//! and both are removed afterwards: for `oskac run` as soon as the
//! program has started, for `oskac test` once every test has run.
//!
//! A program the backend refuses (`E0390`) runs on the interpreter
//! instead, after one note on stderr that says so and why
//! ([`note_fallback`]): it is a checked program the interpreter supports,
//! so it runs, but a user timing it or relying on native speed must not
//! be misled. A program the interpreter refuses too (`E0370`, `E0371`,
//! `E0380`) goes to it with no note: nothing runs, and the interpreter's
//! refusal says why.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use oska_check::program::Program;
use oska_syntax::{Diagnostic, Severity, Span};

use crate::{Flags, Places};

/// Whether this host has a native backend: aarch64 Linux.
pub const HOST_HAS_BACKEND: bool = cfg!(all(target_arch = "aarch64", target_os = "linux"));

/// Whether `oskac run` and `oskac test` run natively with `flags`.
pub fn runs_natively(flags: Flags) -> bool {
    HOST_HAS_BACKEND && !flags.interpret
}

/// How a native `oskac run` went.
pub enum Run {
    /// The program ran, or could not be started: the exit code to end with.
    Ran(ExitCode),
    /// The backend refused the program, with this diagnostic: run it on
    /// the interpreter, after [`note_fallback`].
    Refused(Diagnostic),
}

/// Tell the user, on stderr, that the native backend refused the program
/// with `refusal` and it runs on the interpreter: one line, in the
/// diagnostics' style (`path:line:col: note: ...`), or one JSON object
/// with `"severity":"note"`. Nothing when the interpreter refuses the
/// program too (`interpreter_refusal`): nothing runs, and its refusal
/// is the one reported.
pub fn note_fallback(
    refusal: Diagnostic,
    interpreter_refusal: Option<Diagnostic>,
    places: &Places,
    json: bool,
) {
    if interpreter_refusal.is_some() {
        return;
    }
    let mut note = match refusal.location {
        Some(_) => refusal,
        None => places.locate(refusal),
    };
    note.message = format!("running on the interpreter: {}", note.message);
    note.severity = Severity::Note;
    if json {
        eprintln!("{}", note.to_json());
        return;
    }
    let text = format!("note: {} ({})", note.message, note.code);
    match &note.location {
        Some(location) => eprintln!("{location}: {text}"),
        None => eprintln!("{text}"),
    }
}

/// How `oskac run` was asked to run a program.
pub struct Invocation<'a> {
    /// The path the program was named by: its `Args.get(0)`, as the
    /// interpreter reports it.
    pub argv0: &'a str,
    /// The arguments after `--`.
    pub args: &'a [String],
    /// Whether reports are JSON (`--format=json`).
    pub json: bool,
}

/// `oskac run`, natively: compile `program`, run it with the invocation's
/// `argv0` as its path and `args` after it, and report a panic as the
/// interpreter's run does, as JSON with `json`.
pub fn run(program: &Program, places: &Places, invocation: Invocation) -> Run {
    let Invocation { argv0, args, json } = invocation;
    // A human report is placed in the source by the executable itself; a
    // JSON one is read back unplaced and placed here, as the
    // interpreter's panic is.
    let built = if json {
        oska_native::build(program, &|d| d)
    } else {
        oska_native::build(program, &|d| places.locate(d))
    };
    let bytes = match built {
        Ok(bytes) => bytes,
        Err(refusal) => return Run::Refused(refusal),
    };
    let exe = match Executable::create(&bytes) {
        Ok(exe) => exe,
        Err(err) => return Run::Ran(fail(&cannot("write", &err), json)),
    };
    let mut command = Command::new(exe.path());
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg0(argv0);
    }
    #[cfg(not(unix))]
    let _ = argv0;
    if json {
        command.stderr(Stdio::piped());
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return Run::Ran(fail(&cannot("run", &err), json)),
    };
    // The program has started: its file is no longer needed.
    drop(exe);
    let panic = match child.stderr.take() {
        Some(stderr) => forward_stderr(stderr),
        None => None,
    };
    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => return Run::Ran(fail(&cannot("run", &err), json)),
    };
    if let Some(report) = panic {
        match oska_native::test_failure(&report).filter(|_| status.code() == Some(101)) {
            Some(panic) => {
                let _ = writeln!(std::io::stderr(), "{}", places.locate(panic).to_json());
            }
            None => {
                let _ = std::io::stderr().write_all(&report);
            }
        }
    }
    Run::Ran(ended(status, json))
}

/// Copy the program's stderr to oskac's, line by line, as it comes, but
/// hold back each line that may be a panic report until the next line
/// or the end: the last one is returned, to be read back.
fn forward_stderr(mut stderr: impl Read) -> Option<Vec<u8>> {
    let mut out = std::io::stderr();
    let mut pending: Vec<u8> = Vec::new();
    let mut held: Option<Vec<u8>> = None;
    let mut buffer = [0u8; 8192];
    loop {
        let read = match stderr.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        pending.extend_from_slice(&buffer[..read]);
        while let Some(end) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if let Some(previous) = held.take() {
                let _ = out.write_all(&previous);
            }
            if line.starts_with(b"error[P") {
                held = Some(line);
            } else {
                let _ = out.write_all(&line);
            }
        }
    }
    if !pending.is_empty() {
        if let Some(previous) = held.take() {
            let _ = out.write_all(&previous);
        }
        let _ = out.write_all(&pending);
    }
    held
}

/// The exit code `oskac run` ends with after the program ended with
/// `status`: the program's own, or after a signal the shell's `128 + n`.
/// A signal only a fault can raise is reported (`E0392`): the
/// interpreter would not have died of it.
fn ended(status: std::process::ExitStatus, json: bool) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            if let Some(name) = fault_signal(signal) {
                let fault = Diagnostic::new(
                    oska_native::codes::PROCESS_FAULT,
                    format!(
                        "the native program was killed by signal {signal} ({name}): a fault in \
                         the native backend, not in the program; run it with `--interpret`"
                    ),
                    Span { start: 0, end: 0 },
                );
                report(&fault, json);
            }
            return ExitCode::from(u8::try_from(128 + signal).unwrap_or(u8::MAX));
        }
    }
    ExitCode::FAILURE
}

/// The Linux signals a native process may die of, by number: the name,
/// and whether only a fault raises it. A program killed by any other
/// (`SIGKILL`, `SIGPIPE`, `SIGTERM`) was killed from outside, as any
/// process can be.
const SIGNALS: &[(i32, &str, bool)] = &[
    (4, "SIGILL", true),
    (5, "SIGTRAP", true),
    (6, "SIGABRT", true),
    (7, "SIGBUS", true),
    (8, "SIGFPE", true),
    (9, "SIGKILL", false),
    (11, "SIGSEGV", true),
    (13, "SIGPIPE", false),
    (15, "SIGTERM", false),
];

/// The name of the Linux signal `number`, when [`SIGNALS`] has it.
pub fn signal_name(number: i32) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find(|(n, _, _)| *n == number)
        .map(|(_, name, _)| *name)
}

/// The name of the Linux signal `number`, for the ones a faulty program
/// dies of; `None` for one sent from outside.
pub fn fault_signal(number: i32) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find(|(n, _, fault)| *n == number && *fault)
        .map(|(_, name, _)| *name)
}

/// The refusal of a temporary executable that could not be written or
/// started (`E0391`), with `what` was done: the temporary directory is
/// unwritable, or mounted `noexec`.
pub fn cannot(what: &str, err: &std::io::Error) -> Diagnostic {
    Diagnostic::new(
        oska_native::codes::UNWRITABLE_OUTPUT,
        format!(
            "cannot {what} the native executable in the temporary directory `{}`: {err}; set \
             `TMPDIR` to a directory you can run programs from, or use `--interpret`",
            std::env::temp_dir().display()
        ),
        Span { start: 0, end: 0 },
    )
}

/// Print `diagnostic`, about the run rather than a place in the source
/// (`E0391`, `E0392`), to stderr: as `error[CODE]: message`, with no
/// location or span, or as one JSON object.
fn report(diagnostic: &Diagnostic, json: bool) {
    if json {
        eprintln!("{}", diagnostic.to_json());
    } else {
        eprintln!("error[{}]: {}", diagnostic.code, diagnostic.message);
    }
}

/// [`report`] `diagnostic` and end with exit code 1.
pub fn fail(diagnostic: &Diagnostic, json: bool) -> ExitCode {
    report(diagnostic, json);
    ExitCode::FAILURE
}

/// A native executable in a private temporary directory of its own,
/// both removed when dropped.
pub struct Executable {
    dir: PathBuf,
    path: PathBuf,
}

impl Executable {
    /// Write `bytes` as a new executable in a new private directory.
    pub fn create(bytes: &[u8]) -> std::io::Result<Executable> {
        let dir = private_dir()?;
        let exe = Executable {
            path: dir.join("program"),
            dir,
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        // The file is closed before it runs: a file open for writing
        // cannot be executed.
        options.open(&exe.path)?.write_all(bytes)?;
        Ok(exe)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Executable {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// A new directory, mode 0700, under the temporary directory, with a
/// name no one can predict. An existing entry of that name, a symlink
/// included, is never used: another name is tried.
fn private_dir() -> std::io::Result<PathBuf> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    let base = std::env::temp_dir();
    let mut last = None;
    for _ in 0..16 {
        let dir = base.join(format!("oskac-{:016x}", random()));
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last = Some(err),
            Err(err) => return Err(err),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
}

/// 64 random bits: the standard library's hash keys come from the
/// operating system's random source, and differ for every map.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_executable_lives_in_a_private_directory_of_its_own_until_dropped() {
        let one = Executable::create(b"one").expect("a temporary executable");
        let two = Executable::create(b"two").expect("another");
        assert_ne!(one.dir, two.dir, "each has its own directory");
        assert_eq!(one.dir.parent(), Some(std::env::temp_dir().as_path()));
        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&one.dir), 0o700);
        assert_eq!(mode(one.path()), 0o700);
        assert_eq!(std::fs::read(one.path()).expect("the file"), b"one");
        let (dir, path) = (one.dir.clone(), one.path.clone());
        drop(one);
        assert!(!path.exists() && !dir.exists(), "both are removed");
        assert!(two.path().exists());
    }

    #[test]
    fn only_a_fault_signal_is_reported() {
        assert_eq!(fault_signal(11), Some("SIGSEGV"));
        assert_eq!(fault_signal(13), None, "SIGPIPE: a closed pipe");
        assert_eq!(fault_signal(15), None, "SIGTERM: killed from outside");
    }
}
