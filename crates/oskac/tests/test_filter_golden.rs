// SPDX-License-Identifier: Apache-2.0
//! `oskac test --filter <substring>` (spec #367, ticket #371): run only the
//! tests whose names contain the substring, case-sensitive, and say how
//! many were filtered out. A filter that matches no test is an error, and
//! any other command given `--filter` is a usage error.
//!
//! Each case runs the real binary from the repository root and compares
//! the exit code, stdout and stderr with `tests/golden/test_filter/<case>.txt`.
//! A `test` case runs twice, by default (natively where the host has a
//! backend) and with `--interpret`, and both must meet the same golden
//! file. To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test test_filter_golden` and
//! review the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::portable::{literal_path, output_of};

/// Three tests: `adds` and `adds twice` pass, `subtracts wrongly` fails.
const FILE: &str = "crates/oskac/tests/fixtures/test_filter.osk";
/// A file with no tests at all.
const NO_TESTS: &str = "crates/oskac/tests/fixtures/test_filter_none.osk";
/// A package whose tests are in three modules.
const PACKAGE: &str = "crates/oskac/tests/fixtures/packages/multi";

fn repo_root() -> PathBuf {
    literal_path(Path::new(env!("CARGO_MANIFEST_DIR")), &["..", ".."])
}

/// What `oskac <args>`, run from the repository root, ended with and
/// printed.
fn oskac(args: &[&str]) -> std::process::Output {
    output_of(
        Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(repo_root()),
    )
}

/// The run of `oskac <args>` as a golden file shows it, headed by the
/// command line `shown`.
fn transcript(args: &[&str], shown: &[&str]) -> String {
    let output = oskac(args);
    format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        shown.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        common::golden::mask_durations(&String::from_utf8_lossy(&output.stdout)),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn assert_golden(name: &str, actual: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/test_filter")
        .join(format!("{name}.txt"));
    common::golden::assert_golden_file(&golden, actual);
}

/// `oskac <args>` meets the golden file `name`, by default and with
/// `--interpret`.
fn assert_both_backends(name: &str, args: &[&str]) {
    assert_golden(name, &transcript(args, args));
    let mut interpreted = args.to_vec();
    interpreted.push("--interpret");
    assert_golden(name, &transcript(&interpreted, args));
}

#[test]
fn a_filter_runs_only_the_tests_whose_names_contain_it() {
    assert_both_backends("adds", &["test", FILE, "--filter", "adds"]);
}

#[test]
fn a_filter_may_be_written_with_an_equals_sign_and_the_json_report_counts_the_filtered() {
    assert_both_backends(
        "adds_json",
        &["test", FILE, "--filter=adds", "--format=json"],
    );
}

#[test]
fn a_failing_test_the_filter_selects_still_fails_the_run() {
    assert_both_backends("wrongly", &["test", FILE, "--filter=wrongly"]);
}

#[test]
fn a_filter_matches_test_names_in_every_module_of_a_package() {
    assert_both_backends("package", &["test", PACKAGE, "--filter=d"]);
}

#[test]
fn a_filter_that_matches_no_test_is_an_error() {
    assert_both_backends("no_match", &["test", FILE, "--filter=nope"]);
    assert_both_backends(
        "no_match_json",
        &["test", FILE, "--filter=nope", "--format=json"],
    );
}

#[test]
fn a_filter_is_case_sensitive() {
    assert_both_backends("case", &["test", FILE, "--filter=Adds"]);
}

#[test]
fn a_filter_on_a_program_with_no_tests_says_there_are_none() {
    assert_both_backends("no_tests", &["test", NO_TESTS, "--filter=adds"]);
    assert_both_backends(
        "no_tests_json",
        &["test", NO_TESTS, "--filter=adds", "--format=json"],
    );
}

#[test]
fn a_filter_for_another_command_is_a_usage_error() {
    for args in [
        &["check", FILE, "--filter=adds"][..],
        &["run", FILE, "--filter=adds"],
        &["build", FILE, "--filter=adds"],
    ] {
        let output = oskac(args);
        assert_eq!(output.status.code(), Some(2), "oskac {args:?}");
        assert!(output.stdout.is_empty(), "oskac {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("oskac - the Oska compiler"),
            "oskac {args:?}"
        );
    }
}

/// `--filter` with no substring after it, at the end of the command line
/// or before another flag, never reads a file named `--filter` or takes
/// the flag as the substring.
#[test]
fn a_filter_without_a_substring_is_a_usage_error() {
    assert_golden(
        "missing",
        &transcript(&["test", "--filter"], &["test", "--filter"]),
    );
    let before_a_flag = ["test", FILE, "--filter", "--format=json"];
    assert_golden(
        "missing_before_a_flag",
        &transcript(&before_a_flag, &before_a_flag),
    );
}

#[test]
fn an_empty_or_repeated_filter_is_a_usage_error_that_says_which() {
    let empty = ["test", FILE, "--filter="];
    assert_golden("empty", &transcript(&empty, &empty));
    let repeated = ["test", FILE, "--filter=adds", "--filter", "twice"];
    assert_golden("repeated", &transcript(&repeated, &repeated));
}

/// Under `--format=json` a usage error of `--filter` is the JSON error
/// `oskac new` and `oskac self` give, on stdout, still with exit code 2.
#[test]
fn a_filter_usage_error_under_json_is_a_json_error() {
    let empty = ["test", FILE, "--filter=", "--format=json"];
    assert_golden("empty_json", &transcript(&empty, &empty));
}
