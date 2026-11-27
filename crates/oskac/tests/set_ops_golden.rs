// SPDX-License-Identifier: Apache-2.0
//! `oskac check` on the set operations (#803, spec #798, ADR 0032): a
//! `Secret` item is refused with `E0351` and `E0354`, and a `Float` item
//! with `E0266`, for `union`, `intersection` and `difference` as for the
//! core. The reports are golden files in `tests/golden/set_ops/`.
//!
//! To regenerate them after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test set_ops_golden` and review
//! the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `oskac check --format=json` on the fixture `name`: its stdout, and
/// whether it succeeded.
fn check(name: &str) -> (String, bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args([
            "check",
            &format!("crates/oskac/tests/fixtures/set_ops/{name}"),
            "--format=json",
        ])
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    (stdout, output.status.success())
}

fn assert_golden(actual: &str, name: &str) {
    let golden: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/set_ops")
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
fn a_secret_item_is_refused_for_every_set_operation() {
    let (stdout, ok) = check("secret_item.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    // One `E0351` and one `E0354` for the type, and one pair for each of
    // the three operations.
    for code in ["E0351", "E0354"] {
        assert_eq!(
            stdout.matches(&format!("\"code\":\"{code}\"")).count(),
            4,
            "{code}: {stdout}"
        );
    }
    assert_golden(&stdout, "secret_item.json");
}

#[test]
fn a_float_item_is_refused_for_every_set_operation() {
    let (stdout, ok) = check("float_item.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_eq!(stdout.matches("\"code\":\"E0266\"").count(), 4, "{stdout}");
    assert_eq!(stdout.matches("\"code\":\"E0").count(), 4, "{stdout}");
    assert_golden(&stdout, "float_item.json");
}
