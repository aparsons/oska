// SPDX-License-Identifier: Apache-2.0
//! A module wrapped in one Markdown fence (spec #446, ticket #449).
//!
//! A model often wraps its code in a fence. The checker still refuses it
//! with `E0001` on the first backtick, so the verdict is about exactly the
//! bytes that would run, but when the module starts with an opening fence
//! line and ends with its closing fence line, that `E0001` has a
//! `machine` fix that removes both lines. A backtick anywhere else gets
//! the plain `E0001`, with no fix.
//!
//! Each case writes `app.osk` into a scratch directory and runs the real
//! `oskac check` and `oskac fix` there, as an embedder or an agent does.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::portable::Scratch;
use common::Json;

/// The unfenced module the fence cases wrap: it checks with no diagnostic.
const UNFENCED: &str = "fn main() {\n    let x = 1\n}\n";

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// What `oskac check app.osk --format=json` says of `source`: the exit
/// code and stdout.
fn check_json(dir: &Path, source: &str) -> (Option<i32>, String) {
    std::fs::write(dir.join("app.osk"), source).expect("should write app.osk");
    let output = oskac(dir, &["check", "app.osk", "--format=json"]);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    (output.status.code(), stdout)
}

/// The one diagnostic `oskac check` reports on `source`, as JSON.
fn only_diagnostic(dir: &Path, source: &str) -> Json {
    let (_, stdout) = check_json(dir, source);
    match Json::parse(&stdout).get("diagnostics") {
        Some(Json::Arr(list)) if list.len() == 1 => list[0].clone(),
        other => panic!("one diagnostic expected: {other:?}"),
    }
}

/// Assert that `fenced` reports `E0001` with a `machine` fix, that
/// `oskac fix` turns it into exactly `unfenced`, and that the result
/// checks as `unfenced` does.
fn assert_fence_removed(name: &str, fenced: &str, unfenced: &str) {
    let dir = Scratch::new("fence_fix", name);
    let diagnostic = only_diagnostic(&dir, fenced);
    assert_eq!(diagnostic.get("code"), Some(&Json::Str("E0001".into())));
    let applicability = diagnostic
        .get("fix")
        .and_then(|fix| fix.get("applicability"))
        .cloned();
    assert_eq!(
        applicability,
        Some(Json::Str("machine".into())),
        "{name}: {diagnostic:?}"
    );
    std::fs::write(dir.join("app.osk"), fenced).expect("should write app.osk");
    let fix = oskac(&dir, &["fix", "app.osk"]);
    assert!(fix.status.success(), "{name}: oskac fix: {fix:?}");
    let after = std::fs::read_to_string(dir.join("app.osk")).expect("should read app.osk");
    assert_eq!(after, unfenced, "{name}");
    assert_eq!(
        check_json(&dir, &after),
        check_json(&dir, unfenced),
        "{name}"
    );
}

#[test]
fn a_fence_with_an_info_string_is_removed_by_its_machine_fix() {
    let fenced = format!("```oska\n{UNFENCED}```\n");
    assert_fence_removed("info", &fenced, UNFENCED);
}

#[test]
fn a_fence_with_no_info_string_is_removed_by_its_machine_fix() {
    let fenced = format!("````\n{UNFENCED}````\n");
    assert_fence_removed("bare", &fenced, UNFENCED);
}

#[test]
fn a_fence_with_windows_line_endings_is_removed_by_its_machine_fix() {
    let unfenced = UNFENCED.replace('\n', "\r\n");
    let with_info = format!("```oska\r\n{unfenced}```\r\n");
    assert_fence_removed("crlf_info", &with_info, &unfenced);
    let bare = format!("```\r\n{unfenced}```\r\n");
    assert_fence_removed("crlf_bare", &bare, &unfenced);
}

/// The code and the fix of the one diagnostic `oskac check` reports on
/// `source`.
fn fix_of(name: &str, source: &str) -> (Json, Option<Json>) {
    let dir = Scratch::new("fence_fix", name);
    let diagnostic = only_diagnostic(&dir, source);
    let code = diagnostic.get("code").cloned().expect("a code");
    (code, diagnostic.get("fix").cloned())
}

#[test]
fn a_backtick_that_is_not_a_whole_module_fence_gets_no_fix() {
    let e0001 = Json::Str("E0001".into());
    let cases = [
        ("stray", "fn main() {\n    let x = `1`\n}\n".to_string()),
        ("unclosed", format!("```oska\n{UNFENCED}")),
        ("short_close", format!("````oska\n{UNFENCED}```\n")),
        (
            "text_after",
            format!("```oska\n{UNFENCED}```\nRun it with oskac.\n"),
        ),
        (
            "text_before",
            format!("Here it is:\n```oska\n{UNFENCED}```\n"),
        ),
        (
            "closed_inside",
            format!("```oska\n{UNFENCED}```\n{UNFENCED}```\n"),
        ),
        ("backtick_in_info", format!("```os`ka\n{UNFENCED}```\n")),
        ("two_backticks", format!("``\n{UNFENCED}``\n")),
        ("one_line", "```oska```\n".to_string()),
    ];
    for (name, source) in cases {
        assert_eq!(fix_of(name, &source), (e0001.clone(), None), "{name}");
    }
}

#[test]
fn a_fence_that_does_not_start_the_module_gets_no_fix() {
    let e0001 = Json::Str("E0001".into());
    let cases = [
        ("bom", format!("\u{feff}```oska\n{UNFENCED}```\n")),
        ("blank_line", format!("\n```oska\n{UNFENCED}```\n")),
    ];
    for (name, source) in cases {
        assert_eq!(fix_of(name, &source), (e0001.clone(), None), "{name}");
    }
}
