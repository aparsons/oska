//! The one end-to-end test of `oskac check`: the JSON report for
//! `examples/capabilities.osk` must match the checked-in golden file. The
//! example is clean, so the report is its authority alone.
//!
//! To regenerate the golden file after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test check_golden` and review the
//! diff.

use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn capabilities_example_check_matches_golden_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["check", "examples/capabilities.osk", "--format=json"])
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    let actual = String::from_utf8(output.stdout).expect("output should be UTF-8");
    assert!(
        output.status.success(),
        "oskac check reported diagnostics: {actual}{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let golden: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/check_capabilities.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "`oskac check --format=json examples/capabilities.osk` differs from {}; \
         rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}
