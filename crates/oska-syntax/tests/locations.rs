//! A diagnostic's location: the file it is in, and the line and column
//! its span starts at, as `path:line:col`.

use oska_syntax::{Diagnostic, Location, Span};

fn at(src: &str, offset: usize) -> String {
    Location::new("f.osk", src, offset).to_string()
}

#[test]
fn lines_and_columns_count_from_one() {
    let src = "fn f() {\n    1\n}\n";
    assert_eq!(at(src, 0), "f.osk:1:1");
    assert_eq!(at(src, 13), "f.osk:2:5");
    assert_eq!(at(src, 15), "f.osk:3:1");
}

#[test]
fn a_column_counts_characters_not_bytes() {
    let src = "let s = \"é\" + x";
    let x = src.find('x').expect("x is in the source");
    assert_eq!(at(src, x), "f.osk:1:15");
}

#[test]
fn an_offset_past_the_end_is_the_end() {
    assert_eq!(at("ab\nc", 99), "f.osk:2:2");
}

#[test]
fn a_located_diagnostic_carries_its_path_in_json_before_its_span() {
    let diagnostic =
        Diagnostic::new("E0001", "bad", Span { start: 2, end: 3 }).locate("src/a/b.osk", "ab\nc");
    assert_eq!(
        diagnostic.to_json(),
        r#"{"code":"E0001","message":"bad","path":"src/a/b.osk","span":{"start":2,"end":3}}"#
    );
}
