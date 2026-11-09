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
fn enum_with_each_variant_form() {
    let src = "enum Shape {\n    Circle { radius: Int },\n    Rect(Int, Int),\n    Empty,\n}";
    assert_items(
        src,
        &format!(
            r#"[{{
                "kind": "EnumItem", "span": {{"start": 0, "end": 73}},
                "public": false,
                "cap": false,
                "name": {shape},
                "variants": [
                    {{"kind": "Variant", "span": {{"start": 17, "end": 39}},
                      "name": {circle}, "form": "named",
                      "fields": [
                          {{"kind": "Field", "span": {{"start": 26, "end": 37}},
                            "public": false, "name": {radius}, "type": {int1}}}
                      ]}},
                    {{"kind": "Variant", "span": {{"start": 45, "end": 59}},
                      "name": {rect}, "form": "positional",
                      "fields": [
                          {{"kind": "Field", "span": {{"start": 50, "end": 53}},
                            "public": false, "type": {int2}}},
                          {{"kind": "Field", "span": {{"start": 55, "end": 58}},
                            "public": false, "type": {int3}}}
                      ]}},
                    {{"kind": "Variant", "span": {{"start": 65, "end": 70}},
                      "name": {empty}, "form": "unit", "fields": []}}
                ]
            }}]"#,
            shape = ident("Shape", 5),
            circle = ident("Circle", 17),
            radius = ident("radius", 26),
            int1 = path_ty("Int", 34),
            rect = ident("Rect", 45),
            int2 = path_ty("Int", 50),
            int3 = path_ty("Int", 55),
            empty = ident("Empty", 65),
        ),
    );
}

#[test]
fn public_cap_enum() {
    assert_items(
        "pub cap enum Handle { File(FsRead) }",
        &format!(
            r#"[{{
                "kind": "EnumItem", "span": {{"start": 0, "end": 36}},
                "public": true,
                "cap": true,
                "name": {handle},
                "variants": [
                    {{"kind": "Variant", "span": {{"start": 22, "end": 34}},
                      "name": {file}, "form": "positional",
                      "fields": [
                          {{"kind": "Field", "span": {{"start": 27, "end": 33}},
                            "public": false, "type": {fs}}}
                      ]}}
                ]
            }}]"#,
            handle = ident("Handle", 13),
            file = ident("File", 22),
            fs = path_ty("FsRead", 27),
        ),
    );
}

#[test]
fn rejects_missing_comma_between_variants() {
    let err = module_err("enum E {\n    A(Int)\n    B\n}");
    assert_eq!(err.code, "E0120");
    assert_eq!(err.span, Span { start: 19, end: 19 });
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
