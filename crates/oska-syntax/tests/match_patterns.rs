//! `match` expressions and the v0 pattern set, through `parse_module`.

mod common;

use common::{assert_body, module_err};
use oska_syntax::Span;

#[test]
fn match_with_wildcard_arm() {
    assert_body(
        "fn f() {\n    match x {\n        _ => 0\n    }\n}",
        r#"[
            {"kind": "ExprStmt", "span": {"start": 13, "end": 43},
             "expr": {"kind": "MatchExpr", "span": {"start": 13, "end": 43},
              "scrutinee": {"kind": "NameExpr", "span": {"start": 19, "end": 20}, "name": "x"},
              "arms": [
                {"kind": "MatchArm", "span": {"start": 31, "end": 37},
                 "pattern": {"kind": "WildcardPattern", "span": {"start": 31, "end": 32}},
                 "body": {"kind": "IntExpr", "span": {"start": 36, "end": 37}, "value": "0"}}
              ]}}
        ]"#,
    );
}

#[test]
fn binding_and_positional_constructor_patterns() {
    assert_body(
        "fn f() {\n    match x {\n        Some(v) => v\n        Shape.Empty => 0\n        n => n\n    }\n}",
        r#"[
            {"kind": "ExprStmt", "span": {"start": 13, "end": 89},
             "expr": {"kind": "MatchExpr", "span": {"start": 13, "end": 89},
              "scrutinee": {"kind": "NameExpr", "span": {"start": 19, "end": 20}, "name": "x"},
              "arms": [
                {"kind": "MatchArm", "span": {"start": 31, "end": 43},
                 "pattern": {"kind": "ConstructorPattern", "span": {"start": 31, "end": 38},
                             "path": [{"kind": "Ident", "span": {"start": 31, "end": 35}, "name": "Some"}],
                             "args": [{"kind": "PatternArg", "span": {"start": 36, "end": 37},
                                       "pattern": {"kind": "BindingPattern", "span": {"start": 36, "end": 37}, "name": "v"}}]},
                 "body": {"kind": "NameExpr", "span": {"start": 42, "end": 43}, "name": "v"}},
                {"kind": "MatchArm", "span": {"start": 52, "end": 68},
                 "pattern": {"kind": "ConstructorPattern", "span": {"start": 52, "end": 63},
                             "path": [{"kind": "Ident", "span": {"start": 52, "end": 57}, "name": "Shape"},
                                      {"kind": "Ident", "span": {"start": 58, "end": 63}, "name": "Empty"}],
                             "args": []},
                 "body": {"kind": "IntExpr", "span": {"start": 67, "end": 68}, "value": "0"}},
                {"kind": "MatchArm", "span": {"start": 77, "end": 83},
                 "pattern": {"kind": "BindingPattern", "span": {"start": 77, "end": 78}, "name": "n"},
                 "body": {"kind": "NameExpr", "span": {"start": 82, "end": 83}, "name": "n"}}
              ]}}
        ]"#,
    );
}

#[test]
fn named_constructor_pattern_with_guard() {
    assert_body(
        "fn f() {\n    match s {\n        C(radius: r) if r > 1 => r\n    }\n}",
        r#"[
            {"kind": "ExprStmt", "span": {"start": 13, "end": 63},
             "expr": {"kind": "MatchExpr", "span": {"start": 13, "end": 63},
              "scrutinee": {"kind": "NameExpr", "span": {"start": 19, "end": 20}, "name": "s"},
              "arms": [
                {"kind": "MatchArm", "span": {"start": 31, "end": 57},
                 "pattern": {"kind": "ConstructorPattern", "span": {"start": 31, "end": 43},
                             "path": [{"kind": "Ident", "span": {"start": 31, "end": 32}, "name": "C"}],
                             "args": [{"kind": "PatternArg", "span": {"start": 33, "end": 42},
                                       "label": {"kind": "Ident", "span": {"start": 33, "end": 39}, "name": "radius"},
                                       "pattern": {"kind": "BindingPattern", "span": {"start": 41, "end": 42}, "name": "r"}}]},
                 "guard": {"kind": "BinaryExpr", "span": {"start": 47, "end": 52}, "op": ">",
                           "lhs": {"kind": "NameExpr", "span": {"start": 47, "end": 48}, "name": "r"},
                           "rhs": {"kind": "IntExpr", "span": {"start": 51, "end": 52}, "value": "1"}},
                 "body": {"kind": "NameExpr", "span": {"start": 56, "end": 57}, "name": "r"}}
              ]}}
        ]"#,
    );
}

