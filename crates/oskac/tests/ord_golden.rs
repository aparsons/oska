// SPDX-License-Identifier: Apache-2.0
//! `oskac check` on the types that have no `Ord` (spec #660, ticket #664,
//! ADR 0034). `tests/fixtures/ord/refused.osk` has one function per
//! refusal: `cmp` on a `Float`, `Float` and `Secret` as an `Ord` bound, `<`
//! and `cmp` on a secret, and `<` on a list. Each message says why and what
//! to use instead (`total_cmp`, `constant_time_eq`).
//!
//! To regenerate the golden file after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test ord_golden` and review the
//! diff.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `oskac check --format=json` on the fixture `name`: its stdout, and
/// whether it succeeded.
fn check(name: &str) -> (String, bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args([
            "check",
            &format!("crates/oskac/tests/fixtures/ord/{name}"),
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
        .join("tests/golden/ord")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a directory"))
            .expect("should make the golden directory");
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
fn float_and_secret_are_refused_with_the_reason() {
    let (stdout, ok) = check("refused.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    for (code, count) in [("E0228", 1), ("E0265", 1), ("E0356", 3), ("E0223", 1)] {
        assert_eq!(
            stdout.matches(&format!("\"code\":\"{code}\"")).count(),
            count,
            "{code}: {stdout}"
        );
    }
    assert!(stdout.contains("total_cmp"), "{stdout}");
    assert!(stdout.contains("constant_time_eq"), "{stdout}");
    assert_golden(&stdout, "refused.json");
}

#[test]
fn unwrap_and_expect_are_refused_with_the_alternatives() {
    let (stdout, ok) = check("unwrap.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    assert_eq!(stdout.matches("\"code\":\"E0228\"").count(), 2, "{stdout}");
    for alternative in ["`?`", "`unwrap_or`", "`match`"] {
        assert!(stdout.contains(alternative), "{alternative}: {stdout}");
    }
    assert_golden(&stdout, "unwrap.json");
}
