//! Unused authority in `cap struct` fields on the command line (ticket
//! #369, ADR 0016): `oskac check --release` rejects a capability-bearing
//! field that no code reads, with `E0297`, in human and JSON output; a
//! plain `oskac check` says nothing; `oskac fix --release` does not apply
//! the `maybe` fix; and `oskac explain E0297` describes it offline.
//!
//! Each case writes its program into a fresh scratch directory, runs the
//! real binary there, and compares the exit code, stdout, stderr and the
//! program afterwards with `tests/golden/unused_cap_fields/<case>.txt`.
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test unused_cap_fields_cli` and
//! review the diff.

mod common;

use common::golden::assert_run_and_file;
use common::portable::Scratch;
use std::path::Path;
use std::process::{Command, Output};

/// `Tools` holds a `Net` that nothing reads; its `Clock` is read by
/// destructuring in `stamp`, and its `Stdout` by a field access.
const PROGRAM: &str = "\
cap struct Tools {
    out: Stdout,
    clock: Clock,
    net: Net,
}

fn stamp(tools: Tools) -> Int {
    match tools {
        Tools(out: _, clock: c, net: _) => c.now_millis()
    }
}

fn main(out: Stdout, clock: Clock, net: Net) -> Result<(), IoError> {
    let tools = Tools(out: out, clock: clock, net: net)
    let n = stamp(tools)
    tools.out.println(\"{n}\")
}
";

fn scratch(name: &str) -> Scratch {
    let dir = Scratch::new("unused_cap_fields_cli", name);
    std::fs::write(dir.join("app.osk"), PROGRAM).expect("should write the program");
    dir
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// Run `oskac args` in a scratch directory and compare the exit code,
/// stdout, stderr and the program afterwards with the golden file `name`.
fn assert_golden(name: &str, args: &[&str]) -> Output {
    let dir = scratch(name);
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/unused_cap_fields")
        .join(format!("{name}.txt"));
    assert_run_and_file(&golden, &dir, args, "app.osk")
}

#[test]
fn the_release_check_reports_an_unread_capability_field() {
    let output = assert_golden("check_release", &["check", "app.osk", "--release"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_release_check_reports_an_unread_capability_field_as_json() {
    let output = assert_golden(
        "check_release_json",
        &["check", "app.osk", "--release", "--format=json"],
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn an_ordinary_check_reports_no_unread_field() {
    let output = assert_golden("check", &["check", "app.osk"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn fix_with_release_leaves_an_unread_field() {
    let output = assert_golden("fix", &["fix", "app.osk", "--release"]);
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn explain_describes_the_code_with_its_fix() {
    let output = oskac(Path::new(env!("CARGO_MANIFEST_DIR")), &["explain", "E0297"]);
    assert_eq!(output.status.code(), Some(0));
    let human = text(&output.stdout);
    assert!(
        human.contains("\nFix: Remove the field.")
            && human.contains("\nFix applicability: maybe.\n")
            && human.contains("net: Net,"),
        "{human}"
    );
}
