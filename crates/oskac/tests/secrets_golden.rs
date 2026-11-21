//! `oskac check` on secret integers (#224).
//! `tests/fixtures/secrets/allowed.osk` uses every form a secret allows
//! (`Secret.new`, the wrapping and bitwise operators with a secret or a
//! public operand, rotations, conversions, `ct_eq`, `ct_select`, the
//! `Secret<Bool>` logic, `Secret<Bytes>`'s reads, pushes and comparisons,
//! and a struct field) and checks cleanly, its `audit` listing only the
//! `expose()` calls. `tests/fixtures/secrets/refused.osk` has one
//! function per refusal (`E0353` to `E0356`, and `E0351` on a secret
//! integer and a secret buffer), each with its span and, where there is
//! one, its machine fix.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test secrets_golden` and review
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
            &format!("crates/oskac/tests/fixtures/secrets/{name}"),
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
        .join("tests/golden/secrets")
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
fn every_allowed_form_checks_cleanly_and_audits_each_exposure() {
    let (stdout, ok) = check("allowed.osk");
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_eq!(stdout.matches("\"kind\":\"expose\"").count(), 2, "{stdout}");
    assert_golden(&stdout, "allowed.json");
}

#[test]
fn every_refusal_has_its_code_span_and_fix() {
    let (stdout, ok) = check("refused.osk");
    assert!(!ok, "the fixture should fail to check: {stdout}");
    for (code, count) in [
        ("E0351", 2),
        ("E0353", 9),
        ("E0354", 4),
        ("E0355", 4),
        ("E0356", 1),
    ] {
        assert_eq!(
            stdout.matches(&format!("\"code\":\"{code}\"")).count(),
            count,
            "{code}: {stdout}"
        );
    }
    assert_golden(&stdout, "refused.json");
}
