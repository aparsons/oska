// SPDX-License-Identifier: Apache-2.0
//! The `Acme` capability in `oskac check`'s authority report (spec #824,
//! ticket #840): each `restrict_to` of `Acme` is listed under its function
//! as `Acme -> AcmeDomain: restrict_to "<domain>"`, and the JSON report has
//! it in its `attenuations` array. The fixture
//! `tests/fixtures/acme_capability.osk` narrows with a literal, a module
//! constant and a computed domain.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test acme_golden` and review the
//! diff.

use std::path::{Path, PathBuf};
use std::process::Command;

fn check(format: Option<&str>) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command
        .args(["check", "crates/oskac/tests/fixtures/acme_capability.osk"])
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
fn the_human_report_lists_acme_domains_under_their_function() {
    assert_golden(&check(None), "check_acme_capability.txt");
}

#[test]
fn the_json_report_lists_acme_domains_in_its_attenuations() {
    assert_golden(&check(Some("--format=json")), "check_acme_capability.json");
}
