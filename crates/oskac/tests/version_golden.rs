//! End-to-end tests of `oskac --version` (spec #70, ticket #82): plain text
//! by default and a JSON object with `--format=json`, which the agent guide
//! and the agent-setup prompt rely on.

use std::process::{Command, Output};

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

#[test]
fn version_prints_plain_text_by_default() {
    let output = oskac(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    assert_eq!(
        text(&output.stdout),
        format!("oskac {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn version_prints_json_with_format_json() {
    let output = oskac(&["--version", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    assert_eq!(
        text(&output.stdout),
        format!(
            "{{\"name\":\"oskac\",\"version\":\"{}\"}}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn short_version_flag_also_prints_json() {
    let output = oskac(&["-V", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stdout).starts_with("{\"name\":\"oskac\""));
}
