// SPDX-License-Identifier: Apache-2.0
//! The `verdict` of `oskac check --format=json` (#448): always the last
//! key of the check object, and always the same answer as the exit code:
//! `ok` (0), `diagnostics` (1) or `widened` (3), so that an embedder
//! branches on one value.
//!
//! Each program case is a file under `tests/fixtures/check_verdict/`, and
//! its exit code and standard output are a golden file under
//! `tests/golden/check_verdict/`. The baseline cases use the authority
//! diff fixtures, whose full output `authority_baseline.rs` keeps.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test check_verdict` and review
//! the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::portable::Scratch;

const FIXTURES: &str = "crates/oskac/tests/fixtures/check_verdict";
const DIFF_FIXTURES: &str = "crates/oskac/tests/fixtures/authority_diff";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .arg("--no-cache")
        .current_dir(root())
        .output()
        .expect("oskac should run")
}

fn exit(output: &Output) -> i32 {
    output.status.code().expect("oskac should exit with a code")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("output should be UTF-8")
}

/// Check the program case `case` with `--format=json`, compare its exit
/// code and standard output with the golden file, and return the exit
/// code.
fn golden_case(case: &str) -> i32 {
    let output = oskac(&["check", &format!("{FIXTURES}/{case}.osk"), "--format=json"]);
    let actual = format!("exit: {}\n--- stdout\n{}", exit(&output), stdout(&output));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/check_verdict")
        .join(format!("{case}.json"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "the output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
    exit(&output)
}

/// The check object in `output` ends with `verdict`, as its last key.
fn ends_with_verdict(output: &Output, verdict: &str) -> bool {
    stdout(output).ends_with(&format!(",\n  \"verdict\": \"{verdict}\"\n}}\n"))
}

#[test]
fn a_clean_program_has_the_verdict_ok_and_exit_code_0() {
    assert_eq!(golden_case("clean"), 0);
}

#[test]
fn a_program_with_diagnostics_has_the_verdict_diagnostics_and_exit_code_1() {
    assert_eq!(golden_case("diagnostics"), 1);
}

#[test]
fn a_parse_error_has_the_verdict_diagnostics_and_exit_code_1() {
    assert_eq!(golden_case("parse_error"), 1);
}

/// A scratch directory of its own for the test `name`.
fn scratch_dir(name: &str) -> Scratch {
    Scratch::new("oska-check-verdict", name)
}

/// Check the authority diff case `case`'s `after.osk` with `--format=json`
/// against the baseline of its `before.osk`.
fn against_baseline(case: &str) -> Output {
    let dir = scratch_dir(case);
    let baseline = dir.join("authority.json");
    let flag = format!("--update-baseline={}", baseline.display());
    let written = oskac(&[
        "check",
        &format!("{DIFF_FIXTURES}/{case}/before.osk"),
        &flag,
    ]);
    assert_eq!(
        exit(&written),
        0,
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    let flag = format!("--baseline={}", baseline.display());
    oskac(&[
        "check",
        &format!("{DIFF_FIXTURES}/{case}/after.osk"),
        &flag,
        "--format=json",
    ])
}

#[test]
fn each_way_the_baseline_comparison_fails_has_the_verdict_widened_and_exit_code_3() {
    // Widened, became impure, a new audit site, and an added function
    // with a new kind.
    for case in [
        "gains_net",
        "becomes_impure",
        "audit_added",
        "helper_new_kind",
    ] {
        let output = against_baseline(case);
        assert_eq!(exit(&output), 3, "{case}");
        assert!(
            ends_with_verdict(&output, "widened"),
            "{case}: {}",
            stdout(&output)
        );
        let shown = stdout(&output);
        let diff = shown.find("\"authority_diff\"").expect("the diff");
        assert!(
            diff < shown.find("\"verdict\"").expect("the verdict"),
            "{case}"
        );
    }
}

#[test]
fn a_baseline_comparison_that_passes_has_the_verdict_ok_and_exit_code_0() {
    for case in ["narrows", "audit_removed", "helper_within"] {
        let output = against_baseline(case);
        assert_eq!(exit(&output), 0, "{case}");
        assert!(
            ends_with_verdict(&output, "ok"),
            "{case}: {}",
            stdout(&output)
        );
    }
}

#[test]
fn diagnostics_against_a_baseline_have_the_verdict_diagnostics_and_exit_code_1() {
    let dir = scratch_dir("diagnostics_baseline");
    let flag = format!("--baseline={}", dir.join("absent.json").display());
    let output = oskac(&[
        "check",
        &format!("{FIXTURES}/diagnostics.osk"),
        &flag,
        "--format=json",
    ]);
    assert_eq!(exit(&output), 1);
    assert!(
        ends_with_verdict(&output, "diagnostics"),
        "{}",
        stdout(&output)
    );
}

/// `oskac check` of a clean program against `baseline`, with `--format=json`.
fn clean_against(baseline: &Path) -> Output {
    let flag = format!("--baseline={}", baseline.display());
    oskac(&[
        "check",
        &format!("{FIXTURES}/clean.osk"),
        &flag,
        "--format=json",
    ])
}

#[test]
fn an_unreadable_baseline_is_an_error_with_exit_code_1_and_no_check_object() {
    let dir = scratch_dir("unreadable");
    let output = clean_against(&dir.join("absent.json"));
    assert_eq!(exit(&output), 1);
    assert_eq!(stdout(&output), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: cannot read the baseline"),
        "{stderr}"
    );
}

#[test]
fn a_malformed_baseline_is_an_error_with_exit_code_1_and_no_check_object() {
    let dir = scratch_dir("malformed");
    for (name, text) in [
        ("not_json.json", "not json"),
        ("format_2.json", "{\"format\": 2, \"functions\": []}"),
        ("no_format.json", "{\"functions\": []}"),
    ] {
        let baseline = dir.join(name);
        std::fs::write(&baseline, text).expect("should write the baseline");
        let output = clean_against(&baseline);
        assert_eq!(exit(&output), 1, "{name}");
        assert_eq!(stdout(&output), "", "{name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.starts_with("error: the baseline"),
            "{name}: {stderr}"
        );
    }
}

#[test]
fn a_baseline_that_cannot_be_written_is_an_error_with_exit_code_1_and_no_check_object() {
    let dir = scratch_dir("unwritable");
    // A directory where the baseline should be: it cannot be replaced.
    let baseline = dir.join("authority.json");
    std::fs::create_dir(&baseline).expect("should make the directory");
    let flag = format!("--update-baseline={}", baseline.display());
    let output = oskac(&[
        "check",
        &format!("{FIXTURES}/clean.osk"),
        &flag,
        "--format=json",
    ]);
    assert_eq!(exit(&output), 1);
    assert_eq!(stdout(&output), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: cannot write the baseline"),
        "{stderr}"
    );
}

#[test]
fn a_written_baseline_has_the_verdict_ok_and_exit_code_0() {
    let dir = scratch_dir("written");
    let flag = format!("--update-baseline={}", dir.join("authority.json").display());
    let output = oskac(&[
        "check",
        &format!("{FIXTURES}/clean.osk"),
        &flag,
        "--format=json",
    ]);
    assert_eq!(exit(&output), 0);
    assert!(ends_with_verdict(&output, "ok"), "{}", stdout(&output));
}

#[test]
fn a_package_that_does_not_load_has_the_verdict_diagnostics_and_exit_code_1() {
    let dir = scratch_dir("package");
    std::fs::write(dir.join("oska.toml"), "not a manifest\n").expect("should write");
    let path = dir.display().to_string();
    let output = oskac(&["check", &path, "--format=json"]);
    assert_eq!(
        exit(&output),
        1,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        ends_with_verdict(&output, "diagnostics"),
        "{}",
        stdout(&output)
    );
}
