// SPDX-License-Identifier: Apache-2.0
//! `std.store` end to end through `oskac run` (#848): a small program keeps
//! notes in a directory. It runs twice, so the second run shows that the
//! first run's records survived a restart, and it runs natively and with
//! `--interpret`, which must print the same. The output is the golden file
//! `tests/golden/store_notes.txt`.
//!
//! To regenerate it after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test store_golden` and review the
//! diff.

mod common;

use common::portable::Scratch;
use std::path::Path;
use std::process::Command;

fn run(dir: &Path, interpret: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command.arg("run");
    if interpret {
        command.arg("--interpret");
    }
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store_notes.osk");
    let output = command
        .arg(program)
        .current_dir(dir)
        .output()
        .expect("oskac should run");
    assert!(
        output.status.success(),
        "oskac run failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn two_runs(interpret: bool, name: &str) -> String {
    let scratch = Scratch::new("oska-store", name);
    std::fs::create_dir(scratch.join("notes")).expect("the notes directory");
    let first = run(&scratch, interpret);
    let second = run(&scratch, interpret);
    let log = std::fs::read_to_string(scratch.join("notes/log")).expect("the log");
    assert_eq!(log.lines().count(), 8, "four records a run: {log}");
    format!("first run\n{first}second run\n{second}")
}

#[test]
fn a_small_program_keeps_notes_across_runs_and_both_backends_agree() {
    let native = two_runs(false, "native");
    let interpreted = two_runs(true, "interpreted");
    assert_eq!(native, interpreted, "the backends print the same");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/store_notes.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &native).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        native == expected,
        "differs from {}; rerun with UPDATE_GOLDEN=1 if intended\n{native}",
        golden.display()
    );
}
