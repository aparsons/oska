//! `oska-eval validate`: checks a task set offline. Each case is a task set
//! under `tests/fixtures/validate/`; the report is compared with
//! `tests/golden/validate_<case>.json`. Refresh the golden files with
//! `UPDATE_GOLDEN=1 cargo test -p oska-eval --test validate` and review the
//! diff.

mod common;

use common::{assert_golden, harness, oskac};

fn validate(case: &str) -> common::Outcome {
    let oskac = oskac();
    let tasks = format!("tests/fixtures/validate/{case}");
    harness(&["validate", "--oskac", &oskac, &tasks])
}

#[test]
fn a_task_file_that_is_not_json_is_bad_input() {
    let outcome = validate("malformed");
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("validate_malformed.json", &outcome.stdout);
}

#[test]
fn every_problem_with_a_tasks_files_is_listed() {
    let outcome = validate("format");
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("validate_format.json", &outcome.stdout);
}

/// An HTML task caught by a security test and its audit sites, a program
/// whose output is scanned for a planted token, and a deferred task, which
/// is only checked.
#[test]
fn a_task_set_whose_solutions_behave_as_declared_is_valid() {
    let outcome = validate("valid");
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("validate_valid.json", &outcome.stdout);
}

/// Security checks that pass the insecure solution, a reference solution
/// that fails a functional check, one that asks for more authority than the
/// task expects, an insecure solution that does not work, and a starting
/// package that does not check.
#[test]
fn a_task_whose_solutions_do_not_behave_as_declared_fails() {
    let outcome = validate("failing");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("validate_failing.json", &outcome.stdout);
}

#[test]
fn an_oskac_that_cannot_run_is_bad_input() {
    let tasks = "tests/fixtures/validate/valid";
    let outcome = harness(&["validate", "--oskac", "tests/no-such-oskac", tasks]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("cannot run `tests/no-such-oskac"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_missing_task_directory_is_bad_input() {
    let outcome = harness(&[
        "validate",
        "--oskac",
        "oskac",
        "tests/fixtures/validate/absent",
    ]);
    assert_eq!(outcome.code, 2);
    assert!(outcome.stdout.is_empty(), "{}", outcome.stdout);
    assert!(
        outcome
            .stderr
            .contains("cannot read tests/fixtures/validate/absent"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn unknown_arguments_are_bad_input() {
    let outcome = harness(&["validate", "eval/tasks"]);
    assert_eq!(outcome.code, 2);
    assert!(outcome.stderr.contains("USAGE"), "{}", outcome.stderr);
}
