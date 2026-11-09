//! End-to-end tests of `oskac run`: the example programs print what they
//! should, and a program with checker diagnostics is refused.

use std::path::Path;
use std::process::{Command, Output};

fn oskac_run(args: &[&str]) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("run")
        .args(args)
        .current_dir(&root)
        .output()
        .expect("oskac should run")
}

#[test]
fn hello_example_prints_its_greeting() {
    let output = oskac_run(&["examples/hello.osk"]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stdout, "Hello from Oska!\n");
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
        stderr.starts_with(&format!("{path}: ")) && stderr.contains("E0228"),
        "the diagnostic is printed as `oskac check` prints it: {stderr}"
    );
}
