//! Unused authority on the command line (ticket #368, ADR 0016): `oskac
//! check --release` rejects a capability-bearing parameter that its
//! function never uses, with `E0296`, in human and JSON output; a plain
//! `oskac check` says nothing; and `oskac fix --release` applies the
//! `machine` fix on `main` but not the `maybe` fix anywhere else.
//!
//! Each case writes its program into a fresh scratch directory, runs the
//! real binary there, and compares the exit code, stdout, stderr and the
//! files afterwards with `tests/golden/unused_authority/<case>.txt`. To
//! regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test unused_authority_cli` and
//! review the diff.

mod common;

use common::portable::Scratch;
use std::path::Path;
use std::process::{Command, Output};

/// `main` takes `Net` and never uses it; `count` takes the `Clock` that
/// `main` passes on, and never uses it.
const PROGRAM: &str = "\
fn count(clock: Clock, n: Int) -> Int {
    n + 1
}

fn main(out: Stdout, net: Net, clock: Clock) -> Result<(), IoError> {
    let n = count(clock, 1)
    out.println(\"{n}\")
}
";

/// `main` alone over-asks, so its `machine` fix is all there is.
const MAIN_ONLY: &str = "\
fn main(out: Stdout, net: Net) -> Result<(), IoError> {
    out.println(\"hi\")
}
";

/// Only `count` over-asks, and its fix is `maybe`.
const HELPER_ONLY: &str = "\
fn count(clock: Clock, n: Int) -> Int {
    n + 1
}

fn main(out: Stdout, clock: Clock) -> Result<(), IoError> {
    let n = count(clock, 1)
    out.println(\"{n}\")
}
";

fn scratch(name: &str, program: &str) -> Scratch {
    let dir = Scratch::new("unused_authority_cli", name);
    std::fs::write(dir.join("app.osk"), program).expect("should write the program");
    dir
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

/// Run `oskac args` in `dir` and compare the exit code, stdout, stderr
/// and the program afterwards with the golden file `name`.
fn assert_golden(name: &str, dir: &Path, args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run");
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- file app.osk\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        text(&output.stdout),
        text(&output.stderr),
        std::fs::read_to_string(dir.join("app.osk")).expect("should read the program"),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/unused_authority")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
    output
}

#[test]
fn the_release_check_reports_unused_authority() {
    let dir = scratch("check_release", PROGRAM);
    let output = assert_golden("check_release", &dir, &["check", "app.osk", "--release"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_release_check_reports_unused_authority_as_json() {
    let dir = scratch("check_release_json", PROGRAM);
    let output = assert_golden(
        "check_release_json",
        &dir,
        &["check", "app.osk", "--release", "--format=json"],
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn an_ordinary_check_reports_no_unused_authority() {
    let dir = scratch("check", PROGRAM);
    let output = assert_golden("check", &dir, &["check", "app.osk"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn fix_with_release_removes_an_unused_parameter_of_main() {
    let dir = scratch("fix_main", MAIN_ONLY);
    let output = assert_golden("fix_main", &dir, &["fix", "app.osk", "--release"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn fix_with_release_leaves_an_unused_parameter_of_any_other_function() {
    let dir = scratch("fix_helper", HELPER_ONLY);
    let output = assert_golden("fix_helper", &dir, &["fix", "app.osk", "--release"]);
    assert_ne!(output.status.code(), Some(0));
}
