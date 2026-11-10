//! `oskac check` on methods called qualified by their type (review fix F2
//! of #93). `tests/fixtures/qualified_calls.osk` attenuates and asserts
//! trust through `Type.method(receiver, ...)` and through `Type.method`
//! taken as a value, and the report lists each like the same method call.
//! `tests/fixtures/qualified_removed.osk` calls the removed path methods
//! qualified, and each gets `E0273`.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test qualified_calls_golden`
//! and review the diff.

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
fn the_human_report_lists_qualified_attenuations_and_audit_sites() {
    let (stdout, ok) = check("qualified_calls.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_qualified_calls.txt");
}

#[test]
fn the_json_report_lists_qualified_attenuations_and_audit_sites() {
    let (stdout, ok) = check("qualified_calls.osk", Some("--format=json"));
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_qualified_calls.json");
}

#[test]
fn a_qualified_removed_path_method_is_e0273() {
    let (stdout, ok) = check("qualified_removed.osk", Some("--format=json"));
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_golden(&stdout, "check_qualified_removed.json");
}
