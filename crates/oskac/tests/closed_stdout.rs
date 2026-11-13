//! `oskac`'s own output into a pipe whose reader has gone, as in
//! `oskac check --format=json | head -1`: it ends quietly with 0, as a
//! program it runs does (#86), rather than panicking.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

/// Run `oskac args` with its stdout in a pipe whose reader reads one line
/// and closes it: what it wrote to stderr, and its exit code.
fn closed_early(dir: &std::path::Path, args: &[&str]) -> (String, Option<i32>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oskac should start");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut first = String::new();
    stdout.read_line(&mut first).expect("should read a line");
    drop(stdout);
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("should read stderr");
    let status = child.wait().expect("oskac should end");
    (stderr, status.code())
}

#[test]
fn oskac_output_into_a_closed_pipe_ends_quietly_with_0() {
    let dir = std::env::temp_dir().join(format!("oskac-closed-stdout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("should create the directory");
    // Far more output than a pipe holds, in every form.
    let src: String = (0..4000)
        .map(|i| format!("fn f{i}(x: Int) -> Int {{\n    x + {i}\n}}\n\n"))
        .collect();
    std::fs::write(dir.join("big.osk"), src).expect("should write the program");
    for args in [
        &["parse", "big.osk"][..],
        &["parse", "--format=json", "big.osk"],
        &["check", "--format=json", "big.osk"],
    ] {
        assert_eq!(
            closed_early(&dir, args),
            (String::new(), Some(0)),
            "oskac {args:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
