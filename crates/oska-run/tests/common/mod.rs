//! Helpers shared by the `oska-run` integration tests.
//!
//! Every test runs a small Oska program through [`oska_run::run`] on a
//! [`FakeHost`] and asserts on what a user sees: what the program printed,
//! its exit code, its panic, or the diagnostics that refused it.

#![allow(dead_code)]

use oska_run::{
    codes, run, test, FakeHost, Host, RunOutcome, TestOptions, TestReport, UNSUPPORTED_EXIT_CODE,
};

/// The program path every helper passes as `args[0]`.
pub const PROGRAM_PATH: &str = "prog.osk";

/// Run `src` on a fresh [`FakeHost`], with `args` after the program path.
pub fn run_fake(src: &str, args: &[&str]) -> RunOutcome {
    run_on(FakeHost::new(), src, args)
}

/// Run `src` on `host`, with `args` after the program path.
pub fn run_on(mut host: FakeHost, src: &str, args: &[&str]) -> RunOutcome {
    let args: Vec<String> = std::iter::once(PROGRAM_PATH)
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    run(src, &mut host, &args)
}

/// Whether `code` is one the runtime refuses a checked program with,
/// rather than one of the checker's.
fn is_runtime_refusal(code: &str) -> bool {
    codes::ALL.contains(&code) && code.starts_with('E')
}

/// Assert that `src` runs to completion with exit code 0, and that it
/// printed exactly `expected_stdout`.
pub fn assert_runs(src: &str, args: &[&str], expected_stdout: &str) {
    let outcome = run_fake(src, args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src:?} should run cleanly: {outcome:#?}"
    );
    assert_eq!(outcome.stdout, expected_stdout, "stdout of {src:?}");
}

/// Assert that `src` is refused before it starts, with a diagnostic
/// `code`, and that it printed nothing. A checker code exits with 1; a
/// runtime refusal (`E0370`, `E0380`, ...) of a program that checks
/// exits with 2.
pub fn assert_refused(src: &str, code: &str) {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.iter().any(|d| d.code == code),
        "{src:?} should be refused with {code}: {outcome:#?}"
    );
    let exit_code = if is_runtime_refusal(code) {
        UNSUPPORTED_EXIT_CODE
    } else {
        1
    };
    assert_eq!(
        outcome.exit_code, exit_code,
        "the exit code refusing {code}"
    );
    assert_eq!(outcome.stdout, "", "a refused program prints nothing");
    assert!(outcome.panic.is_none(), "a refused program never panics");
}

/// Assert that `src` stops with a panic `code` whose span covers exactly
/// `spanned_text`, and that it exits with code 101.
pub fn assert_panics(src: &str, code: &str, spanned_text: &str) {
    let outcome = run_fake(src, &[]);
    let Some(panic) = &outcome.panic else {
        panic!("{src:?} should panic with {code}: {outcome:#?}");
    };
    assert_eq!(panic.code, code, "the panic code of {src:?}: {panic:#?}");
    assert_eq!(
        &src[panic.span.start..panic.span.end],
        spanned_text,
        "the text the panic {code} spans"
    );
    assert_eq!(outcome.exit_code, 101, "a panic exits with code 101");
}

/// Assert that `src` runs to the end with exit code `code`, without a
/// panic, and that it wrote exactly `expected_stderr` to stderr.
pub fn assert_exits(src: &str, code: i32, expected_stderr: &str) {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src:?} should run without a panic: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, code, "the exit code of {src:?}");
    assert_eq!(outcome.stderr, expected_stderr, "stderr of {src:?}");
}

/// Run the `test` blocks of `src`, the plain ones on fresh [`FakeHost`]s.
pub fn test_fake(src: &str) -> TestReport {
    test_with(FakeHost::new(), src, TestOptions::default())
}

/// Run the `test` blocks of `src` with `options`; `host` backs the
/// `#[integration]` tests when they run.
pub fn test_with(mut host: FakeHost, src: &str, options: TestOptions) -> TestReport {
    test(src, &mut host, options)
}

// ---- Filesystem capability programs --------------------------------------------------

/// Oska helpers a filesystem test program may call, to print what an
/// operation returned: its text (or `done`, or the names joined with
/// `,`), or `REFUSED: ` and its error's message.
pub const SHOWN: &str = r#"
fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// The source of a program whose `main` runs `body`, each line one
/// `io.println`, followed by [`SHOWN`]. `main` is given `io: Stdout`,
/// `fs: Fs`, `read: FsRead`, `write: FsWrite`, and `args: Args`.
pub fn fs_program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, fs: Fs, read: FsRead, write: FsWrite, args: Args) \
         -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{SHOWN}"
    )
}

/// What the [`fs_program`] running `body` prints on `host`, with `args`
/// after the program path. The host is left as the program left it.
pub fn printed_with(host: &mut dyn Host, args: &[&str], body: &str) -> String {
    let src = fs_program(body);
    let args: Vec<String> = std::iter::once(PROGRAM_PATH)
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    let outcome = run(&src, host, &args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// What the [`fs_program`] running `body` prints on `host`, with no
/// arguments.
pub fn printed(mut host: FakeHost, body: &str) -> String {
    printed_with(&mut host, &[], body)
}
