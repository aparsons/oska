//! The toolchain's shared JSON reader: what `oskac hook claude` reads its
//! input with. It tells well-formed JSON from malformed, and it decodes
//! every escape, so a path with an escaped quote comes out as written.

use oska_syntax::{parse_json, JsonValue};

#[test]
fn reads_every_kind_of_value() {
    let value = parse_json(" {\"a\": [1, -2.5e3, true, false, null, \"x\"], \"b\": {}} ")
        .expect("well-formed");
    let a = value.get("a").expect("a key");
    assert_eq!(
        *a,
        JsonValue::Array(vec![
            JsonValue::Number("1".into()),
            JsonValue::Number("-2.5e3".into()),
            JsonValue::Bool(true),
            JsonValue::Bool(false),
            JsonValue::Null,
            JsonValue::Str("x".into()),
        ])
    );
    assert_eq!(value.get("b"), Some(&JsonValue::Object(Vec::new())));
    assert_eq!(value.get("c"), None);
}

#[test]
fn decodes_every_escape_in_a_string() {
    let value = parse_json(r#"{"p": "C:\\dir\\\"q\".osk \/ \b\f\n\r\t \u00e9 \ud83d\ude00"}"#)
        .expect("well-formed");
    assert_eq!(
        value.get("p").and_then(JsonValue::as_str),
        Some("C:\\dir\\\"q\".osk / \u{8}\u{c}\n\r\t \u{e9} \u{1f600}")
    );
}

#[test]
fn refuses_malformed_text() {
    for bad in [
        "",
        "{\"a\": }",
        "[1,]",
        "[1 2]",
        "\"a\nb\"",
        "{} x",
        "\"\\q\"",
        "\"\\ud800\"",
        "{\"a\" 1}",
        "{1: 2}",
        "01",
        "-",
        "1.",
        "tru",
        "\"open",
        "{\"a\": 1,}",
    ] {
        assert!(parse_json(bad).is_err(), "{bad:?} is not JSON");
    }
}

#[test]
fn refuses_nesting_deep_enough_to_exhaust_the_stack() {
    let deep = "[".repeat(100_000);
    assert!(parse_json(&deep).is_err());
}

#[test]
fn a_key_given_twice_reads_as_its_first() {
    let value = parse_json("{\"k\": \"first\", \"k\": \"second\"}").expect("well-formed");
    assert_eq!(value.get("k").and_then(JsonValue::as_str), Some("first"));
}
