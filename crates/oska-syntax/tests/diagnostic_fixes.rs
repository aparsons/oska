//! Machine-applicable fixes attached to diagnostics. Each test checks the
//! diagnostic's code and span, then applies the fix's edits to the source
//! and checks the result.

mod common;

use common::module_err;
use oska_syntax::{lex, Diagnostic, Span};

/// Apply every edit of the diagnostic's fix to `src`.
fn apply_fix(src: &str, err: &Diagnostic) -> String {
    let fix = err
        .fix
        .as_ref()
        .unwrap_or_else(|| panic!("{} should carry a fix", err.code));
    let mut edits = fix.edits.clone();
    // Apply from the end so earlier offsets stay valid.
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = src.to_string();
    for edit in edits {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    out
}

#[test]
fn a_diagnostic_without_a_fix_omits_the_field() {
    let err = module_err("fn f() {\n    let x\n}");
    assert_eq!(err.code, "E0104");
    assert!(err.fix.is_none());
    assert!(!err.to_json().contains("\"fix\""), "{}", err.to_json());
}

#[test]
fn json_carries_the_fix_after_the_span() {
    let err = module_err("pub impl C {}");
    assert_eq!(err.code, "E0151");
    assert_eq!(err.span, Span { start: 0, end: 3 });
    let json = err.to_json();
    let fix_at = json.find(",\"fix\":").expect("the JSON should have a fix");
    assert!(
        json.ends_with(
            ",\"fix\":{\"message\":\"remove `pub`\",\"edits\":\
             [{\"span\":{\"start\":0,\"end\":4},\"replacement\":\"\"}],\
             \"applicability\":\"machine\"},\"related\":[],\"suggestions\":[]}"
        ),
        "{json}"
    );
    assert!(json[..fix_at].contains("\"span\":{\"start\":0,\"end\":3}"));
    assert!(json[..fix_at].ends_with("\"url\":\"https://oskalang.dev/codes/E0151\""));
}

#[test]
fn pub_impl_fix_deletes_pub() {
    let src = "pub impl C {}";
    assert_eq!(apply_fix(src, &module_err(src)), "impl C {}");
}

#[test]
fn match_arm_comma_fix_deletes_the_comma() {
    let src = "fn f() {\n    match x {\n        _ => 0,\n        _ => 1\n    }\n}";
    let err = module_err(src);
    assert_eq!(err.code, "E0143");
    assert_eq!(err.span, Span { start: 37, end: 38 });
    assert_eq!(
        apply_fix(src, &err),
        "fn f() {\n    match x {\n        _ => 0\n        _ => 1\n    }\n}"
    );
}

#[test]
fn match_arm_comma_on_one_line_has_no_fix() {
    // Deleting the comma alone would not separate the arms.
    let err = module_err("fn f() {\n    match x {\n        _ => 0, _ => 1\n    }\n}");
    assert_eq!(err.code, "E0143");
    assert_eq!(err.span, Span { start: 37, end: 38 });
    assert!(err.fix.is_none());
}

#[test]
fn missing_comma_fix_inserts_one() {
    let src = "struct A {\n    x: Int\n    y: Int\n}";
    let err = module_err(src);
    assert_eq!(err.code, "E0120");
    assert_eq!(err.span, Span { start: 21, end: 21 });
    assert_eq!(
        apply_fix(src, &err),
        "struct A {\n    x: Int,\n    y: Int\n}"
    );
}

#[test]
fn var_parameter_fix_deletes_var() {
    let src = "fn f(var n: Int) {}";
    let err = module_err(src);
    assert_eq!(err.code, "E0112");
    assert_eq!(err.span, Span { start: 5, end: 8 });
    assert_eq!(apply_fix(src, &err), "fn f(n: Int) {}");
}

#[test]
fn trailing_semicolon_fix_deletes_it() {
    let src = "let x = 1;  // one\nlet y = 2";
    let err = lex(src).expect_err("should fail");
    assert_eq!(err.code, "E0005");
    assert_eq!(err.span, Span { start: 9, end: 10 });
    assert_eq!(apply_fix(src, &err), "let x = 1  // one\nlet y = 2");
}

#[test]
fn semicolon_between_statements_has_no_fix() {
    // Deleting it would run the two statements together.
    let err = lex("let x = 1; let y = 2").expect_err("should fail");
    assert_eq!(err.code, "E0005");
    assert_eq!(err.span, Span { start: 9, end: 10 });
    assert!(err.fix.is_none());
}

#[test]
fn capability_attribute_off_a_struct_fix_removes_it() {
    let src = "#[capability]\nfn args() -> Args";
    let err = oska_syntax::parse_prelude(src).expect_err("should fail");
    assert_eq!(err.code, "E0170");
    assert_eq!(apply_fix(src, &err), "fn args() -> Args");
}

#[test]
fn capability_attribute_on_a_method_fix_removes_it() {
    let src = "impl Stdout {\n    #[capability]\n    fn print(self, s: String)\n}";
    let err = oska_syntax::parse_prelude(src).expect_err("should fail");
    assert_eq!(err.code, "E0170");
    assert_eq!(
        apply_fix(src, &err),
        "impl Stdout {\n    fn print(self, s: String)\n}"
    );
}
