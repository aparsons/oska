//! `oskac check` on contracts in the wrong place (#108).
//! `tests/fixtures/contract_placement.osk` has a contract on a trait impl's
//! method (`E0218`), one on `main` (`E0219`), and a `?` inside a condition
//! (`E0282`); each diagnostic spans its clause or the `?`.
//!
//! To regenerate the golden file after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test contract_placement_golden`
//! and review the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn contracts_in_the_wrong_place_are_reported_at_their_clauses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args([
            "check",
            "crates/oskac/tests/fixtures/contract_placement.osk",
            "--format=json",
        ])
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    assert!(!output.status.success(), "the fixture should fail to check");
    let actual = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let golden: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/check_contract_placement.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "the report differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}
