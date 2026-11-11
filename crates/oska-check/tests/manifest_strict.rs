//! The manifest reader's strictness (review of spec #115): a bare CR,
//! a loose `\u` escape, a byte-order mark and text that is not UTF-8 are
//! each one clear error with a location, and locating errors stays linear
//! in the manifest's length.

use std::time::{Duration, Instant};

use oska_check::{decode_manifest, parse_manifest};
use oska_syntax::Diagnostic;

fn errors(text: &str) -> Vec<Diagnostic> {
    parse_manifest("oska.toml", text).expect_err("the manifest should be refused")
}

fn line_col(d: &Diagnostic) -> (usize, usize) {
    let location = d
        .location
        .as_ref()
        .expect("a manifest error names its place");
    (location.line, location.column)
}

#[test]
fn a_bare_carriage_return_does_not_end_a_line() {
    let text = "[package]\nname = \"a\"\nversion = \"0.1.0\"\r[dependencies] evil = \"x\"\n";
    let found = errors(text);
    let first = &found[0];
    assert_eq!(first.code, "E0400", "{found:#?}");
    assert_eq!(line_col(first), (3, 18));
    assert!(
        first.message.contains("carriage return"),
        "{}",
        first.message
    );
}

#[test]
fn crlf_line_ends_are_still_accepted() {
    let text = "[package]\r\nname = \"a\"\r\nversion = \"0.1.0\"\r\n";
    let manifest = parse_manifest("oska.toml", text).expect("CRLF is a line end");
    assert_eq!(manifest.name, "a");
}

#[test]
fn a_unicode_escape_takes_exactly_hex_digits() {
    for escape in ["\\u+061", "\\u-061", "\\U+0000061"] {
        let text = format!("[package]\nname = \"a{escape}\"\nversion = \"0.1.0\"\n");
        let found = errors(&text);
        assert_eq!(found.len(), 1, "{escape}: {found:#?}");
        assert_eq!(found[0].code, "E0400", "{escape}");
        assert_eq!(
            &text[found[0].span.start..found[0].span.start + 2],
            &escape[..2]
        );
    }
}

#[test]
fn a_byte_order_mark_is_one_error_whose_fix_removes_it() {
    let text = "\u{feff}[package]\nname = \"a\"\nversion = \"0.1.0\"\n";
    let found = errors(text);
    assert_eq!(found.len(), 1, "{found:#?}");
    let bom = &found[0];
    assert_eq!((bom.code, line_col(bom)), ("E0400", (1, 1)));
    assert!(bom.message.contains("byte-order mark"), "{}", bom.message);
    let fix = bom.fix.as_ref().expect("a fix that removes the mark");
    let mut fixed = text.to_string();
    for edit in &fix.edits {
        fixed.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    assert!(parse_manifest("oska.toml", &fixed).is_ok(), "{fixed:?}");
}

#[test]
fn a_manifest_that_is_not_utf8_is_a_coded_error_at_the_bad_byte() {
    let bytes = b"[package]\nname = \"a\"\nversion = \"0.1.0\"\n# caf\xff\n";
    let d = decode_manifest("oska.toml", bytes).expect_err("not UTF-8");
    assert_eq!(d.code, "E0400");
    assert_eq!(line_col(&d), (4, 6));
    assert_eq!(
        d.location.as_ref().map(|l| l.path.as_str()),
        Some("oska.toml")
    );
    assert!(d.message.contains("UTF-8"), "{}", d.message);
    let text = decode_manifest("oska.toml", b"[package]\n").expect("UTF-8 decodes");
    assert_eq!(text, "[package]\n");
}

#[test]
fn locating_many_errors_is_linear_in_the_manifests_length() {
    let small = format!("[package]\n{}", "x\n".repeat(10_000));
    let large = format!("[package]\n{}", "x\n".repeat(100_000));
    let started = Instant::now();
    assert_eq!(errors(&small).len(), 10_000 + 2);
    let found = errors(&large);
    let elapsed = started.elapsed();
    assert_eq!(found.len(), 100_000 + 2);
    assert_eq!(line_col(&found[99_999]), (100_001, 1));
    // Quadratic locating takes minutes here; linear takes well under one
    // second even in a debug build.
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
}
