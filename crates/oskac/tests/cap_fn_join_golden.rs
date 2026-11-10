//! `oskac check` on the `cap fn` join (#109).
//! `tests/fixtures/cap_fn_join.osk` puts a plain `fn` and a capturing
//! closure in one unannotated list and checks cleanly.
//! `tests/fixtures/cap_fn_join_plain.osk` passes that list where a
//! `List<fn() -> Int>` is expected and gets `E0270`, naming the joined
//! type.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test cap_fn_join_golden` and
//! review the diff.

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
fn a_list_joining_a_plain_fn_and_a_cap_fn_checks_cleanly() {
    let (stdout, ok) = check("cap_fn_join.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_cap_fn_join.txt");
}

#[test]
fn a_joined_list_passed_as_plain_fns_is_e0270() {
    let (stdout, ok) = check("cap_fn_join_plain.osk", Some("--format=json"));
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_golden(&stdout, "check_cap_fn_join_plain.json");
}
