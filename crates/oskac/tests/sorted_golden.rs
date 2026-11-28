// SPDX-License-Identifier: Apache-2.0
//! `oskac check` on the sorted collections (#806, spec #888, ADR 0033): a
//! `Float` key or item is refused with `E0265` and a `Secret` one with
//! `E0356`, the codes of the `Ord` refusal of `sorted`. The reports are
//! golden files in `tests/golden/sorted/`.
//!
//! To regenerate them after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test sorted_golden` and review
//! the diff.

mod common;

use common::portable::literal_path;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `oskac check --format=json` on the fixture `name`: its stdout, and
/// whether it succeeded.
fn check(name: &str) -> (String, bool) {
    let root = literal_path(Path::new(env!("CARGO_MANIFEST_DIR")), &["..", ".."]);
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args([
            "check",
            &format!("crates/oskac/tests/fixtures/sorted/{name}"),
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
        .join("tests/golden/sorted")
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
fn a_float_key_or_item_is_refused_with_the_ord_code() {
    let (stdout, ok) = check("float_key.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert!(stdout.contains("\"code\":\"E0265\""), "{stdout}");
    assert_eq!(
        stdout.matches("\"code\":\"E0").count(),
        stdout.matches("\"code\":\"E0265\"").count(),
        "{stdout}"
    );
    assert_golden(&stdout, "float_key.json");
}

#[test]
fn a_secret_key_or_item_is_refused_with_the_ord_code() {
    let (stdout, ok) = check("secret_key.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_eq!(stdout.matches("\"code\":\"E0356\"").count(), 2, "{stdout}");
    assert_eq!(stdout.matches("\"code\":\"E0").count(), 2, "{stdout}");
    assert_golden(&stdout, "secret_key.json");
}
