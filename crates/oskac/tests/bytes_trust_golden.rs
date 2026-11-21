//! `oskac check` on trust on bytes (#223).
//! `tests/fixtures/bytes_trust.osk` passes a slice of bytes read through
//! a capability where trusted `Bytes` is required, and gets `E0300`.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test bytes_trust_golden` and
//! review the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `oskac check` on the fixture `name`, with `format` if given: what it
/// wrote (stderr, where the human format puts diagnostics, then stdout),
/// and whether it succeeded.
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
    let stderr = String::from_utf8(output.stderr).expect("output should be UTF-8");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    (format!("{stderr}{stdout}"), output.status.success())
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
fn untrusted_bytes_in_a_trusted_slot_is_e0300() {
    let (stdout, ok) = check("bytes_trust.osk", None);
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert!(stdout.contains("E0300"), "{stdout}");
    assert_golden(&stdout, "check_bytes_trust.txt");
}

#[test]
fn untrusted_bytes_in_a_trusted_slot_is_e0300_as_json() {
    let (stdout, ok) = check("bytes_trust.osk", Some("--format=json"));
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_golden(&stdout, "check_bytes_trust.json");
}
