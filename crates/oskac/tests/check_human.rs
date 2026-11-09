//! The human-readable report of `oskac check` shows what the JSON report
//! does: diagnostics, each function's authority, and the audit sites.

use std::path::Path;
use std::process::Command;

#[test]
fn the_human_report_lists_audit_sites() {
    let src = "fn trusted(name: Untrusted<String>) -> String {\n    name.assume_trusted()\n}\n";
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("audited.osk");
    std::fs::write(&path, src).expect("should write the source file");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("oskac should run");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    assert!(output.status.success(), "{stdout}");
    let start = src
        .find("name.assume_trusted()")
        .expect("the call is in the source");
    let end = start + "name.assume_trusted()".len();
    let expected = format!("audit: assume_trusted (bytes {start}..{end})");
    assert!(
        stdout.lines().any(|line| line.ends_with(&expected)),
        "expected a line ending with {expected:?} in {stdout:?}"
    );
}
