//! End-to-end test of `oskac test` on a fixture file: the per-test lines,
//! the summary, the JSON report, and the exit code.

use std::path::Path;
use std::process::{Command, Output};

const FIXTURE: &str = "crates/oskac/tests/fixtures/tested.osk";

fn oskac_test(args: &[&str]) -> (Output, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("test")
        .args(args)
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    let stdout = String::from_utf8(output.stdout.clone()).expect("output should be UTF-8");
    (output, stdout)
}

#[test]
fn reports_each_test_and_a_summary_and_fails_when_a_test_fails() {
    let (output, stdout) = oskac_test(&[FIXTURE]);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[..5],
        [
            "test double doubles ... ok",
            "test greet prints a greeting ... ok",
            "test double is wrong here ... FAILED",
            "test dividing by zero panics ... FAILED",
            "test the clock has moved on ... skipped (#[integration]; run with --integration)",
        ]
    );
    assert!(
        stdout.contains(&format!("{FIXTURE}: error[P0011]: "))
            && stdout.contains("actual 6, expected 7")
            && stdout.contains(&format!("{FIXTURE}: error[P0004]: ")),
        "the failures are listed: {stdout}"
    );
    assert_eq!(
        lines.last(),
        Some(&"5 tests: 2 passed, 2 failed, 1 skipped")
    );
}

#[test]
fn integration_runs_the_integration_tests_on_the_real_os() {
    let (_, stdout) = oskac_test(&[FIXTURE, "--integration"]);
    assert!(
        stdout.contains("test the clock has moved on ... ok"),
        "{stdout}"
    );
    assert!(
        stdout.ends_with("5 tests: 3 passed, 2 failed, 0 skipped\n"),
        "{stdout}"
    );
}

#[test]
fn json_reports_one_object_with_each_test() {
    let (output, stdout) = oskac_test(&[FIXTURE, "--format=json"]);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(
        stdout.starts_with(
            "{\"tests\":[{\"name\":\"double doubles\",\"status\":\"passed\",\"failure\":null},"
        ),
        "{stdout}"
    );
    assert!(
        stdout.ends_with("\"passed\":2,\"failed\":2,\"skipped\":1,\"diagnostics\":[]}\n"),
        "{stdout}"
    );
}
