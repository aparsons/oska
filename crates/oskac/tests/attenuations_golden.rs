//! The attenuations in `oskac check`'s authority report (#98): the human
//! report lists them under their function, and the JSON report has them
//! in a top-level `attenuations` array. The fixture
//! `tests/fixtures/attenuations.osk` narrows capabilities every way the
//! report distinguishes: a literal root, a narrower literal root, a root
//! computed at runtime, narrowing with a `Path` built from untrusted text,
//! `read_only`, `write_only`, and `restrict_to("/")`. (A module constant
//! as a root is covered in oska-check's tests: a body cannot name a
//! constant yet, #83.)
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test attenuations_golden` and
//! review the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

fn check(format: Option<&str>) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command
        .args(["check", "crates/oskac/tests/fixtures/attenuations.osk"])
        .current_dir(&root);
    if let Some(format) = format {
        command.arg(format);
    }
    let output = command.output().expect("oskac should run");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    assert!(
        output.status.success(),
        "oskac check reported diagnostics: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
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
fn the_human_report_lists_attenuations_under_their_function() {
    assert_golden(&check(None), "check_attenuations.txt");
}

#[test]
fn the_json_report_has_an_attenuations_array() {
    assert_golden(&check(Some("--format=json")), "check_attenuations.json");
}
