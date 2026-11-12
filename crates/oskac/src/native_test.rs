//! `oskac test --native`: a program's `test` blocks run as native code.
//!
//! PROVISIONAL: `--native` is how the native test runner is reached until
//! #141 makes native the default on aarch64 Linux (and `--interpret` the
//! way back); it is not in `oskac --help`.
//!
//! The program's tests compile into one test executable
//! (`oska_native::build_tests`), written to a temporary file, which then
//! runs once per test, in source order, given the test's number: one
//! process per test, so each starts on a fresh test host and a panic stops
//! only its own test. A process inherits stderr, where the test's `dbg`
//! lines go, prefixed with its name; its stdout carries only its panic's
//! report (`oska_native::test_failure`). An `#[integration]` test runs
//! only when asked, and is skipped otherwise without starting a process.
//! The result is the report the interpreter's runner gives
//! (`oska_run::TestReport`), printed by the same code.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oska_check::program::Program;
use oska_run::{TestOptions, TestReport, TestResult, TestStatus};
use oska_syntax::{Diagnostic, Span};

/// Run the `test` blocks of the checked `program` natively.
pub fn test_program(program: &Program, options: TestOptions) -> TestReport {
    let bytes = match oska_native::build_tests(program) {
        Ok(bytes) => bytes,
        Err(refusal) => return refused(refusal),
    };
    let exe = Executable::new();
    if let Err(err) = exe.write(&bytes) {
        return refused(Diagnostic::new(
            oska_native::codes::UNWRITABLE_OUTPUT,
            format!(
                "cannot write the test executable `{}`: {err}",
                exe.path.display()
            ),
            Span { start: 0, end: 0 },
        ));
    }
    let tests = program
        .tests
        .iter()
        .enumerate()
        .map(|(index, test)| {
            let (status, failure) = if test.integration && !options.integration {
                (TestStatus::Skipped, None)
            } else {
                match run_one(&exe.path, index) {
                    None => (TestStatus::Passed, None),
                    Some(failure) => (TestStatus::Failed, Some(failure)),
                }
            };
            TestResult {
                name: test.name.clone(),
                module: None,
                span: test.span,
                status,
                failure,
            }
        })
        .collect();
    TestReport {
        tests,
        diagnostics: Vec::new(),
    }
}

/// Whether the report's refusal is one of the native backend's own
/// (`E0390`), which `oskac test` exits with 2 for, as it does for the
/// interpreter's.
pub fn is_unsupported(report: &TestReport) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.code == oska_native::codes::UNSUPPORTED)
}

fn refused(refusal: Diagnostic) -> TestReport {
    TestReport {
        tests: Vec::new(),
        diagnostics: vec![refusal],
    }
}

/// Run test `index` of the test executable at `exe`: why it failed, or
/// `None` when it passed.
fn run_one(exe: &Path, index: usize) -> Option<Diagnostic> {
    let output = Command::new(exe)
        .arg(index.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => return Some(abnormal(&format!("could not start: {err}"))),
    };
    match output.status.code() {
        Some(0) if output.stdout.is_empty() => None,
        Some(101) => oska_native::test_failure(&output.stdout).or_else(|| {
            Some(abnormal(&format!(
                "panicked with a report that is not one: {:?}",
                String::from_utf8_lossy(&output.stdout)
            )))
        }),
        Some(0) => Some(abnormal(&format!(
            "passed but printed {:?}",
            String::from_utf8_lossy(&output.stdout)
        ))),
        _ => Some(abnormal(&ending(output.status))),
    }
}

/// The failure of a test whose process ended in a way no test can make
/// it end (`E0392`): a fault in the native backend, as a Rust panic in the
/// interpreter would be one in the interpreter. A test's own failures are
/// panics, reported as the interpreter reports them.
fn abnormal(what: &str) -> Diagnostic {
    Diagnostic::new(
        oska_native::codes::TEST_PROCESS_FAULT,
        format!(
            "the native test process {what}: a fault in the native backend, not in the test; \
             run the test with the interpreter"
        ),
        Span { start: 0, end: 0 },
    )
}

/// How a test process ended that no test can make it end: killed by a
/// signal, named, or an exit code other than a pass's or a panic's.
fn ending(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return match signal_name(signal) {
                Some(name) => format!("was killed by signal {signal} ({name})"),
                None => format!("was killed by signal {signal}"),
            };
        }
    }
    match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => format!("ended with {status}"),
    }
}

/// The name of the Linux signal `number`, for the ones a faulty program
/// dies of.
fn signal_name(number: i32) -> Option<&'static str> {
    Some(match number {
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        15 => "SIGTERM",
        _ => return None,
    })
}

/// The test executable's temporary file, removed when dropped.
struct Executable {
    path: PathBuf,
}

impl Executable {
    fn new() -> Executable {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!("oskac-test-{}-{nanos}", std::process::id());
        Executable {
            path: std::env::temp_dir().join(name),
        }
    }

    fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(bytes)
    }
}

impl Drop for Executable {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Run the shell script `script` as a test process.
    fn run_script(name: &str, script: &str) -> Option<Diagnostic> {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("oskac-native-test-{}-{name}", std::process::id()));
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("the script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("making the script executable");
        let failure = run_one(&path, 0);
        let _ = std::fs::remove_file(&path);
        failure
    }

    #[test]
    fn a_test_process_killed_by_a_signal_is_a_fault_of_the_backend() {
        let failure = run_script("segv", "kill -SEGV $$").expect("a failure");
        assert_eq!(failure.code, "E0392");
        assert_eq!(
            failure.message,
            "the native test process was killed by signal 11 (SIGSEGV): a fault in the native \
             backend, not in the test; run the test with the interpreter"
        );
    }

    #[test]
    fn a_test_process_that_ends_with_no_report_is_a_fault_of_the_backend() {
        let failure = run_script("exit3", "exit 3").expect("a failure");
        assert_eq!(failure.code, "E0392");
        assert!(failure
            .message
            .starts_with("the native test process exited with code 3:"));
        let failure = run_script("garbled", "echo nonsense; exit 101").expect("a failure");
        assert_eq!(failure.code, "E0392");
        assert!(
            failure
                .message
                .starts_with("the native test process panicked with a report that is not one"),
            "{}",
            failure.message
        );
        assert_eq!(run_script("pass", "exit 0"), None);
    }
}
