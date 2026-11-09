//! The JSON export of function bodies: every statement and expression node.

mod common;

use common::assert_body;

#[test]
fn let_assign_and_bare_return() {
    assert_body(
        "fn f() {\n    var x: Int = 1\n    x = -x\n    return\n}",
        r#"[
            {"kind": "LetStmt", "span": {"start": 13, "end": 27},
             "mutable": true,
             "name": {"kind": "Ident", "span": {"start": 17, "end": 18}, "name": "x"},
             "type": {"kind": "PathType", "span": {"start": 20, "end": 23},
                      "segments": [{"kind": "Ident", "span": {"start": 20, "end": 23}, "name": "Int"}],
                      "args": []},
             "value": {"kind": "IntExpr", "span": {"start": 26, "end": 27}, "value": "1"}},
            {"kind": "AssignStmt", "span": {"start": 32, "end": 38},
             "target": {"kind": "NameExpr", "span": {"start": 32, "end": 33}, "name": "x"},
             "value": {"kind": "UnaryExpr", "span": {"start": 36, "end": 38}, "op": "-",
                       "operand": {"kind": "NameExpr", "span": {"start": 37, "end": 38}, "name": "x"}}},
            {"kind": "ReturnStmt", "span": {"start": 43, "end": 49}}
        ]"#,
    );
}

#[test]
fn loops_and_return_value() {
    assert_body(
        "fn f() {\n    for i in 0..n { break }\n    while !done { continue }\n    return a && b\n}",
        r#"[
            {"kind": "ForStmt", "span": {"start": 13, "end": 36},
             "binding": {"kind": "Ident", "span": {"start": 17, "end": 18}, "name": "i"},
             "iter": {"kind": "RangeExpr", "span": {"start": 22, "end": 26},
                      "start": {"kind": "IntExpr", "span": {"start": 22, "end": 23}, "value": "0"},
                      "end": {"kind": "NameExpr", "span": {"start": 25, "end": 26}, "name": "n"}},
             "body": {"kind": "Block", "span": {"start": 27, "end": 36},
                      "stmts": [{"kind": "BreakStmt", "span": {"start": 29, "end": 34}}]}},
            {"kind": "WhileStmt", "span": {"start": 41, "end": 65},
             "cond": {"kind": "UnaryExpr", "span": {"start": 47, "end": 52}, "op": "!",
                      "operand": {"kind": "NameExpr", "span": {"start": 48, "end": 52}, "name": "done"}},
             "body": {"kind": "Block", "span": {"start": 53, "end": 65},
                      "stmts": [{"kind": "ContinueStmt", "span": {"start": 55, "end": 63}}]}},
            {"kind": "ReturnStmt", "span": {"start": 70, "end": 83},
             "value": {"kind": "BinaryExpr", "span": {"start": 77, "end": 83}, "op": "&&",
                       "lhs": {"kind": "NameExpr", "span": {"start": 77, "end": 78}, "name": "a"},
                       "rhs": {"kind": "NameExpr", "span": {"start": 82, "end": 83}, "name": "b"}}}
        ]"#,
    );
}

#[test]
fn calls_fields_lists_and_literals() {
    assert_body(
        r#"fn f() { g(self.v, to: [1, "a{b.c}!"], sql"q")? }"#,
        r#"[
            {"kind": "ExprStmt", "span": {"start": 9, "end": 47},
             "expr": {"kind": "TryExpr", "span": {"start": 9, "end": 47},
              "operand": {"kind": "CallExpr", "span": {"start": 9, "end": 46},
               "callee": {"kind": "NameExpr", "span": {"start": 9, "end": 10}, "name": "g"},
               "args": [
                 {"kind": "Arg", "span": {"start": 11, "end": 17},
                  "value": {"kind": "FieldExpr", "span": {"start": 11, "end": 17},
                            "base": {"kind": "SelfExpr", "span": {"start": 11, "end": 15}},
                            "name": {"kind": "Ident", "span": {"start": 16, "end": 17}, "name": "v"}}},
                 {"kind": "Arg", "span": {"start": 19, "end": 37},
                  "label": {"kind": "Ident", "span": {"start": 19, "end": 21}, "name": "to"},
                  "value": {"kind": "ListExpr", "span": {"start": 23, "end": 37},
                   "elements": [
                     {"kind": "IntExpr", "span": {"start": 24, "end": 25}, "value": "1"},
                     {"kind": "StrExpr", "span": {"start": 27, "end": 36},
                      "parts": [
                        {"kind": "StrText", "span": {"start": 28, "end": 29}, "value": "a"},
                        {"kind": "StrHole", "span": {"start": 29, "end": 34},
                         "path": [{"kind": "Ident", "span": {"start": 30, "end": 31}, "name": "b"},
                                  {"kind": "Ident", "span": {"start": 32, "end": 33}, "name": "c"}]},
                        {"kind": "StrText", "span": {"start": 34, "end": 35}, "value": "!"}
                      ]}
                   ]}},
                 {"kind": "Arg", "span": {"start": 39, "end": 45},
                  "value": {"kind": "SinkStrExpr", "span": {"start": 39, "end": 45},
                            "tag": "sql", "value": "q"}}
               ]}}}
        ]"#,
    );
}

