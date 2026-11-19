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
fn a_fix_that_was_applied_and_gone_at_the_next_check() {
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
fn a_run_that_finished_before_a_failed_grade_is_finished_despite_failing() {
    let failed = analyse_with("finished_failing", &["--functional", "fail"]);
    assert_eq!(failed.code, 0, "{}{}", failed.stdout, failed.stderr);
    assert_golden("analyse_finished_failing.json", &failed.stdout);
    let passed = analyse_with("finished_failing", &["--functional", "pass"]);
    assert!(
        passed
            .stdout
            .contains("\"finished_despite_failing\": false"),
        "{}",
        passed.stdout
    );
    let ungraded = analyse_with("finished_failing", &[]);
    assert!(
        ungraded
            .stdout
            .contains("\"finished_despite_failing\": null"),
        "{}",
        ungraded.stdout
    );
}

#[test]
fn two_diagnostics_with_one_code_in_one_file_are_not_merged() {
    analyse("same_code_twice");
}

#[test]
fn a_line_with_a_repeated_key_is_bad_input() {
    let outcome = analyse_with("duplicate_key", &[]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome
            .stdout
            .contains("line 2: the key `command` appears twice"),
        "{}",
        outcome.stdout
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

/// Every file beneath `dir`.
fn files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("the directory should be readable") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// No fixture, and no committed result, holds a real home path: recorded
/// transcripts and results are scrubbed before they are committed.
#[test]
fn the_fixtures_and_the_committed_results_hold_no_home_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    files_under(&root.join("tests/fixtures"), &mut files);
    files_under(&root.join("../../eval/results"), &mut files);
    assert!(files.len() > 20, "the walk should find the fixtures");
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for home in ["/home/pi", "/Users/", "C:\\Users", "C:/Users"] {
            assert!(
                !text.contains(home),
                "{} holds a home path ({home})",
                path.display()
            );
        }
    }
}
