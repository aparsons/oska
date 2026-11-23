// SPDX-License-Identifier: Apache-2.0
//! `oskac check` on implementations of `std.mcp`'s `Toolset` (#249):
//! the authority report lists each tool under `call`, with what its arm
//! reaches, in the human and the JSON report.
//!
//! `tests/fixtures/toolset_split.osk` has a `call` that is a `match` on
//! the tool's name with literal arms: one tool reaches through a method
//! of the toolset, one through a free function given a field, one writes
//! to standard output, one spec has no arm and one arm has no spec.
//! `tests/fixtures/toolset_unsplit.osk` has a `call` of another shape,
//! reported as one entry for the whole toolset.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test toolset_golden` and review
//! the diff.

mod common;

use std::path::Path;
use std::process::Command;

use common::golden::assert_golden_file;
use common::portable::literal_path;

/// `oskac check` on the fixture `name`, run from the repository's root
/// with the fixture's path as written there, so the report names it the
/// same way on every platform, with `format` if given: its stdout, and
/// whether it succeeded.
fn check(name: &str, format: Option<&str>) -> (String, bool) {
    check_path(&format!("crates/oskac/tests/fixtures/{name}"), format)
}

/// `oskac check` on the file `path`, relative to the repository's root.
fn check_path(path: &str, format: Option<&str>) -> (String, bool) {
    let root = literal_path(Path::new(env!("CARGO_MANIFEST_DIR")), &["..", ".."]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command.args(["check", path]).current_dir(&root);
    if let Some(format) = format {
        command.arg(format);
    }
    let output = command.output().expect("oskac should run");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    (stdout, output.status.success())
}

/// Assert that `actual` is the golden file `name`; `UPDATE_GOLDEN=1`
/// rewrites it.
fn assert_golden(actual: &str, name: &str) {
    let golden = literal_path(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["tests", "golden", name],
    );
    assert_golden_file(&golden, actual);
}

#[test]
fn a_split_toolset_lists_each_tool_in_the_human_report() {
    let (stdout, ok) = check("toolset_split.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_split.txt");
}

#[test]
fn a_split_toolset_lists_each_tool_in_the_json_report() {
    let (stdout, ok) = check("toolset_split.osk", Some("--format=json"));
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_split.json");
}

#[test]
fn an_unsplit_toolset_has_one_entry_in_the_human_report() {
    let (stdout, ok) = check("toolset_unsplit.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unsplit.txt");
}

#[test]
fn an_unsplit_toolset_has_one_entry_in_the_json_report() {
    let (stdout, ok) = check("toolset_unsplit.osk", Some("--format=json"));
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unsplit.json");
}

#[test]
fn a_toolset_whose_specs_are_not_literal_says_its_names_are_not_checked() {
    let (stdout, ok) = check("toolset_unchecked.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_toolset_unchecked.txt");
}

#[test]
fn the_example_mcp_server_lists_each_tools_authority() {
    let (stdout, ok) = check_path("examples/notes_server.osk", None);
    assert!(ok, "the example should check cleanly: {stdout}");
    assert_golden(&stdout, "check_notes_server.txt");
    let (stdout, ok) = check_path("examples/notes_server.osk", Some("--format=json"));
    assert!(ok, "the example should check cleanly: {stdout}");
    assert_golden(&stdout, "check_notes_server.json");
}

#[test]
fn a_toolset_with_a_tool_for_each_kind_of_authority_flags_the_protocol_stream() {
    let (stdout, ok) = check("mcp_hints.osk", None);
    assert!(ok, "the fixture should check cleanly: {stdout}");
    assert_golden(&stdout, "check_mcp_hints.txt");
}

/// The server loop and `handle` are generic over the toolset: the report
/// never calls them pure, and gives `serve` only the streams it is given.
/// Checking `std.mcp`'s own source shows each of its functions.
#[test]
fn serve_and_handle_are_reported_generic_over_the_toolset() {
    let (stdout, ok) = check_path("crates/oska-check/src/std/mcp.osk", None);
    assert!(ok, "std.mcp should check cleanly: {stdout}");
    assert_golden(&stdout, "check_std_mcp.txt");
    for line in [
        "pub fn serve: Stdin (stdin), Stdout (stdout), generic T (tools)\n",
        "pub fn handle: generic T (tools)\n",
    ] {
        assert!(stdout.contains(line), "{line}");
    }
}