#[test]
fn describe_from_the_design_notes_parses() {
    // The `describe` function from the Syntax sample in docs/design.md.
    let src = "fn describe(s: Shape) -> String {
    match s {
        Shape.Circle(radius: r) if r > 100 => \"big circle\"
        Shape.Circle(radius: _) => \"circle\"
        Shape.Square(side: n) => \"square\"
    }
}";
    let json = common::export(src);
    assert_eq!(json.matches(r#""kind":"MatchArm""#).count(), 3);
    assert_eq!(json.matches(r#""kind":"ConstructorPattern""#).count(), 3);
    assert_eq!(json.matches(r#""guard":"#).count(), 1);
    assert_eq!(json.matches(r#""kind":"WildcardPattern""#).count(), 1);
}

/// A function whose body matches `x` with the single arm `arm => 0`. The
/// arm starts at byte 31.
fn one_arm(arm: &str) -> String {
    format!("fn f() {{\n    match x {{\n        {arm} => 0\n    }}\n}}")
}

#[test]
fn rejects_or_patterns() {
    let err = module_err(&one_arm("A | B"));
    assert_eq!(err.code, "E0140");
    assert_eq!(err.span, Span { start: 31, end: 36 });
    assert!(
        err.message.contains("not supported in v0"),
        "{}",
        err.message
    );

    let err = module_err(&one_arm("Some(A || B)"));
    assert_eq!(err.code, "E0140");
    assert_eq!(err.span, Span { start: 36, end: 42 });
}

#[test]
fn rejects_range_patterns() {
    let err = module_err(&one_arm("1..5"));
    assert_eq!(err.code, "E0141");
    assert_eq!(err.span, Span { start: 31, end: 35 });
    assert!(
        err.message.contains("not supported in v0"),
        "{}",
        err.message
    );
}

#[test]
fn rejects_at_bindings() {
    let err = module_err(&one_arm("v @ Some(_)"));
    assert_eq!(err.code, "E0142");
    assert_eq!(err.span, Span { start: 31, end: 42 });
    assert!(
        err.message.contains("not supported in v0"),
        "{}",
        err.message
    );
}

#[test]
fn rejects_commas_between_arms() {
    let err = module_err("fn f() {\n    match x {\n        _ => 0,\n        _ => 1\n    }\n}");
    assert_eq!(err.code, "E0143");
    assert_eq!(err.span, Span { start: 37, end: 38 });
}

#[test]
fn literal_patterns() {
    assert_body(
        "fn f() {\n    match x {\n        1 => a\n        -2 => b\n        \"s\" => c\n        true => d\n        () => e\n    }\n}",
        r#"[
            {"kind": "ExprStmt", "span": {"start": 13, "end": 110},
             "expr": {"kind": "MatchExpr", "span": {"start": 13, "end": 110},
              "scrutinee": {"kind": "NameExpr", "span": {"start": 19, "end": 20}, "name": "x"},
              "arms": [
                {"kind": "MatchArm", "span": {"start": 31, "end": 37},
                 "pattern": {"kind": "LiteralPattern", "span": {"start": 31, "end": 32},
                             "value": {"kind": "IntExpr", "span": {"start": 31, "end": 32}, "value": "1"}},
                 "body": {"kind": "NameExpr", "span": {"start": 36, "end": 37}, "name": "a"}},
                {"kind": "MatchArm", "span": {"start": 46, "end": 53},
                 "pattern": {"kind": "LiteralPattern", "span": {"start": 46, "end": 48},
                             "value": {"kind": "UnaryExpr", "span": {"start": 46, "end": 48}, "op": "-",
                                       "operand": {"kind": "IntExpr", "span": {"start": 47, "end": 48}, "value": "2"}}},
                 "body": {"kind": "NameExpr", "span": {"start": 52, "end": 53}, "name": "b"}},
                {"kind": "MatchArm", "span": {"start": 62, "end": 70},
                 "pattern": {"kind": "LiteralPattern", "span": {"start": 62, "end": 65},
                             "value": {"kind": "StrExpr", "span": {"start": 62, "end": 65},
                                       "parts": [{"kind": "StrText", "span": {"start": 63, "end": 64}, "value": "s"}]}},
                 "body": {"kind": "NameExpr", "span": {"start": 69, "end": 70}, "name": "c"}},
                {"kind": "MatchArm", "span": {"start": 79, "end": 88},
                 "pattern": {"kind": "LiteralPattern", "span": {"start": 79, "end": 83},
                             "value": {"kind": "BoolExpr", "span": {"start": 79, "end": 83}, "value": true}},
                 "body": {"kind": "NameExpr", "span": {"start": 87, "end": 88}, "name": "d"}},
                {"kind": "MatchArm", "span": {"start": 97, "end": 104},
                 "pattern": {"kind": "LiteralPattern", "span": {"start": 97, "end": 99},
                             "value": {"kind": "UnitExpr", "span": {"start": 97, "end": 99}}},
                 "body": {"kind": "NameExpr", "span": {"start": 103, "end": 104}, "name": "e"}}
              ]}}
        ]"#,
    );
}
