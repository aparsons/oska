//! `parse_module` tests for `struct` and `enum` declarations, asserted
//! through the JSON export.

mod common;

use common::{assert_items, module_err};
use oska_syntax::Span;

/// The JSON for a single-segment path type such as `Int` at `start`.
fn path_ty(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(
        r#"{{"kind": "PathType", "span": {{"start": {start}, "end": {end}}},
            "segments": [{{"kind": "Ident", "span": {{"start": {start}, "end": {end}}}, "name": "{name}"}}],
            "args": []}}"#
    )
}

/// The JSON for an `Ident` node.
fn ident(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(r#"{{"kind": "Ident", "span": {{"start": {start}, "end": {end}}}, "name": "{name}"}}"#)
}

#[test]
fn named_field_struct_with_per_field_pub() {
    let src = "struct Point { x: Int, pub y: Int }";
    assert_items(
        src,
        &format!(
            r#"[{{
                "kind": "StructItem", "span": {{"start": 0, "end": 35}},
                "public": false,
                "cap": false,
                "name": {name},
                "form": "named",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 15, "end": 21}},
                      "public": false, "name": {x}, "type": {int1}}},
                    {{"kind": "Field", "span": {{"start": 23, "end": 33}},
                      "public": true, "name": {y}, "type": {int2}}}
                ]
            }}]"#,
            name = ident("Point", 7),
            x = ident("x", 15),
            int1 = path_ty("Int", 18),
            y = ident("y", 27),
            int2 = path_ty("Int", 30),
        ),
    );
}

#[test]
fn positional_struct_with_trailing_comma() {
    assert_items(
        "struct Meters(pub Int, Float,)",
        &format!(
            r#"[{{
                "kind": "StructItem", "span": {{"start": 0, "end": 30}},
                "public": false,
                "cap": false,
                "name": {name},
                "form": "positional",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 14, "end": 21}},
                      "public": true, "type": {int}}},
                    {{"kind": "Field", "span": {{"start": 23, "end": 28}},
                      "public": false, "type": {float}}}
                ]
            }}]"#,
            name = ident("Meters", 7),
            int = path_ty("Int", 18),
            float = path_ty("Float", 23),
        ),
    );
}

#[test]
fn public_cap_struct_over_several_lines() {
    // The item span starts at `pub`. Fields may sit on their own lines.
    let src = "pub cap struct Logger {\n    out: Stdout,\n}";
    assert_items(
        src,
        &format!(
            r#"[{{
                "kind": "StructItem", "span": {{"start": 0, "end": 42}},
                "public": true,
                "cap": true,
                "name": {name},
                "form": "named",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 28, "end": 39}},
                      "public": false, "name": {out}, "type": {stdout}}}
                ]
            }}]"#,
            name = ident("Logger", 15),
            out = ident("out", 28),
            stdout = path_ty("Stdout", 33),
        ),
    );
}

#[test]
fn rejects_missing_comma_between_fields() {
    // A type ending in `>` never ends a line, so without the comma `y`
    // would run into `List<Int>`. The span is empty, where the comma goes.
    let err = module_err("struct A {\n    xs: List<Int>\n    y: Int\n}");
    assert_eq!(err.code, "E0120");
    assert_eq!(err.span, Span { start: 28, end: 28 });
    assert!(err.message.contains("add a comma"), "{}", err.message);

    let err = module_err("struct A {\n    x: Int\n    y: Int\n}");
    assert_eq!(err.code, "E0120");
    assert_eq!(err.span, Span { start: 21, end: 21 });

    let err = module_err("struct P(Int Int)");
    assert_eq!(err.code, "E0120");
    assert_eq!(err.span, Span { start: 12, end: 12 });
}
