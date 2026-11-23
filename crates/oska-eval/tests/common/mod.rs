// SPDX-License-Identifier: Apache-2.0
//! Helpers shared by the harness's CLI tests. Each test runs the
//! `oska-eval` binary on files and checks what it prints and its exit code.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment that makes `home` the home directory on every platform:
/// `HOME`, `USERPROFILE` and `XDG_CONFIG_HOME` (beneath it), as
/// `oskac`'s tests' `set_home` sets them. Pass it to [`harness_env`].
pub fn home_env(home: &str) -> Vec<(&'static str, String)> {
    vec![
        ("HOME", home.to_string()),
        ("USERPROFILE", home.to_string()),
        ("XDG_CONFIG_HOME", format!("{home}/.config")),
    ]
}

/// What one run of `oska-eval` printed, and how it exited.
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `oska-eval` with `args`, in the crate's directory.
pub fn harness(args: &[&str]) -> Outcome {
    harness_env(args, &[])
}

/// Runs `oska-eval` with `args`, in the crate's directory, with the
/// environment variables `env` set too.
pub fn harness_env(args: &[&str], env: &[(&str, &str)]) -> Outcome {
    let output = Command::new(env!("CARGO_BIN_EXE_oska-eval"))
        .args(args)
        .envs(env.iter().copied())
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

/// The `oskac` the tests run: the one `OSKA_EVAL_OSKAC` names, or else this
/// workspace's, built fresh (once per test binary) so `cargo test -p
/// oska-eval` alone never uses a stale one or finds none. Cargo itself says
/// where the build put it, so neither the profile nor the target directory
/// is guessed from a path.
pub fn oskac() -> String {
    static BUILT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| match std::env::var("OSKA_EVAL_OSKAC") {
            Ok(path) => path,
            Err(_) => build_oskac(),
        })
        .clone()
}

fn build_oskac() -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .args(["build", "--quiet", "-p", "oskac", "--message-format=json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    // A test build without debug assertions is a release build.
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    let output = build.output().expect("cargo should start");
    assert!(
        output.status.success(),
        "cargo build -p oskac failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| oska_syntax::parse_json(line).ok())
        .filter(|message| {
            message.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
                && message
                    .get("target")
                    .and_then(|t| t.get("name"))
                    .and_then(|n| n.as_str())
                    == Some("oskac")
        })
        .find_map(|message| {
            message
                .get("executable")
                .and_then(|e| e.as_str())
                .map(str::to_string)
        })
        .expect("cargo should report the oskac it built")
}

/// A directory in the system's temporary directory, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("oska-eval-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("should create a scratch directory");
        Scratch(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn str(&self) -> &str {
        self.0.to_str().expect("the path should be UTF-8")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies every file beneath `from` to the same relative path beneath `to`.
pub fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("should read the directory") {
        let entry = entry.expect("should read the entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).expect("should create the directory");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("should copy the file");
        }
    }
}

/// Compares `actual` with the golden file `tests/golden/<name>`, or rewrites
/// it when `UPDATE_GOLDEN` is set.
pub fn assert_golden(name: &str, actual: &str) {
    assert_file(&format!("tests/golden/{name}"), actual);
}

/// Compares `actual` with the file at `relative` (from the crate's
/// directory), or rewrites it when `UPDATE_GOLDEN` is set.
pub fn assert_file(relative: &str, actual: &str) {
    let golden: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
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
