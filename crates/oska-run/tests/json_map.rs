// SPDX-License-Identifier: Apache-2.0
//! `std.json`'s `Value.as_map()` (#604, ADR 0028): the members of an
//! object as a `Map`, in the order the text wrote them. A duplicate key is
//! an error, as `parse` reports it today. Every program also runs natively
//! on Linux and must print the same (`common/mod.rs`).

mod common;

use common::{assert_refused, assert_runs};

/// A program that imports `std.json`, parses the JSON text
/// `json` and runs `body` with `value` bound to the parsed value.
fn program(json: &str, body: &str) -> String {
    let escaped = json
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}");
    format!(
        "use std.json\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             let value = match json.parse(\"{escaped}\") {{\n        \
                 Ok(v) => v\n        \
                 Err(e) => {{\n            \
                     io.println(e.to_string())?\n            \
                     return Ok(())\n        \
                 }}\n    \
             }}\n{body}\n    Ok(())\n}}\n"
    )
}

#[test]
fn the_map_holds_the_members_in_text_order_and_finds_each_by_key() {
    let src = program(
        r#"{"zebra":1,"apple":"x","mango":[true]}"#,
        r#"    let m = match value.as_map() {
        Ok(m) => m
        Err(e) => {
            io.println(e.to_string())?
            return Ok(())
        }
    }
    let n = m.len()
    io.println("len {n}")?
    for e in m.entries() {
        let text = json.encode(e.value)
        io.println("{e.key} -> {text}")?
    }
    match m.get("apple") {
        Some(v) => {
            let text = json.encode(v)
            io.println("apple {text}")?
        }
        None => io.println("no apple")?
    }
    match m.get("kiwi") {
        Some(v) => {
            let text = json.encode(v)
            io.println("kiwi {text}")?
        }
        None => io.println("no kiwi")?
    }"#,
    );
    assert_runs(
        &src,
        &[],
        "len 3\nzebra -> 1\napple -> \"x\"\nmango -> [true]\napple \"x\"\nno kiwi\n",
    );
}

#[test]
fn an_empty_object_is_an_empty_map() {
    let src = program(
        "{}",
        r#"    match value.as_map() {
        Ok(m) => {
            let n = m.len()
            io.println("len {n}")?
        }
        Err(e) => io.println(e.to_string())?
    }"#,
    );
    assert_runs(&src, &[], "len 0\n");
}

#[test]
fn a_value_that_is_not_an_object_is_an_error() {
    let src = program(
        "[1]",
        r#"    match value.as_map() {
        Ok(m) => {
            let n = m.len()
            io.println("len {n}")?
        }
        Err(e) => io.println(e.to_string())?
    }"#,
    );
    assert_runs(&src, &[], "invalid JSON at byte 0: not an object\n");
}

#[test]
fn a_duplicate_key_gives_the_error_parse_gives() {
    // `parse` refuses a duplicate key, so a program that reaches one
    // builds the object by hand. The reason is the parser's.
    let src = "use std.json\n\
        fn member(key: String, n: Int) -> json.Member {\n    \
            json.Member(key: key, value: json.Value.Number(json.Number.from_int(n)))\n}\n\n\
        fn main(io: Stdout) -> Result<(), IoError> {\n    \
            let object = json.Value.Object([member(\"a\", 1), member(\"b\", 2), member(\"a\", 3), member(\"b\", 4)])\n    \
            match object.as_map() {\n        \
                Ok(_) => io.println(\"len\")?\n        \
                Err(e) => io.println(e.to_string())?\n    \
            }\n    \
            match json.parse(\"\\{\\\"a\\\":1,\\\"b\\\":2,\\\"a\\\":3\\}\") {\n        \
                Ok(_) => io.println(\"parsed\")?\n        \
                Err(e) => io.println(e.to_string())?\n    \
            }\n    \
            Ok(())\n}\n";
    assert_runs(
        src,
        &[],
        "invalid JSON at byte 2: duplicate key\ninvalid JSON at byte 13: duplicate key\n",
    );
}

#[test]
fn a_key_from_untrusted_text_stays_untrusted_in_the_map() {
    let src = "use std.json\n\
        fn trusted(text: String) -> String {\n    text\n}\n\n\
        fn first_key(raw: Untrusted<String>) -> String {\n    \
            let value = json.parse(raw).unwrap_or(json.Value.Null)\n    \
            let m = value.as_map().unwrap_or(Map.new())\n    \
            for e in m.entries() {\n        \
                return trusted(e.key)\n    \
            }\n    \
            \"\"\n}\n\n\
        fn main(io: Stdout) -> Result<(), IoError> {\n    \
            io.println(first_key(\"\\{\\\"k1\\\":1,\\\"k2\\\":2\\}\"))\n}\n";
    assert_refused(src, "E0300");
}

#[test]
fn a_parsed_object_gives_its_untrusted_keys_back_from_the_map() {
    let src = "use std.json\n\
        fn main(io: Stdout) -> Result<(), IoError> {\n    \
            let raw: Untrusted<String> = \"\\{\\\"k1\\\":1,\\\"k2\\\":2\\}\"\n    \
            let value = json.parse(raw).unwrap_or(json.Value.Null)\n    \
            let m = value.as_map().unwrap_or(Map.new())\n    \
            for e in m.entries() {\n        \
                let text = json.encode(e.value)\n        \
                io.println(\"{e.key}={text}\")?\n    \
            }\n    \
            Ok(())\n}\n";
    assert_runs(src, &[], "k1=1\nk2=2\n");
}
