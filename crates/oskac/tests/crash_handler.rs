// SPDX-License-Identifier: Apache-2.0
//! A bug inside `oskac`, here a failed write to standard output (the one
//! panic `oskac` has on purpose, `print!`'s), ends with one clear line, a
//! request to report it, and exit code 101: the code a Rust panic ends the
//! process with when it unwinds, so no script sees a change. The same holds
//! in a build with `panic = "abort"`.

#![cfg(target_os = "linux")]

mod common;

use common::portable::Scratch;
use std::process::{Command, Stdio};

#[test]
fn a_crash_prints_one_line_and_a_request_and_exits_101() {
    let dir = Scratch::new("oskac", "crash-handler");
    std::fs::write(
        dir.join("p.osk"),
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n",
    )
    .expect("should write the program");
    // `/dev/full` fails every write with "no space left on device".
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full should open");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(&dir)
        .args(["check", "--format=json", "p.osk"])
        .stdout(Stdio::from(full))
        .output()
        .expect("oskac should start");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "stderr: {stderr}");
    assert!(
        stderr.starts_with("oskac: internal error: failed printing to stdout"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("This is a bug in oskac, not in your program. Please report it: https://github.com/aparsons/oska/issues"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("stack backtrace"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
