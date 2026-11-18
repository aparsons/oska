//! `oska-eval analyse`: how an agent worked, read from a recorded Claude
//! Code `stream-json` transcript. Each case is a transcript under
//! `tests/fixtures/analyse/`; the analysis is compared with
//! `tests/golden/analyse_<case>.json`. Refresh the golden files with
//! `UPDATE_GOLDEN=1 cargo test -p oska-eval --test analyse` and review the
//! diff. No test calls a model.

mod common;

use common::{assert_golden, harness};

fn analyse_with(case: &str, extra: &[&str]) -> common::Outcome {
    let transcript = format!("tests/fixtures/analyse/{case}.jsonl");
    let mut args = vec!["analyse"];
    args.extend_from_slice(extra);
    args.push(&transcript);
    harness(&args)
}

fn analyse(case: &str) -> common::Outcome {
    let outcome = analyse_with(case, &[]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden(&format!("analyse_{case}.json"), &outcome.stdout);
    outcome
}

#[test]
fn a_clean_first_check_is_one_iteration() {
    analyse("clean_first");
}

#[test]
fn a_fix_that_was_applied_and_helped() {
    analyse("fix_applied_helped");
}

#[test]
fn a_fix_that_was_offered_and_ignored() {
    analyse("fix_ignored");
}

#[test]
fn a_fix_that_was_applied_and_did_not_help() {
    analyse("fix_applied_unhelped");
}

#[test]
fn a_code_that_lingers_across_checks_in_text_output() {
    analyse("lingers");
}

#[test]
fn checks_that_came_only_from_the_hook_count_as_iterations() {
    analyse("hook_only");
}

#[test]
fn a_run_with_no_check_has_no_iterations() {
    analyse("no_check");
}

#[test]
fn a_run_cut_off_by_the_turn_limit() {
    analyse("turn_limit");
}

#[test]
fn tool_calls_outside_the_workspace_are_escapes() {
    analyse("escape");
}

#[test]
fn explain_and_site_lookups_and_oskac_fix_across_diagnostic_shapes() {
    analyse("lookups_and_fix");
}

#[test]
fn a_final_message_before_a_failed_grade_is_a_false_claim() {
    let failed = analyse_with("false_claim", &["--functional", "fail"]);
    assert_eq!(failed.code, 0, "{}{}", failed.stdout, failed.stderr);
    assert_golden("analyse_false_claim.json", &failed.stdout);
    let passed = analyse_with("false_claim", &["--functional", "pass"]);
    assert!(
        passed.stdout.contains("\"false_claim\": false"),
        "{}",
        passed.stdout
    );
    let ungraded = analyse_with("false_claim", &[]);
    assert!(
        ungraded.stdout.contains("\"false_claim\": null"),
        "{}",
        ungraded.stdout
    );
}

#[test]
fn a_workspace_flag_overrides_the_one_the_transcript_names() {
    let outcome = analyse_with("escape", &["--workspace", "/home/user"]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        !outcome.stdout.contains("id_rsa"),
        "a path under the given workspace is not an escape:\n{}",
        outcome.stdout
    );
}

#[test]
fn a_malformed_transcript_is_bad_input() {
    let outcome = analyse_with("malformed", &[]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("analyse_malformed.json", &outcome.stdout);
}

#[test]
fn a_missing_transcript_is_bad_input() {
    let outcome = harness(&["analyse", "tests/fixtures/analyse/no-such.jsonl"]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("cannot read"), "{}", outcome.stdout);
}

#[test]
fn the_fixtures_hold_no_home_path() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analyse");
    for entry in std::fs::read_dir(dir).expect("the fixtures should exist") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a fixture should be text");
        assert!(
            !text.contains("/home/pi") && !text.contains("/Users/"),
            "{} holds a home path",
            path.display()
        );
    }
}
