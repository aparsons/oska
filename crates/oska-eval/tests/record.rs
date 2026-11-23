// SPDX-License-Identifier: Apache-2.0
//! `oska-eval record`: a run's grade and analysis joined, with what
//! produced them, into one run record. The grades and analyses are fixtures
//! under `tests/fixtures/record/`; each record is compared with
//! `tests/golden/record_<case>.json`. Refresh the golden files with
//! `UPDATE_GOLDEN=1 cargo test -p oska-eval --test record` and review the
//! diff. No test calls a model.

mod common;

use common::{assert_golden, harness_env, home_env, Outcome};

const TASKS: &str = "tests/fixtures/validate/valid";

/// The flags every record needs, for attempt 2 of `greeting`.
fn base() -> Vec<&'static str> {
    vec![
        "record",
        TASKS,
        "greeting",
        "--attempt",
        "2",
        "--variant",
        "hook",
        "--model",
        "example-model",
        "--agent",
        "claude-code",
        "--agent-version",
        "2.0.0",
        "--oskac-version",
        "0.4.0",
        "--oskac-commit",
        "4bb471f",
        "--task-set",
        "3f2a9c0d",
        "--started",
        "2026-11-20T10:00:00Z",
        "--ended",
        "2026-11-20T10:04:30Z",
    ]
}

/// The home directory the fixture analyses name, as the recorded
/// transcript wrote it. It is only text to `record`, which scrubs it from
/// the record whatever the platform, so it is the same on Windows.
const HOME: &str = "/home/user";

/// `record` with the base flags and `extra`, with the home [`HOME`]. A
/// flag in `extra` that the base flags have replaces its value there.
fn record(extra: &[&str]) -> Outcome {
    let mut args = base();
    for pair in extra.chunks(2) {
        match args.iter().position(|a| *a == pair[0]) {
            Some(at) if pair.len() == 2 => args[at + 1] = pair[1],
            _ => args.extend_from_slice(pair),
        }
    }
    let env = home_env(HOME);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    harness_env(&args, &env)
}

fn fixture(name: &str) -> String {
    format!("tests/fixtures/record/{name}.json")
}

#[test]
fn a_finished_run_joins_its_grade_and_analysis_with_no_local_path() {
    let grade = fixture("grade_insecure");
    let analysis = fixture("analysis_escape");
    let outcome = record(&[
        "--grade",
        &grade,
        "--analysis",
        &analysis,
        "--workspace",
        "/work/ws",
    ]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("record_escape.json", &outcome.stdout);
    assert!(!outcome.stdout.contains("/work/ws"), "{}", outcome.stdout);
    assert!(!outcome.stdout.contains(HOME), "{}", outcome.stdout);
}

#[test]
fn a_run_that_finished_and_failed_its_grade_is_a_false_claim() {
    let grade = fixture("grade_failing");
    let analysis = fixture("analysis_fix");
    let outcome = record(&["--grade", &grade, "--analysis", &analysis]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("record_false_claim.json", &outcome.stdout);
}

#[test]
fn a_run_stopped_by_its_timeout_with_no_grade_is_recorded_not_dropped() {
    let analysis = fixture("analysis_fix");
    let outcome = record(&["--stop", "timeout", "--analysis", &analysis]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("record_timeout.json", &outcome.stdout);
}

/// Each case is bad input, with the error it must name.
#[test]
fn bad_input_is_refused_with_what_is_wrong() {
    let pass = fixture("grade_pass");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--grade", &pass, "--stop", "lunch"], "`--stop`"),
        (vec!["--grade", &pass, "--attempt", "6"], "attempt 6"),
        (vec!["--grade", &pass, "--attempt", "0"], "`--attempt`"),
        (
            vec!["--grade", &pass, "--started", "yesterday"],
            "`--started`",
        ),
        (vec!["--grade", &pass, "--variant", "a b"], "`--variant`"),
        (vec!["--grade", &pass, "--colour", "red"], "`--colour`"),
        (
            vec!["--grade", "tests/fixtures/record/none.json"],
            "cannot read",
        ),
        (
            vec!["--grade", "tests/fixtures/record/malformed.json"],
            "at line 3, column 3",
        ),
        (
            vec!["--grade", &pass],
            "neither a stop reason nor an analysis",
        ),
    ];
    for (extra, expected) in cases {
        let outcome = record(&extra);
        assert_eq!(outcome.code, 2, "{extra:?}: {}", outcome.stdout);
        assert!(
            outcome.stdout.contains(expected),
            "{extra:?} should name {expected}: {}",
            outcome.stdout
        );
    }
}

#[test]
fn a_grade_of_another_task_is_refused() {
    let grade = fixture("grade_pass");
    let mut args = base();
    args[2] = "token";
    args.extend_from_slice(&["--stop", "error", "--grade", &grade]);
    let outcome = harness_env(&args, &[]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("grade is of the task `greeting`"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_missing_flag_is_refused() {
    let mut args = base();
    args.truncate(args.len() - 2);
    let outcome = harness_env(&args, &[]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("`--ended`"), "{}", outcome.stdout);
}
