// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s typed queries through `oskac check` (#852): a field the
//! record does not have, a value of the wrong type and a write on a
//! read-only store are check errors, and the golden file
//! `tests/golden/check_store_queries.txt` holds what the CLI prints.
//!
//! To regenerate it after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test store_queries_golden` and
//! review the diff.

use std::path::Path;
use std::process::Command;

#[test]
fn the_cli_reports_a_bad_query_and_a_write_on_a_read_only_store() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["check", "crates/oskac/tests/fixtures/store_queries_bad.osk"])
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    assert!(!output.status.success(), "the fixture should not check");
    let text = format!(
        "{}{}",
        String::from_utf8(output.stderr).expect("UTF-8 output"),
        String::from_utf8(output.stdout).expect("UTF-8 output")
    );
    assert!(
        text.contains("did you mean `qty`"),
        "suggests the field: {text}"
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/check_store_queries.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &text).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        text == expected,
        "differs from {}; rerun with UPDATE_GOLDEN=1 if intended\n{text}",
        golden.display()
    );
}
