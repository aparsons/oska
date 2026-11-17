//! Helpers shared by the harness's CLI tests. Each test runs the
//! `oska-eval` binary on files and checks what it prints and its exit code.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// What one run of `oska-eval` printed, and how it exited.
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `oska-eval` with `args`, in the crate's directory.
pub fn harness(args: &[&str]) -> Outcome {
    let output = Command::new(env!("CARGO_BIN_EXE_oska-eval"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("oska-eval should start");
    Outcome {
        code: output
            .status
            .code()
            .expect("oska-eval should exit, not be killed"),
        stdout: String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    }
}

/// The `oskac` this workspace builds, found in the nearest directory above
/// this test binary that holds one. `cargo test` builds it before running
/// any test, because `oskac`'s own tests need it; on its own, this crate's
/// tests need `cargo build -p oskac` first.
pub fn oskac() -> String {
    let test = std::env::current_exe().expect("the test should know its path");
    let name = format!("oskac{}", std::env::consts::EXE_SUFFIX);
    let oskac = test
        .ancestors()
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| {
            panic!(
                "no oskac above {}: run `cargo build -p oskac` first",
                test.display()
            )
        });
    oskac
        .to_str()
        .expect("the path should be UTF-8")
        .to_string()
}

/// Compares `actual` with the golden file `tests/golden/<name>`, or rewrites
/// it when `UPDATE_GOLDEN` is set.
pub fn assert_golden(name: &str, actual: &str) {
    let golden: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("the golden file should exist");
    assert!(
        actual == expected,
        "the output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}
