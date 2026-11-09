//! Helpers shared by the `oska-run` integration tests.
//!
//! Every test runs a small Oska program through [`oska_run::run`] on a
//! [`FakeHost`] and asserts on what a user sees: what the program printed,
//! its exit code, its panic, or the diagnostics that refused it.

#![allow(dead_code)]

use oska_run::{run, FakeHost, RunOutcome};

/// Run `src` on a fresh [`FakeHost`], with `args` after the program path.
pub fn run_fake(src: &str, args: &[&str]) -> RunOutcome {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run(src, &mut FakeHost::new(), &args)
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
/// `code`, and that it printed nothing.
pub fn assert_refused(src: &str, code: &str) {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.iter().any(|d| d.code == code),
        "{src:?} should be refused with {code}: {outcome:#?}"
    );
    assert_ne!(outcome.exit_code, 0, "a refused program fails");
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
