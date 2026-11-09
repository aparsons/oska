//! `parse_module` tests for full function headers: generic parameters with
//! bounds, `self` and `var self` receivers, and contract clauses.

mod common;

use common::{assert_items, module_err};
use oska_syntax::ast::ItemKind;
use oska_syntax::{parse_module, Span};

/// The first ```` ```oska ```` sample after `heading` in the design notes.
fn design_sample(heading: &str) -> String {
    let doc = include_str!("../../../docs/design.md");
    let section = &doc[doc.find(heading).expect("the heading should exist")..];
    let start = section
        .find("```oska\n")
        .expect("the section should have a sample")
        + 8;
    let len = section[start..].find("```").expect("the sample should end");
    section[start..start + len].to_string()
}

#[test]
fn contracts_sample_parses() {
    let src = design_sample("\n## Contracts\n");
    let module = parse_module(&src).unwrap_or_else(|err| panic!("should parse: {err}"));
    let [item] = module.items.as_slice() else {
        panic!("expected one item, got {:?}", module.items);
    };
    let ItemKind::Fn(decl) = &item.kind else {
        panic!("expected a function, got {item:?}");
    };
    assert_eq!(decl.name.name, "withdraw");
    assert_eq!(decl.params.len(), 2);
    assert_eq!(decl.requires.len(), 1);
    assert_eq!(decl.ensures.len(), 1);
    assert_eq!(decl.ensures[0].binding.name, "r");
}

#[test]
fn generic_parameters_with_bounds() {
    assert_items(
        "fn f<T: Eq + Hash, U>() {}",
        r#"[{
            "kind": "FnItem", "span": {"start": 0, "end": 26},
            "public": false,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 3, "end": 4}, "name": "f"},
            "generics": [
                {"kind": "GenericParam", "span": {"start": 5, "end": 17},
                 "name": {"kind": "Ident", "span": {"start": 5, "end": 6}, "name": "T"},
                 "bounds": [
                     {"kind": "PathType", "span": {"start": 8, "end": 10},
                      "segments": [{"kind": "Ident", "span": {"start": 8, "end": 10}, "name": "Eq"}],
                      "args": []},
                     {"kind": "PathType", "span": {"start": 13, "end": 17},
                      "segments": [{"kind": "Ident", "span": {"start": 13, "end": 17}, "name": "Hash"}],
                      "args": []}
                 ]},
                {"kind": "GenericParam", "span": {"start": 19, "end": 20},
                 "name": {"kind": "Ident", "span": {"start": 19, "end": 20}, "name": "U"},
                 "bounds": []}
            ],
            "params": [],
            "requires": [],
            "ensures": [],
            "body": {"kind": "Block", "span": {"start": 24, "end": 26}, "stmts": []}
        }]"#,
    );
}

#[test]
fn self_receiver() {
    // The receiver sits apart from the ordinary parameters. Only methods
    // take one, so the function sits in an `impl` block.
    assert_items(
        "impl T {\nfn f(self, n: Int) {}\n}",
        r#"[{
          "kind": "ImplItem", "span": {"start": 0, "end": 32},
          "public": false, "attributes": [], "generics": [],
          "target": {"kind": "PathType", "span": {"start": 5, "end": 6}, "segments": [{"kind": "Ident", "span": {"start": 5, "end": 6}, "name": "T"}], "args": []},
          "methods": [{
            "kind": "FnItem", "span": {"start": 9, "end": 30},
            "public": false,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 12, "end": 13}, "name": "f"},
            "generics": [],
            "receiver": {"kind": "SelfParam", "span": {"start": 14, "end": 18}, "mutable": false},
            "params": [
                {"kind": "Param", "span": {"start": 20, "end": 26},
                 "name": {"kind": "Ident", "span": {"start": 20, "end": 21}, "name": "n"},
                 "type": {"kind": "PathType", "span": {"start": 23, "end": 26},
                          "segments": [{"kind": "Ident", "span": {"start": 23, "end": 26}, "name": "Int"}],
                          "args": []}}
            ],
            "requires": [],
            "ensures": [],
            "body": {"kind": "Block", "span": {"start": 28, "end": 30}, "stmts": []}
        }]
        }]"#,
    );
}