#[test]
fn string_text_spans_cover_escapes() {
    // `\n` is two source bytes; the text part's span covers both.
    assert_body(
        r#"fn f() { "a\n{x}" }"#,
        r#"[
            {"kind": "ExprStmt", "span": {"start": 9, "end": 17},
             "expr": {"kind": "StrExpr", "span": {"start": 9, "end": 17},
              "parts": [
                {"kind": "StrText", "span": {"start": 10, "end": 13}, "value": "a\n"},
                {"kind": "StrHole", "span": {"start": 13, "end": 16},
                 "path": [{"kind": "Ident", "span": {"start": 14, "end": 15}, "name": "x"}]}
              ]}}
        ]"#,
    );
}

#[test]
fn if_else_chain_and_closure() {
    assert_body(
        "fn f() { if c { true } else if d { () } else { |x: Int, y| x } }",
        r#"[
            {"kind": "ExprStmt", "span": {"start": 9, "end": 62},
             "expr": {"kind": "IfExpr", "span": {"start": 9, "end": 62},
              "cond": {"kind": "NameExpr", "span": {"start": 12, "end": 13}, "name": "c"},
              "then_block": {"kind": "Block", "span": {"start": 14, "end": 22},
               "stmts": [{"kind": "ExprStmt", "span": {"start": 16, "end": 20},
                          "expr": {"kind": "BoolExpr", "span": {"start": 16, "end": 20}, "value": true}}]},
              "else_branch": {"kind": "IfExpr", "span": {"start": 28, "end": 62},
               "cond": {"kind": "NameExpr", "span": {"start": 31, "end": 32}, "name": "d"},
               "then_block": {"kind": "Block", "span": {"start": 33, "end": 39},
                "stmts": [{"kind": "ExprStmt", "span": {"start": 35, "end": 37},
                           "expr": {"kind": "UnitExpr", "span": {"start": 35, "end": 37}}}]},
               "else_branch": {"kind": "BlockExpr", "span": {"start": 45, "end": 62},
                "stmts": [{"kind": "ExprStmt", "span": {"start": 47, "end": 60},
                 "expr": {"kind": "ClosureExpr", "span": {"start": 47, "end": 60},
                  "params": [
                    {"kind": "ClosureParam", "span": {"start": 48, "end": 54},
                     "name": {"kind": "Ident", "span": {"start": 48, "end": 49}, "name": "x"},
                     "type": {"kind": "PathType", "span": {"start": 51, "end": 54},
                              "segments": [{"kind": "Ident", "span": {"start": 51, "end": 54}, "name": "Int"}],
                              "args": []}},
                    {"kind": "ClosureParam", "span": {"start": 56, "end": 57},
                     "name": {"kind": "Ident", "span": {"start": 56, "end": 57}, "name": "y"}}
                  ],
                  "body": {"kind": "NameExpr", "span": {"start": 59, "end": 60}, "name": "x"}}}]}}}}
        ]"#,
    );
}

#[test]
fn block_and_scope_expressions() {
    assert_body(
        "fn f() { let v = { scope |s| { s } } }",
        r#"[
            {"kind": "LetStmt", "span": {"start": 9, "end": 36},
             "mutable": false,
             "name": {"kind": "Ident", "span": {"start": 13, "end": 14}, "name": "v"},
             "value": {"kind": "BlockExpr", "span": {"start": 17, "end": 36},
              "stmts": [{"kind": "ExprStmt", "span": {"start": 19, "end": 34},
               "expr": {"kind": "ScopeExpr", "span": {"start": 19, "end": 34},
                "handle": {"kind": "Ident", "span": {"start": 26, "end": 27}, "name": "s"},
                "body": {"kind": "Block", "span": {"start": 29, "end": 34},
                 "stmts": [{"kind": "ExprStmt", "span": {"start": 31, "end": 32},
                            "expr": {"kind": "NameExpr", "span": {"start": 31, "end": 32}, "name": "s"}}]}}}]}}
        ]"#,
    );
}
