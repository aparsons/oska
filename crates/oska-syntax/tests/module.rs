//! `parse_module` tests for modules of plain functions, asserted through
//! the JSON export.

mod common;

use common::{assert_items, module_err};
use oska_syntax::Span;

#[test]
fn empty_function() {
    assert_items(
        "fn main() {}",
        r#"[{
            "kind": "FnItem", "span": {"start": 0, "end": 12},
            "public": false,
            "name": {"kind": "Ident", "span": {"start": 3, "end": 7}, "name": "main"},
            "params": [],
            "body": {"kind": "Block", "span": {"start": 10, "end": 12}, "stmts": []}
        }]"#,
    );
}

#[test]
fn parameters_and_return_type() {
    // Capability parameters are ordinary parameters with their types.
    assert_items(
        "fn f(fs: FsRead, k: fn(Int) -> ()) -> Result<(), std.io.Error> {}",
        r#"[{
            "kind": "FnItem", "span": {"start": 0, "end": 65},
            "public": false,
            "name": {"kind": "Ident", "span": {"start": 3, "end": 4}, "name": "f"},
            "params": [
                {"kind": "Param", "span": {"start": 5, "end": 15},
                 "name": {"kind": "Ident", "span": {"start": 5, "end": 7}, "name": "fs"},
                 "type": {"kind": "PathType", "span": {"start": 9, "end": 15},
                          "segments": [{"kind": "Ident", "span": {"start": 9, "end": 15}, "name": "FsRead"}],
                          "args": []}},
                {"kind": "Param", "span": {"start": 17, "end": 33},
                 "name": {"kind": "Ident", "span": {"start": 17, "end": 18}, "name": "k"},
                 "type": {"kind": "FnType", "span": {"start": 20, "end": 33},
                          "cap": false,
                          "params": [{"kind": "PathType", "span": {"start": 23, "end": 26},
                                      "segments": [{"kind": "Ident", "span": {"start": 23, "end": 26}, "name": "Int"}],
                                      "args": []}],
                          "return_type": {"kind": "UnitType", "span": {"start": 31, "end": 33}}}}
            ],
            "return_type": {
                "kind": "PathType", "span": {"start": 38, "end": 62},
                "segments": [{"kind": "Ident", "span": {"start": 38, "end": 44}, "name": "Result"}],
                "args": [
                    {"kind": "UnitType", "span": {"start": 45, "end": 47}},
                    {"kind": "PathType", "span": {"start": 49, "end": 61},
                     "segments": [
                         {"kind": "Ident", "span": {"start": 49, "end": 52}, "name": "std"},
                         {"kind": "Ident", "span": {"start": 53, "end": 55}, "name": "io"},
                         {"kind": "Ident", "span": {"start": 56, "end": 61}, "name": "Error"}
                     ],
                     "args": []}
                ]
            },
            "body": {"kind": "Block", "span": {"start": 63, "end": 65}, "stmts": []}
        }]"#,
    );
}

#[test]
fn public_functions_on_separate_lines() {
    assert_items(
        "\n// leading comment\npub fn a() {}\n\nfn b(\n    x: T,\n) {}\n",
        r#"[
            {"kind": "FnItem", "span": {"start": 20, "end": 33},
             "public": true,
             "name": {"kind": "Ident", "span": {"start": 27, "end": 28}, "name": "a"},
             "params": [],
             "body": {"kind": "Block", "span": {"start": 31, "end": 33}, "stmts": []}},
            {"kind": "FnItem", "span": {"start": 35, "end": 55},
             "public": false,
             "name": {"kind": "Ident", "span": {"start": 38, "end": 39}, "name": "b"},
             "params": [
                 {"kind": "Param", "span": {"start": 45, "end": 49},
                  "name": {"kind": "Ident", "span": {"start": 45, "end": 46}, "name": "x"},
                  "type": {"kind": "PathType", "span": {"start": 48, "end": 49},
                           "segments": [{"kind": "Ident", "span": {"start": 48, "end": 49}, "name": "T"}],
                           "args": []}}
             ],
             "body": {"kind": "Block", "span": {"start": 53, "end": 55}, "stmts": []}}
        ]"#,
    );
}

#[test]
fn items_need_a_line_break_between_them() {
    let err = module_err("fn a() {} fn b() {}");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 10, end: 12 });
}
