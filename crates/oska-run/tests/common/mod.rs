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
