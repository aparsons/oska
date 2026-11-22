//! `oskac check` on implementations of `std.mcp`'s `Toolset` (#249):
//! the authority report lists each tool under `call`, with what its arm
//! reaches, in the human and the JSON report.
//!
//! `tests/fixtures/toolset_split.osk` has a `call` that is a `match` on
//! the tool's name with literal arms: one tool reaches through a method
//! of the toolset, one through a free function given a field, one writes
//! to standard output, one spec has no arm and one arm has no spec.
//! `tests/fixtures/toolset_unsplit.osk` has a `call` of another shape,
//! reported as one entry for the whole toolset.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test toolset_golden` and review
//! the diff.

mod common;

use std::path::Path;
use std::process::Command;

use common::golden::assert_golden_file;
use common::portable::literal_path;

/// `oskac check` on the fixture `name`, run from the repository's root
/// with the fixture's path as written there, so the report names it the
/// same way on every platform, with `format` if given: its stdout, and
/// whether it succeeded.
fn check(name: &str, format: Option<&str>) -> (String, bool) {
    let root = literal_path(Path::new(env!("CARGO_MANIFEST_DIR")), &["..", ".."]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command
        .args(["check", &format!("crates/oskac/tests/fixtures/{name}")])
        .current_dir(&root);
    if let Some(format) = format {
        command.arg(format);
    }
    let output = command.output().expect("oskac should run");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    (stdout, output.status.success())
}

/// Assert that `actual` is the golden file `name`; `UPDATE_GOLDEN=1`
/// rewrites it.
fn assert_golden(actual: &str, name: &str) {
    let golden = literal_path(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["tests", "golden", name],
    );
    assert_golden_file(&golden, actual);
}

#[test]
fn a_split_toolset_lists_each_tool_in_the_human_report() {
    let (stdout, ok) = check("toolset_split.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_split.txt");
}

#[test]
fn a_split_toolset_lists_each_tool_in_the_json_report() {
    let (stdout, ok) = check("toolset_split.osk", Some("--format=json"));
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_split.json");
}

#[test]
fn an_unsplit_toolset_has_one_entry_in_the_human_report() {
    let (stdout, ok) = check("toolset_unsplit.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unsplit.txt");
}

#[test]
fn an_unsplit_toolset_has_one_entry_in_the_json_report() {
    let (stdout, ok) = check("toolset_unsplit.osk", Some("--format=json"));
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unsplit.json");
}

#[test]
fn a_toolset_whose_specs_are_not_literal_says_its_names_are_not_checked() {
    let (stdout, ok) = check("toolset_unchecked.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unchecked.txt");
}
