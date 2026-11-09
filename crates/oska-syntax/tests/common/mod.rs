//! Helpers shared by the `parse_module` integration tests.
//!
//! Tests parse a small snippet and compare its JSON export against JSON
//! written inline. Both sides are normalized by dropping whitespace outside
//! strings, so the inline JSON can be laid out for readability.

#![allow(dead_code)]

use oska_syntax::{module_to_json, parse_module, Diagnostic};

/// The file name the test exports use in their envelope.
pub const FILE: &str = "test.osk";

/// Remove all whitespace outside JSON strings.
pub fn compact(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut in_str = false;
    let mut escaped = false;
    for c in json.chars() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Parse `src` as a module and return its compacted JSON export.
pub fn export(src: &str) -> String {
    match parse_module(src) {
        Ok(module) => compact(&module_to_json(&module, FILE)),
        Err(err) => panic!("{src:?} should parse: {err}"),
    }
}

/// Assert that `src` exports exactly the envelope whose `items` array is
/// the inline JSON `items`.
pub fn assert_items(src: &str, items: &str) {
    let expected = compact(&format!(
        r#"{{"ast_version": 1, "file": "{FILE}", "items": {items}}}"#
    ));
    assert_eq!(export(src), expected, "export of {src:?}");
}

/// Assert that the body of the single function in `src` exports exactly
/// the inline JSON `stmts` array. Use it for expression and statement nodes.
pub fn assert_body(src: &str, stmts: &str) {
    let module = export(src);
    let start = module
        .find(r#""stmts":"#)
        .expect("the export should contain a function body");
    let body = &module[start + r#""stmts":"#.len()..];
    // The body's `stmts` array is followed by the closing braces of the
    // block, the item, the items array, and the envelope.
    let body = body
        .strip_suffix("}}]}")
        .expect("the function body should be the last field of the export");
    assert_eq!(body, compact(stmts), "body of {src:?}");
}

/// Parse `src` as a module, expecting an error.
pub fn module_err(src: &str) -> Diagnostic {
    parse_module(src).expect_err("should fail to parse")
}

/// The JSON for an `Ident` node.
pub fn ident(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(r#"{{"kind": "Ident", "span": {{"start": {start}, "end": {end}}}, "name": "{name}"}}"#)
}

/// The JSON for a single-segment path type such as `Int` at `start`.
pub fn path_ty(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(
        r#"{{"kind": "PathType", "span": {{"start": {start}, "end": {end}}},
            "segments": [{segment}], "args": []}}"#,
        segment = ident(name, start),
    )
}