#[test]
fn var_self_receiver_marks_a_mutating_method() {
    assert_items(
        "impl T {\nfn g(var self) {}\n}",
        r#"[{
          "kind": "ImplItem", "span": {"start": 0, "end": 28},
          "public": false, "attributes": [], "generics": [],
          "target": {"kind": "PathType", "span": {"start": 5, "end": 6}, "segments": [{"kind": "Ident", "span": {"start": 5, "end": 6}, "name": "T"}], "args": []},
          "methods": [{
            "kind": "FnItem", "span": {"start": 9, "end": 26},
            "public": false,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 12, "end": 13}, "name": "g"},
            "generics": [],
            "receiver": {"kind": "SelfParam", "span": {"start": 14, "end": 22}, "mutable": true},
            "params": [],
            "requires": [],
            "ensures": [],
            "body": {"kind": "Block", "span": {"start": 24, "end": 26}, "stmts": []}
        }]
        }]"#,
    );
}

#[test]
fn requires_clause_on_its_own_line() {
    // The clause and the `{` each start a line that continues the header.
    assert_items(
        "fn f(n: Int)\n    requires n > 0\n{}",
        r#"[{
            "kind": "FnItem", "span": {"start": 0, "end": 34},
            "public": false,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 3, "end": 4}, "name": "f"},
            "generics": [],
            "params": [
                {"kind": "Param", "span": {"start": 5, "end": 11},
                 "name": {"kind": "Ident", "span": {"start": 5, "end": 6}, "name": "n"},
                 "type": {"kind": "PathType", "span": {"start": 8, "end": 11},
                          "segments": [{"kind": "Ident", "span": {"start": 8, "end": 11}, "name": "Int"}],
                          "args": []}}
            ],
            "requires": [
                {"kind": "RequiresClause", "span": {"start": 17, "end": 31},
                 "cond": {"kind": "BinaryExpr", "span": {"start": 26, "end": 31}, "op": ">",
                          "lhs": {"kind": "NameExpr", "span": {"start": 26, "end": 27}, "name": "n"},
                          "rhs": {"kind": "IntExpr", "span": {"start": 30, "end": 31}, "value": "0"}}}
            ],
            "ensures": [],
            "body": {"kind": "Block", "span": {"start": 32, "end": 34}, "stmts": []}
        }]"#,
    );
}

#[test]
fn ensures_clause_binds_the_return_value() {
    assert_items(
        "fn f() -> Int ensures |r| r > 0 {}",
        r#"[{
            "kind": "FnItem", "span": {"start": 0, "end": 34},
            "public": false,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 3, "end": 4}, "name": "f"},
            "generics": [],
            "params": [],
            "return_type": {"kind": "PathType", "span": {"start": 10, "end": 13},
                            "segments": [{"kind": "Ident", "span": {"start": 10, "end": 13}, "name": "Int"}],
                            "args": []},
            "requires": [],
            "ensures": [
                {"kind": "EnsuresClause", "span": {"start": 14, "end": 31},
                 "binding": {"kind": "Ident", "span": {"start": 23, "end": 24}, "name": "r"},
                 "cond": {"kind": "BinaryExpr", "span": {"start": 26, "end": 31}, "op": ">",
                          "lhs": {"kind": "NameExpr", "span": {"start": 26, "end": 27}, "name": "r"},
                          "rhs": {"kind": "IntExpr", "span": {"start": 30, "end": 31}, "value": "0"}}}
            ],
            "body": {"kind": "Block", "span": {"start": 32, "end": 34}, "stmts": []}
        }]"#,
    );
}

#[test]
fn rejects_ensures_without_a_binding() {
    // The error covers the `ensures` keyword.
    let err = module_err("fn f() -> Int ensures r > 0 {}");
    assert_eq!(err.code, "E0110");
    assert_eq!(err.span, Span { start: 14, end: 21 });

    let err = module_err("fn f() -> Int\n    ensures || true\n{}");
    assert_eq!(err.code, "E0110");
    assert_eq!(err.span, Span { start: 18, end: 25 });
}

#[test]
fn rejects_self_after_other_parameters() {
    let err = module_err("impl T { fn f(n: Int, self) {} }");
    assert_eq!(err.code, "E0111");
    assert_eq!(err.span, Span { start: 22, end: 26 });

    let err = module_err("impl T { fn f(n: Int, var self) {} }");
    assert_eq!(err.code, "E0111");
    assert_eq!(err.span, Span { start: 22, end: 30 });
}

#[test]
fn rejects_var_on_an_ordinary_parameter() {
    // Only the receiver can be `var`; the error covers the `var` keyword.
    let err = module_err("fn f(var n: Int) {}");
    assert_eq!(err.code, "E0112");
    assert_eq!(err.span, Span { start: 5, end: 8 });
}

#[test]
fn rejects_where_clause() {
    // Bounds are written inline. The error covers the whole clause, up to
    // the body.
    let err = module_err("fn f<T>(x: T) where T: Eq {}");
    assert_eq!(err.code, "E0113");
    assert_eq!(err.span, Span { start: 14, end: 25 });
}
