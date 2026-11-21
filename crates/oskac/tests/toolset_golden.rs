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

use std::path::{Path, PathBuf};
use std::process::Command;

/// `oskac check` on the fixture `name`, with `format` if given: its
/// stdout, and whether it succeeded.
fn check(name: &str, format: Option<&str>) -> (String, bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
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

fn assert_golden(actual: &str, name: &str) {
    let golden: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "the report differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
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
