//! Helpers shared by the run-limits tests (`run_limits.rs`,
//! `run_limits_steps.rs`): running `oskac` in a scratch directory, and
//! golden files under `tests/golden/run_limits/`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::portable::Scratch;

/// What `oskac <args>` does, run in `dir`.
pub fn oskac_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env_remove("OSKA_CACHE_DIR")
        .output()
        .expect("oskac should run")
}

/// The golden file `name`.
pub fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/run_limits")
        .join(name)
}

/// Assert that `actual` is the content of the golden file `name`.
pub fn assert_golden(name: &str, actual: &str) {
    let golden = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("the golden directory");
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// What `oskac <args>` printed and its exit code, as one text: the
/// command line, then stdout, stderr and the exit code.
pub fn transcript(dir: &Path, args: &[&str]) -> String {
    let output = oskac_in(dir, args);
    format!(
        "$ oskac {}\n--- stdout\n{}--- stderr\n{}--- exit {:?}\n\n",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        output.status.code()
    )
}

/// A scratch directory holding `main.osk` with `src`.
pub fn program(name: &str, src: &str) -> Scratch {
    let dir = Scratch::new("run-limits", name);
    std::fs::write(dir.join("main.osk"), src).expect("should write main.osk");
    dir
}

pub const HELLO: &str = "fn main(io: Stdout) {\n    let _ = io.println(\"hello\")\n}\n";

/// What `oskac <args>` does, run in `dir` with `src` on its standard
/// input, as `oskac run -` reads a program.
pub fn oskac_with_stdin(dir: &Path, args: &[&str], src: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env_remove("OSKA_CACHE_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oskac should start");
    child
        .stdin
        .take()
        .expect("a stdin")
        .write_all(src.as_bytes())
        .expect("should write the program");
    child.wait_with_output().expect("oskac should finish")
}
