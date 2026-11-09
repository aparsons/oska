//! Rules that reject forms the grammar would otherwise accept: holes in
//! string patterns and test names, misplaced `pub`, and `#[integration]`
//! on anything but a test. Each test asserts the code and exact span.

mod common;

use common::{assert_items, module_err};
use oska_syntax::{Diagnostic, Edit, Span};

/// Assert that `err` carries exactly one edit, deleting `start..end`.
fn assert_deletes(err: &Diagnostic, start: usize, end: usize) {
    let fix = err.fix.as_ref().expect("the diagnostic should carry a fix");
    assert_eq!(fix.message, "remove `pub`");
    assert_eq!(
        fix.edits,
        vec![Edit {
            span: Span { start, end },
            replacement: String::new(),
        }]
    );
}

#[test]
fn rejects_a_string_pattern_with_a_hole() {
    // A pattern compares against a fixed value, so it cannot interpolate.
    // The error covers the whole string literal.
    let src = "fn f() {\n    match s {\n        \"hi {name}\" => 1\n    }\n}";
    let err = module_err(src);
    assert_eq!(err.code, "E0160");
    assert_eq!(err.span, Span { start: 31, end: 42 });
    assert!(err.message.contains("plain literal"), "{}", err.message);
}

#[test]
fn accepts_a_string_pattern_with_an_escaped_brace() {
    let src = "fn f() {\n    match s {\n        \"\\{x\\}\" => 1\n    }\n}";
    oska_syntax::parse_module(src).expect("an escaped brace is not a hole");
}

#[test]
fn rejects_a_test_name_with_a_hole() {
    // The error covers the hole itself.
    let err = module_err("test \"adds {n}\" {}");
    assert_eq!(err.code, "E0161");
    assert_eq!(err.span, Span { start: 11, end: 14 });
}

#[test]
fn rejects_pub_on_a_trait_method() {
    let src = "trait Show {\n    pub fn show(self) -> Str\n}";
    let err = module_err(src);
    assert_eq!(err.code, "E0162");
    assert_eq!(err.span, Span { start: 17, end: 20 });
    assert!(
        err.message
            .contains("trait methods take their visibility from the trait"),
        "{}",
        err.message
    );
    assert_deletes(&err, 17, 21);
}

#[test]
fn rejects_pub_test() {
    let err = module_err("pub test \"adds\" {}");
    assert_eq!(err.code, "E0163");
    assert_eq!(err.span, Span { start: 0, end: 3 });
    assert_deletes(&err, 0, 4);
}

#[test]
fn rejects_pub_use() {
    let err = module_err("pub use std.io");
    assert_eq!(err.code, "E0164");
    assert_eq!(err.span, Span { start: 0, end: 3 });
    assert!(
        err.message.contains("re-exports are not defined in v0"),
        "{}",
        err.message
    );
    assert_deletes(&err, 0, 4);
}

#[test]
fn rejects_pub_after_an_attribute_with_the_right_span() {
    let err = module_err("#[integration]\npub test \"db\" {}");
    assert_eq!(err.code, "E0163");
    assert_eq!(err.span, Span { start: 15, end: 18 });
    assert_deletes(&err, 15, 19);
}

#[test]
fn rejects_integration_on_a_function() {
    // The error covers the attribute.
    let err = module_err("#[integration]\nfn f() {}");
    assert_eq!(err.code, "E0165");
    assert_eq!(err.span, Span { start: 0, end: 14 });
}

#[test]
fn rejects_integration_on_a_method() {
    let err = module_err("impl A {\n    #[integration] fn f() {}\n}");
    assert_eq!(err.code, "E0165");
    assert_eq!(err.span, Span { start: 13, end: 27 });
}

#[test]
fn integration_on_a_test_is_accepted() {
    assert_items(
        "#[integration] test \"db\" {}",
        r#"[{
            "kind": "TestItem", "span": {"start": 0, "end": 27},
            "public": false,
            "attributes": [{
                "kind": "Attribute", "span": {"start": 0, "end": 14},
                "name": {"kind": "Ident", "span": {"start": 2, "end": 13}, "name": "integration"},
                "args": []
            }],
            "name": {
                "kind": "StrExpr", "span": {"start": 20, "end": 24},
                "parts": [{"kind": "StrText", "span": {"start": 21, "end": 23}, "value": "db"}]
            },
            "body": {"kind": "Block", "span": {"start": 25, "end": 27}, "stmts": []}
        }]"#,
    );
}
