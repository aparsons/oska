// SPDX-License-Identifier: Apache-2.0
//! Native by default: on aarch64 and x86_64 Linux and macOS and on
//! x86_64 Windows (#179), `oskac run` and `oskac test` compile the checked
//! program for the host with the native backend (`oska-native`) and run
//! the executable; `--interpret` selects the interpreter, and every other
//! host keeps it until it has a backend of its own.
//!
//! The executable is written to a private temporary directory: a new
//! directory with a random name under the temporary directory (`TMPDIR`),
//! created with mode 0700 and never reused, so no other user can name,
//! replace or read what runs. On Windows no mode is set: the directory is
//! under the user's own temporary directory (`TEMP`), whose access it
//! takes. The file in it is created new, mode 0700 on Unix,
//! and both are removed afterwards: for `oskac run` as soon as the
//! program has started (on Windows, which keeps a running program's
//! file, once it ends), for `oskac test` once every test has run.
//!
//! A program the backend refuses (`E0390`) is an error, as `oskac build`
//! reports it, and nothing runs: the parity gate ([`gate`]), since every
//! supported platform is native. `--interpret` runs it on the
//! interpreter. A program the interpreter refuses too (`E0370`, `E0371`,
//! `E0380`) goes to it: nothing runs, and the interpreter's refusal says
//! why, as it always has.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use oska_check::program::Program;
use oska_native::Target;
use oska_syntax::{Diagnostic, Span};

use crate::cache::Cached;
use crate::cli::Flags;
use crate::places::Places;

/// The target `oskac run` and `oskac test` compile for with `flags`:
/// the host's, when it has a backend and `--interpret` is not given.
pub fn native_target(flags: Flags) -> Option<Target> {
    Target::host().filter(|_| !flags.interpret)
}

/// How a native `oskac run` went.
pub enum Run {
    /// The program ran, or could not be started: the exit code to end with.
    Ran(ExitCode),
    /// The backend refused the program, with this diagnostic, which the
    /// parity gate ([`gate`]) reports.
    Refused(Diagnostic),
}

/// The parity gate (spec #155, ticket #181): every supported platform is
/// native, so a program the native backend refuses with `refusal` is an
/// error on each, never a silent run on the interpreter. Returns the
/// refusal, located, for the caller to report; or `None` when the
/// interpreter refuses the program too (`interpreter_refusal`), whose own
/// refusal the interpreter then reports, as it always has.
pub fn gate(
    refusal: Diagnostic,
    interpreter_refusal: Option<Diagnostic>,
    places: &Places,
) -> Option<Diagnostic> {
    if interpreter_refusal.is_some() {
        return None;
    }
    Some(match refusal.location {
        Some(_) => refusal,
        None => places.locate(refusal),
    })
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
    /// The run limits, which the executable is built with: none builds
    /// the same executable as `oskac build`.
    pub limits: oska_check::limits::Limits,
    /// Whether the program's standard input is closed (`oskac run -`).
    pub stdin_closed: bool,
}

