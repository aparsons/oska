// SPDX-License-Identifier: Apache-2.0
//! `DirWrite.with_lock` (#846, ADR 0008): the scoped exclusive lock on a
//! directory. The checker's half: it types `with_lock` as a method of
//! `DirWrite` alone that takes a `cap fn` of the same capability and gives
//! a `Result`, and the authority report treats it as a narrowing: a
//! function that locks a directory it was given reaches no more than it
//! did, so the authority diff does not call it a widening.

mod common;

use common::{assert_authority, assert_errors, assert_ok, check};
use oska_check::authority_diff::{diff, AuthorityRecord};

#[test]
fn with_lock_takes_a_closure_of_the_directory_and_gives_its_result() {
    assert_ok(
        "fn save(store: DirWrite) -> Result<Int, IoError> {\n    \
         store.with_lock(|dir: DirWrite| 7)\n}",
    );
    assert_ok(
        "fn save(store: DirWrite) -> Result<Result<(), IoError>, IoError> {\n    \
         store.with_lock(|dir: DirWrite| dir.write(Path.new(\"a\"), \"text\"))\n}",
    );
}

#[test]
fn the_closure_may_use_other_capabilities_it_holds() {
    assert_ok(
        "fn save(io: Stdout, store: DirWrite) -> Result<Result<(), IoError>, IoError> {\n    \
         store.with_lock(|dir: DirWrite| io.println(\"holding\"))\n}",
    );
}

#[test]
fn the_closure_is_given_a_directory_write_and_nothing_else() {
    assert_errors(
        "fn save(store: DirWrite) -> Result<Int, IoError> {\n    \
         store.with_lock(|dir: DirRead| 7)\n}",
        &[("E0220", "DirRead")],
    );
}

#[test]
fn only_a_directory_write_can_be_locked() {
    for cap in ["Dir", "DirRead", "FsWrite", "Fs"] {
        let src = format!(
            "fn save(store: {cap}) -> Result<Int, IoError> {{\n    \
             store.with_lock(|dir: DirWrite| 7)\n}}"
        );
        let report = check(&src);
        assert!(
            !report.diagnostics.is_empty(),
            "{cap} should not lock a directory: {src}"
        );
    }
}

#[test]
fn locking_a_directory_reaches_no_more_authority_than_holding_it() {
    assert_authority(
        "fn save(store: DirWrite) -> Result<Int, IoError> {\n    \
         store.with_lock(|dir: DirWrite| 7)\n}",
        "save",
        &["DirWrite"],
        false,
    );
}

#[test]
fn with_lock_is_listed_as_an_attenuation_of_the_directory_write_to_itself() {
    let report = check(
        "fn save(store: DirWrite) -> Result<Int, IoError> {\n    \
         store.with_lock(|dir: DirWrite| 7)\n}",
    );
    let found: Vec<(String, String, String)> = report
        .attenuations
        .iter()
        .map(|a| (a.method.clone(), a.source.clone(), a.result.clone()))
        .collect();
    assert_eq!(
        found,
        vec![(
            "with_lock".to_string(),
            "DirWrite".to_string(),
            "DirWrite".to_string()
        )]
    );
}

#[test]
fn adding_a_lock_to_a_function_is_not_a_widening() {
    let before = check(
        "fn save(store: DirWrite) -> Result<(), IoError> {\n    \
         store.write(Path.new(\"a\"), \"text\")\n}",
    );
    let after = check(
        "fn save(store: DirWrite) -> Result<(), IoError> {\n    \
         match store.with_lock(|dir: DirWrite| dir.write(Path.new(\"a\"), \"text\")) {\n        \
         Ok(written) => written\n        \
         Err(e) => Err(e)\n    }\n}",
    );
    let difference =
        diff(&AuthorityRecord::of(&before), &AuthorityRecord::of(&after)).expect("comparable");
    assert!(
        !difference.fails(),
        "a lock widened authority: {:?}",
        difference.widenings()
    );
}
