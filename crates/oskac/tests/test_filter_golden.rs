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

use std::path::{Path, PathBuf};
use std::process::Command;

/// Three tests: `adds` and `adds twice` pass, `subtracts wrongly` fails.
const FILE: &str = "crates/oskac/tests/fixtures/test_filter.osk";
/// A package whose tests are in three modules.
const PACKAGE: &str = "crates/oskac/tests/fixtures/packages/multi";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn output_of(args: &[&str], shown: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("oskac should run");
    format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        shown.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn assert_golden(name: &str, actual: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/test_filter")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
}

/// `oskac <args>` meets the golden file `name`, by default and with
/// `--interpret`.
fn assert_both_backends(name: &str, args: &[&str]) {
    assert_golden(name, &output_of(args, args));
    let mut interpreted = args.to_vec();
    interpreted.push("--interpret");
    assert_golden(name, &output_of(&interpreted, args));
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
fn a_filter_for_another_command_or_without_a_substring_is_a_usage_error() {
    for args in [
        &["check", FILE, "--filter=adds"][..],
        &["run", FILE, "--filter=adds"],
        &["build", FILE, "--filter=adds"],
        &["test", FILE, "--filter="],
        &["test", FILE, "--filter=adds", "--filter=twice"],
        &["test", FILE, "--filter"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(repo_root())
            .output()
            .expect("oskac should run");
        assert_eq!(output.status.code(), Some(2), "oskac {args:?}");
        assert!(output.stdout.is_empty(), "oskac {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("oskac - the Oska compiler"),
            "oskac {args:?}"
        );
    }
}
