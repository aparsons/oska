//! End-to-end tests of `oskac run`: the example programs print what they
//! should, and a program with checker diagnostics is refused.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oskac_run(args: &[&str]) -> Output {
    oskac_run_in(&repo_root(), args)
}

fn oskac_run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("run")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// A new, empty directory under the test temp directory.
fn fresh_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("run_golden_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the temp directory");
    dir
}

/// What `output` wrote to stdout and stderr, and its exit code.
fn summary(output: &Output) -> (String, String, Option<i32>) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

#[test]
fn hello_example_prints_its_greeting() {
    assert_eq!(
        summary(&oskac_run(&["examples/hello.osk"])),
        ("Hello from Oska!\n".to_string(), String::new(), Some(0))
    );
}

#[test]
fn a_program_with_diagnostics_is_refused_and_they_are_printed() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.printline(\"hi\")\n}\n";
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("refused.osk");
    std::fs::write(&path, src).expect("should write the source file");
    let path = path.to_str().expect("the path is UTF-8");
    let output = oskac_run(&[path]);
    let stderr = String::from_utf8(output.stderr).expect("output should be UTF-8");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        stderr.starts_with(&format!("{path}:2:8: ")) && stderr.contains("E0228"),
        "the diagnostic is printed as `oskac check` prints it: {stderr}"
    );
}

/// The capabilities example, run from `dir` with `args` after `--`.
fn run_capabilities(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    let example = repo_root().join("examples/capabilities.osk");
    let example = example.to_str().expect("the path is UTF-8");
    let mut argv = vec![example, "--"];
    argv.extend_from_slice(args);
    summary(&oskac_run_in(dir, &argv))
}

#[test]
fn capabilities_example_counts_the_lines_of_a_real_file() {
    let dir = fresh_dir("count");
    std::fs::write(dir.join("notes.txt"), "one\ntwo\nthree\n").expect("should write the file");
    assert_eq!(
        run_capabilities(&dir, &["notes.txt"]),
        ("lines: 3\n".to_string(), String::new(), Some(0))
    );
}

#[test]
fn capabilities_example_without_a_file_prints_its_usage_and_exits_1() {
    let dir = fresh_dir("usage");
    assert_eq!(
        run_capabilities(&dir, &[]),
        (
            String::new(),
            "usage: capabilities <file>\n".to_string(),
            Some(1)
        )
    );
}

#[test]
fn capabilities_example_reports_a_missing_file_and_exits_1() {
    let dir = fresh_dir("missing");
    assert_eq!(
        run_capabilities(&dir, &["absent.txt"]),
        (String::new(), "cannot read the file\n".to_string(), Some(1))
    );
}

#[test]
fn capabilities_example_cannot_read_outside_the_current_directory() {
    // The example restricts its filesystem to `.`, so a file next to the
    // directory exists but is out of reach.
    let outer = fresh_dir("confined");
    let dir = outer.join("work");
    std::fs::create_dir_all(&dir).expect("should create the working directory");
    std::fs::write(outer.join("secret.txt"), "hidden\n").expect("should write the file");
    assert_eq!(
        run_capabilities(&dir, &["../secret.txt"]),
        (String::new(), "cannot read the file\n".to_string(), Some(1))
    );
}