/// `oskac run`, natively: compile `program` for `target`, the host's, run
/// it with the invocation's `argv0` as its path and `args` after it, and
/// report a panic as the interpreter's run does, as JSON with `json`.
/// The executable is reused from the build cache (`cached`) when the
/// program's inputs are unchanged.
pub fn run(
    program: &Program,
    target: Target,
    places: &Places,
    invocation: Invocation,
    cached: &Cached,
) -> Run {
    let Invocation {
        argv0,
        args,
        json,
        mut limits,
        stdin_closed,
    } = invocation;
    // A human report is placed in the source by the executable itself; a
    // JSON one is read back unplaced and placed here, as the
    // interpreter's panic is.
    // The timeout is `oskac`'s to enforce, not built in: the executable,
    // and its build cache entry, are the ones without it.
    let timeout = limits.timeout.take();
    let built = cached.native("run", target, json, &crate::limits::flags(&limits), || {
        if json {
            oska_native::build_hooked(
                program,
                target,
                &oska_native::Unplaced,
                limits,
                crate::DNS_TEST_HOOKS,
            )
        } else {
            oska_native::build_hooked(program, target, places, limits, crate::DNS_TEST_HOOKS)
        }
    });
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
    if stdin_closed {
        command.stdin(Stdio::null());
    }
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return Run::Ran(fail(&cannot("run", &err), json)),
    };
    // The program has started: its file is no longer needed. Windows
    // keeps a running program's file, so there it goes once it ends.
    let mut exe = Some(exe);
    if cfg!(unix) {
        exe = None;
    }
    // A deadline past what the clock can count is never reached.
    let deadline =
        timeout.and_then(|ms| Some((started.checked_add(Duration::from_millis(ms))?, ms)));
    if let Some((deadline, ms)) = deadline {
        // It returns once the program has ended or been stopped.
        let code = wait_until_deadline(child, deadline, ms, places, json);
        drop(exe);
        return Run::Ran(code);
    }
    let panic = match child.stderr.take() {
        Some(stderr) => forward_stderr(stderr),
        None => None,
    };
    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => return Run::Ran(fail(&cannot("run", &err), json)),
    };
    drop(exe);
    if let Some(report) = panic {
        held_back(&report, status, places);
    }
    Run::Ran(ended(status, json))
}

/// Wait for `child`, the native program `oskac run` started, until
/// `deadline` (`--timeout=<ms>`). A program that ends by then ends as
/// without the limit. One that does not is stopped, it alone (its own
/// process, by the handle `oskac` started it with, never by name), and
/// `oskac` reports `P0039` itself and ends with 101, never with the
/// signal's `128 + n`. Whatever the stopped program held back as a
/// possible panic report is dropped, never reported as its own.
fn wait_until_deadline(
    mut child: Child,
    deadline: Instant,
    ms: u64,
    places: &Places,
    json: bool,
) -> ExitCode {
    // With JSON reports, stderr is a pipe: it is read on a thread of its
    // own while this one watches the clock.
    let reader = child
        .stderr
        .take()
        .map(|stderr| std::thread::spawn(move || forward_stderr(stderr)));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(err) => return fail(&cannot("run", &err), json),
        }
        let now = Instant::now();
        if now >= deadline {
            // `kill` fails only when the program has just ended: its
            // status is then the one `wait` gives, and it stops all the
            // same.
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep((deadline - now).min(DEADLINE_POLL));
    };
    let panic = reader.and_then(|reader| reader.join().ok().flatten());
    let Some(status) = status else {
        let limit = oska_check::limits::Limit::Time;
        let stopped = Diagnostic::new(limit.code(), limit.message(ms), Span { start: 0, end: 0 });
        report(&stopped, json);
        return ExitCode::from(101);
    };
    if let Some(report) = panic {
        held_back(&report, status, places);
    }
    ended(status, json)
}

/// Print `report`, the last line the program wrote to stderr that may be
/// a panic report and that [`forward_stderr`] held back: placed in the
/// source as JSON when the program panicked with it (exit code 101), else
/// as the program wrote it.
fn held_back(report: &[u8], status: std::process::ExitStatus, places: &Places) {
    match oska_native::test_failure(report).filter(|_| status.code() == Some(101)) {
        Some(panic) => {
            let _ = writeln!(std::io::stderr(), "{}", places.locate(panic).to_json());
        }
        None => {
            let _ = std::io::stderr().write_all(report);
        }
    }
}

/// How often [`wait_until_deadline`] looks whether the program has ended:
/// a program that ends before its deadline ends this much late at most.
const DEADLINE_POLL: Duration = Duration::from_millis(5);

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
        if let Some(exception) = windows_exception(code) {
            let fault = Diagnostic::new(
                oska_native::codes::PROCESS_FAULT,
                format!(
                    "the native program faulted with exception {exception:#010X}: a fault in the \
                     native backend, not in the program; run it with `--interpret`"
                ),
                Span { start: 0, end: 0 },
            );
            report(&fault, json);
        }
        // A Windows process's code is 32 bits, which `ExitCode` cannot
        // carry: `oskac` ends with the program's own, as a Windows shell
        // shows it, where Unix shows `128 + n` after a signal. Nothing is
        // left to drop: the executable went once the program ended.
        if cfg!(windows) && u8::try_from(code).is_err() {
            std::process::exit(code);
        }
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

