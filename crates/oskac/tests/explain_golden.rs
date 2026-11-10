//! End-to-end tests of `oskac explain` (spec #70, ticket #76): the entry of
//! a code in text and in JSON, an unknown code that suggests the closest
//! one, and a command line with no code.
//!
//! The expected output is in `tests/golden/`. To regenerate it after an
//! intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac --test
//! explain_golden` and review the diff.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Assert that `actual` is the content of the golden file `name`.
fn assert_golden(name: &str, actual: &str) {
    let golden = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

#[test]
fn explain_prints_the_entry_of_a_code() {
    let output = oskac(&["explain", "E0300"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    assert_golden("explain_e0300.txt", &text(&output.stdout));
}

#[test]
fn explain_prints_the_entry_as_json() {
    let output = oskac(&["explain", "E0300", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    assert_golden("explain_e0300.json", &text(&output.stdout));
}

#[test]
fn explain_shows_a_code_that_has_no_machine_fix_and_no_example() {
    let output = oskac(&["explain", "P0005"]);
    assert_eq!(output.status.code(), Some(0));
    assert_golden("explain_p0005.txt", &text(&output.stdout));
}

#[test]
fn an_unknown_code_is_an_error_that_suggests_the_closest_code() {
    let output = oskac(&["explain", "E0301"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&output.stdout), "");
    assert_golden("explain_unknown.txt", &text(&output.stderr));
}

#[test]
fn an_unknown_code_in_json_names_the_suggestion() {
    let output = oskac(&["explain", "E0301", "--format=json"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&output.stderr), "");
    assert_golden("explain_unknown.json", &text(&output.stdout));
}

#[test]
fn something_that_is_not_a_code_has_no_suggestion() {
    let output = oskac(&["explain", "frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    assert_golden("explain_not_a_code.txt", &text(&output.stderr));
}

#[test]
fn explain_with_no_code_prints_the_usage() {
    let output = oskac(&["explain"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("oskac explain <code>"));
}
