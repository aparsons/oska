//! `parse_module` tests for `use`, `const`, `test`, and attributes, and
//! for the forms v0 rejects: glob and renaming imports, unknown
//! attributes, and `unsafe`.

mod common;

use common::{assert_items, module_err};
use oska_syntax::Span;

#[test]
fn use_a_module() {
    assert_items(
        "use std.io",
        r#"[{
            "kind": "UseItem", "span": {"start": 0, "end": 10},
            "public": false,
            "attributes": [],
            "path": [
                {"kind": "Ident", "span": {"start": 4, "end": 7}, "name": "std"},
                {"kind": "Ident", "span": {"start": 8, "end": 10}, "name": "io"}
            ]
        }]"#,
    );
}

#[test]
fn use_names_from_a_module() {
    // A trailing comma is allowed, and the group may span lines.
    assert_items(
        "use std.sql.{Db,\n query,}",
        r#"[{
            "kind": "UseItem", "span": {"start": 0, "end": 25},
            "public": false,
            "attributes": [],
            "path": [
                {"kind": "Ident", "span": {"start": 4, "end": 7}, "name": "std"},
                {"kind": "Ident", "span": {"start": 8, "end": 11}, "name": "sql"}
            ],
            "names": [
                {"kind": "Ident", "span": {"start": 13, "end": 15}, "name": "Db"},
                {"kind": "Ident", "span": {"start": 18, "end": 23}, "name": "query"}
            ]
        }]"#,
    );
}

#[test]
fn use_group_one_name_per_line() {
    assert_items(
        "use a.{\n    X,\n    y\n}\n",
        r#"[{
            "kind": "UseItem", "span": {"start": 0, "end": 22},
            "public": false,
            "attributes": [],
            "path": [{"kind": "Ident", "span": {"start": 4, "end": 5}, "name": "a"}],
            "names": [
                {"kind": "Ident", "span": {"start": 12, "end": 13}, "name": "X"},
                {"kind": "Ident", "span": {"start": 19, "end": 20}, "name": "y"}
            ]
        }]"#,
    );
}

#[test]
fn rejects_glob_import() {
    // Every imported name must be written out, so the error points at `*`.
    let err = module_err("use std.sql.*");
    assert_eq!(err.code, "E0130");
    assert_eq!(err.span, Span { start: 12, end: 13 });

    let err = module_err("use std.sql.{Db, *}");
    assert_eq!(err.code, "E0130");
    assert_eq!(err.span, Span { start: 17, end: 18 });
}

#[test]
fn rejects_renaming_import() {
    // The error covers `as` and the new name.
    let err = module_err("use std.sql.query as q");
    assert_eq!(err.code, "E0131");
    assert_eq!(err.span, Span { start: 18, end: 22 });
    assert!(
        err.message.contains("`use std.sql`") && err.message.contains("`sql.query`"),
        "the message should say to import the module and qualify: {}",
        err.message
    );

    let err = module_err("use std.sql.{Db as D, query}");
    assert_eq!(err.code, "E0131");
    assert_eq!(err.span, Span { start: 16, end: 20 });
}

#[test]
fn constant() {
    assert_items(
        "pub const LIMIT: Int = 10 * 2",
        r#"[{
            "kind": "ConstItem", "span": {"start": 0, "end": 29},
            "public": true,
            "attributes": [],
            "name": {"kind": "Ident", "span": {"start": 10, "end": 15}, "name": "LIMIT"},
            "type": {"kind": "PathType", "span": {"start": 17, "end": 20},
                     "segments": [{"kind": "Ident", "span": {"start": 17, "end": 20}, "name": "Int"}],
                     "args": []},
            "value": {"kind": "BinaryExpr", "span": {"start": 23, "end": 29}, "op": "*",
                      "lhs": {"kind": "IntExpr", "span": {"start": 23, "end": 25}, "value": "10"},
                      "rhs": {"kind": "IntExpr", "span": {"start": 28, "end": 29}, "value": "2"}}
        }]"#,
    );
}

#[test]
fn test_item() {
    // The name is a string literal, exported as a `StrExpr`.
    assert_items(
        "test \"adds\" {\n    check()\n}",
        r#"[{
            "kind": "TestItem", "span": {"start": 0, "end": 27},
            "public": false,
            "attributes": [],
            "name": {"kind": "StrExpr", "span": {"start": 5, "end": 11},
                     "parts": [{"kind": "StrText", "span": {"start": 6, "end": 10}, "value": "adds"}]},
            "body": {"kind": "Block", "span": {"start": 12, "end": 27}, "stmts": [
                {"kind": "ExprStmt", "span": {"start": 18, "end": 25},
                 "expr": {"kind": "CallExpr", "span": {"start": 18, "end": 25},
                          "callee": {"kind": "NameExpr", "span": {"start": 18, "end": 23}, "name": "check"},
                          "args": []}}
            ]}
        }]"#,
    );
}

#[test]
fn integration_attribute_on_a_test() {
    // The item's span starts at its first attribute.
    assert_items(
        "#[integration]\ntest \"reads\" {}",
        r#"[{
            "kind": "TestItem", "span": {"start": 0, "end": 30},
            "public": false,
            "attributes": [
                {"kind": "Attribute", "span": {"start": 0, "end": 14},
                 "name": {"kind": "Ident", "span": {"start": 2, "end": 13}, "name": "integration"},
                 "args": []}
            ],
            "name": {"kind": "StrExpr", "span": {"start": 20, "end": 27},
                     "parts": [{"kind": "StrText", "span": {"start": 21, "end": 26}, "value": "reads"}]},
            "body": {"kind": "Block", "span": {"start": 28, "end": 30}, "stmts": []}
        }]"#,
    );
}

#[test]
fn attribute_with_arguments() {
    // Arguments are parsed like call arguments.
    assert_items(
        "#[integration(slow: true)] test \"t\" {}",
        r#"[{
            "kind": "TestItem", "span": {"start": 0, "end": 38},
            "public": false,
            "attributes": [
                {"kind": "Attribute", "span": {"start": 0, "end": 26},
                 "name": {"kind": "Ident", "span": {"start": 2, "end": 13}, "name": "integration"},
                 "args": [
                    {"kind": "Arg", "span": {"start": 14, "end": 24},
                     "label": {"kind": "Ident", "span": {"start": 14, "end": 18}, "name": "slow"},
                     "value": {"kind": "BoolExpr", "span": {"start": 20, "end": 24}, "value": true}}
                 ]}
            ],
            "name": {
                "kind": "StrExpr", "span": {"start": 32, "end": 35},
                "parts": [{"kind": "StrText", "span": {"start": 33, "end": 34}, "value": "t"}]
            },
            "body": {"kind": "Block", "span": {"start": 36, "end": 38}, "stmts": []}
        }]"#,
    );
}

#[test]
fn rejects_unknown_attribute() {
    // The error covers the misspelled name.
    let err = module_err("#[integation]\ntest \"t\" {}");
    assert_eq!(err.code, "E0132");
    assert_eq!(err.span, Span { start: 2, end: 12 });
    assert!(err.message.contains("`#[integration]`"), "{}", err.message);
}

#[test]
fn rejects_unsafe() {
    // v0 has no `unsafe` anywhere: not on items, not as a block.
    let err = module_err("unsafe fn f() {}");
    assert_eq!(err.code, "E0133");
    assert_eq!(err.span, Span { start: 0, end: 6 });

    let err = module_err("fn f() {\n    unsafe { g() }\n}");
    assert_eq!(err.code, "E0133");
    assert_eq!(err.span, Span { start: 13, end: 19 });
}
