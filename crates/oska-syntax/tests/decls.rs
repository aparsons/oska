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
        "pub use std.sql.{Db,\n query,}",
        r#"[{
            "kind": "UseItem", "span": {"start": 0, "end": 29},
            "public": true,
            "attributes": [],
            "path": [
                {"kind": "Ident", "span": {"start": 8, "end": 11}, "name": "std"},
                {"kind": "Ident", "span": {"start": 12, "end": 15}, "name": "sql"}
            ],
            "names": [
                {"kind": "Ident", "span": {"start": 17, "end": 19}, "name": "Db"},
                {"kind": "Ident", "span": {"start": 22, "end": 27}, "name": "query"}
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