/// The exception a Windows process that ended with `code` died of: a
/// status whose severity is an error (`0xC...`, such as
/// `STATUS_ACCESS_VIOLATION` or `STATUS_STACK_OVERFLOW`), or a
/// breakpoint, as Windows ends a process an exception was not handled
/// in. `None` elsewhere, for a code a program can exit with (Oska's are
/// 0 to 255), and for `STATUS_CONTROL_C_EXIT`, which Ctrl+C ends a
/// process with from outside, as `SIGINT` does on Unix.
pub fn windows_exception(code: i32) -> Option<u32> {
    const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
    let code = code as u32;
    let fault =
        code & 0xC000_0000 == 0xC000_0000 && code != STATUS_CONTROL_C_EXIT || code == 0x8000_0003;
    (cfg!(windows) && fault).then_some(code)
}

/// The host's signals a native process may die of, by number: the name,
/// and whether only a fault raises it. A program killed by any other
/// (`SIGKILL`, `SIGPIPE`, `SIGTERM`) was killed from outside, as any
/// process can be. The numbers are the same on every host with a backend
/// (asm-generic/signal.h, which arm64 takes,
/// arch/x86/include/uapi/asm/signal.h and macOS's sys/signal.h agree on
/// them) but `SIGBUS`'s ([`SIGBUS`]).
const SIGNALS: &[(i32, &str, bool)] = &[
    (4, "SIGILL", true),
    (5, "SIGTRAP", true),
    (6, "SIGABRT", true),
    (SIGBUS, "SIGBUS", true),
    (8, "SIGFPE", true),
    (9, "SIGKILL", false),
    (11, "SIGSEGV", true),
    (13, "SIGPIPE", false),
    (15, "SIGTERM", false),
];

/// `SIGBUS`'s number on the host: 10 on macOS, 7 on Linux. A stack
/// overflow on macOS is a `SIGBUS`.
const SIGBUS: i32 = if cfg!(target_os = "macos") { 10 } else { 7 };

/// The name of the host's signal `number`, when [`SIGNALS`] has it.
pub fn signal_name(number: i32) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find(|(n, _, _)| *n == number)
        .map(|(_, name, _)| *name)
}

/// The name of the host's signal `number`, for the ones a faulty program
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
             `{}` to a directory you can run programs from, or use `--interpret`",
            std::env::temp_dir().display(),
            // The variable `temp_dir` reads first.
            if cfg!(windows) { "TMP" } else { "TMPDIR" },
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
        Executable::create_in(&std::env::temp_dir(), bytes)
    }

    /// [`Executable::create`] with the private directory under `base`.
    pub fn create_in(base: &Path, bytes: &[u8]) -> std::io::Result<Executable> {
        let dir = private_dir(base)?;
        let exe = Executable {
            // Windows runs a file by its extension.
            path: dir.join(format!("program{}", std::env::consts::EXE_SUFFIX)),
            dir,
        };
        // A crash of `oskac` removes both, as `Drop` does (see `crash`).
        crate::crash::track(&exe.path);
        crate::crash::track(&exe.dir);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        // The file is closed before it runs: a file open for writing
        // cannot be executed. Nor can a fork copy the descriptor while it is
        // open and make the file busy (#270): `oskac` starts processes only
        // from its main thread, one at a time, after this write.
        options.open(&exe.path)?.write_all(bytes)?;
        Ok(exe)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Remove the file and its directory now, while the program runs from
    /// it: a running executable's file can go on Unix, and the program
    /// keeps running. Anything left (on Windows) goes when it is dropped.
    #[cfg(unix)]
    pub fn unlink(&self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

impl Drop for Executable {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
        crate::crash::untrack(&self.path);
        crate::crash::untrack(&self.dir);
    }
}

/// A new directory, mode 0700, under the temporary directory, with a
/// name no one can predict. An existing entry of that name, a symlink
/// included, is never used: another name is tried.
fn private_dir(base: &Path) -> std::io::Result<PathBuf> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
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
pub(crate) fn random() -> u64 {
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
