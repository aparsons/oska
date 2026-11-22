//! `std.json` (#248), run on both backends through the differential
//! harness: round trips of every kind of value, canonical output, every
//! escape, malformed and hostile input (truncation, bad escapes and
//! surrogates, duplicate keys, the depth and length limits, integers that
//! do not fit), the accessors, building values, and the schema builder,
//! validator and `decode`. The output is the same, byte for byte, on both.
//!
//! `fixtures/json_test_suite` is the `test_parsing` corpus of Nicolas
//! Seriot's JSONTestSuite (MIT, its `LICENSE` beside it), at commit
//! `1ef36fa01286573e846ac449e8683f8833c5b26a`: every `y_` file must parse,
//! every `n_` file must be refused, and an `i_` file may go either way,
//! all without a panic.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use std::path::Path;

use differential::{assert_same, Case};

/// Reads JSON lines from stdin and prints each one encoded, or why it
/// is refused.
const ECHO: &str = r#"use std.json

fn main(io: Stdout, stdin: Stdin) -> Result<(), IoError> {
    while true {
        let line = match stdin.read_line()? {
            Some(line) => line
            None => {
                    return Ok(())
                }
        }
        match json.parse(line) {
            Ok(value) => io.println(json.encode(value))?
            Err(error) => io.println(error.to_string())?
        }
    }
    Ok(())
}
"#;

#[test]
fn values_round_trip_in_canonical_form_and_malformed_input_is_refused() {
    let input = r#"null
true
false
0
-0
12
-3.25
1e10
2.5E-3
6.02e+23
""
"plain"
[]
{}
 [ 1 , [ 2 , { } ] , "x" ] 
{"b":1,"a":{"z":[null]}}
"\"\\\/\b\f\n\r\t"
"Aé€😀"
"line para end"
"é€😀"

[1,
{"a"
{"a":}
{"a":1,}
[1,]
[1 2]
{1:2}
nul
tru
"abc
"tab	in"
"\x"
"\u12"
"\u12G4"
"\ud800"
"\udc00"
"\ud800A"
01
-
1.
1.e5
1e
-a
+1
.5
NaN
[1]]
{"a":1,"a":1}
{"a":{"b":1,"b":2}}
{"k":1,"k":2}
"#;
    let outcome = assert_same(&Case::file("json_echo", ECHO).with_stdin(input));
    assert_eq!(
        outcome.stdout,
        r#"null
true
false
0
-0
12
-3.25
1e10
2.5e-3
6.02e+23
""
"plain"
[]
{}
[1,[2,{}],"x"]
{"b":1,"a":{"z":[null]}}
"\"\\/\b\f\n\r\t"
"Aé€😀"
"line\u2028para\u2029end"
"é€😀"
invalid JSON at byte 0: unexpected end of input
invalid JSON at byte 3: unexpected end of input
invalid JSON at byte 4: unexpected end of input
invalid JSON at byte 5: unexpected character
invalid JSON at byte 7: unexpected character
invalid JSON at byte 3: unexpected character
invalid JSON at byte 3: unexpected character
invalid JSON at byte 1: unexpected character
invalid JSON at byte 0: unexpected character
invalid JSON at byte 0: unexpected character
invalid JSON at byte 4: unexpected end of input
invalid JSON at byte 4: control character in a string
invalid JSON at byte 1: invalid escape
invalid JSON at byte 1: invalid escape
invalid JSON at byte 1: invalid escape
invalid JSON at byte 1: invalid surrogate
invalid JSON at byte 1: invalid surrogate
invalid JSON at byte 1: invalid surrogate
invalid JSON at byte 1: trailing characters after the value
invalid JSON at byte 0: invalid number
invalid JSON at byte 0: invalid number
invalid JSON at byte 0: invalid number
invalid JSON at byte 0: invalid number
invalid JSON at byte 0: invalid number
invalid JSON at byte 0: unexpected character
invalid JSON at byte 0: unexpected character
invalid JSON at byte 0: unexpected character
invalid JSON at byte 3: trailing characters after the value
invalid JSON at byte 7: duplicate key
invalid JSON at byte 12: duplicate key
invalid JSON at byte 7: duplicate key
"#
    );
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn every_control_character_and_line_separator_is_escaped() {
    let escapes: String = (0..32).map(|c| format!("\\u{c:04x}")).collect();
    let input = format!("\"{escapes}\u{7f}\\u2028\\u2029\"\n");
    let outcome = assert_same(&Case::file("json_escapes", ECHO).with_stdin(&input));
    assert_eq!(
        outcome.stdout,
        "\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f\
         \\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f\
         \u{7f}\\u2028\\u2029\"\n"
    );
    assert!(!outcome.stdout.trim_end().contains('\n'));
}

/// The limits, numbers, accessors, built values, and schemas.
const API: &str = r#"use std.json

fn say(io: Stdout, label: String, line: Untrusted<String>) {
    let _ = io.println("{label}: {line}")
}

fn parsed(text: Untrusted<String>) -> Untrusted<String> {
    match json.parse(text) {
        Ok(value) => json.encode(value)
        Err(error) => error.to_string()
    }
}

fn nested(depth: Int) -> String {
    let open = "[".repeat(depth)
    let close = "]".repeat(depth)
    "{open}{close}"
}

fn int_of(text: String) -> String {
    match json.parse(text) {
        Ok(value) => match value.as_int() {
            Some(n) => "{n}"
            None => "none"
        }
        Err(error) => error.to_string()
    }
}

fn many_keys(count: Int, repeat: Int) -> String {
    var parts: List<String> = []
    for index in 0..count {
        parts.push("\"key{index}\":{index}")
    }
    parts.push("\"key{repeat}\":0")
    let members = ",".join(parts)
    "\{{members}\}"
}

struct Point { x: Int, y: Int, label: Untrusted<String> }

fn point(value: json.Value) -> Point {
    Point(
        x: value.int("x").unwrap_or(0),
        y: value.int("y").unwrap_or(0),
        label: value.text("label").unwrap_or("none"),
    )
}

fn decoded(io: Stdout, schema: json.Schema, text: String) {
    let line = match json.decode(text, schema, point) {
        Ok(p) => "{p.x},{p.y} {p.label}"
        Err(error) => error.to_string()
    }
    say(io, "decode {text}", line)
}

fn validated(io: Stdout, schema: json.Schema, text: String) {
    let line = match json.parse(text) {
        Ok(value) => match schema.validate(value) {
            Ok(()) => "ok"
            Err(violation) => violation.to_string()
        }
        Err(error) => error.to_string()
    }
    say(io, "validate {text}", line)
}

fn main(io: Stdout) -> Result<(), IoError> {
    let deepest = nested(json.MAX_DEPTH)
    let too_deep = nested(json.MAX_DEPTH + 1)
    say(io, "depth at the limit", parsed(deepest).slice(0, 4))
    say(io, "depth past the limit", parsed(too_deep))
    let filler = "a".repeat(json.MAX_LENGTH - 2)
    say(io, "length past the limit", parsed("\"{filler}a\""))
    say(io, "huge integer", int_of("123456789012345678901234567890"))
    say(io, "largest Int", int_of("9223372036854775807"))
    say(io, "smallest Int", int_of("-9223372036854775808"))
    say(io, "fraction", int_of("1.0"))
    say(io, "exponent", int_of("1e2"))
    say(io, "not a number", int_of("\"1\""))
    say(io, "many keys", parsed(many_keys(2000, 2001)).slice(0, 12))
    say(io, "many keys, one repeated", parsed(many_keys(2000, 7)))

    let doc = "\{\"name\":\"Ada\",\"age\":36,\"admin\":false,\"tags\":[\"a\",\"b\"],\"home\":\{\"city\":\"London\"\},\"none\":null\}"
    let value = match json.parse(doc) {
        Ok(value) => value
        Err(_) => json.Value.Null
    }
    say(io, "text", value.text("name").unwrap_or("missing"))
    let age = value.int("age").unwrap_or(-1)
    say(io, "int", "{age}")
    let admin = match value.bool("admin") {
        Some(true) => "true"
        Some(false) => "false"
        None => "missing"
    }
    say(io, "bool", admin)
    let tags = value.list("tags").unwrap_or([]).len()
    say(io, "list", "{tags}")
    let city = match value.object("home") {
        Some(home) => home.text("city").unwrap_or("missing")
        None => "missing"
    }
    say(io, "object", city)
    say(io, "wrong type", value.text("age").unwrap_or("none"))
    say(io, "missing", value.text("nope").unwrap_or("none"))
    let null = match value.get("none") {
        Some(member) => member.is_null()
        None => false
    }
    let null_text = if null { "yes" } else { "no" }
    say(io, "null", null_text)

    let built = json.Value.Object([
        json.Member(key: "jsonrpc", value: json.Value.Text("2.0")),
        json.Member(key: "id", value: json.Value.Number(json.Number.from_int(-7))),
        json.Member(key: "ok", value: json.Value.Bool(true)),
        json.Member(key: "items", value: json.Value.List([json.Value.Null, json.Value.Text("a\nb")])),
    ])
    say(io, "built", json.encode(built))
    let decimal = match json.Number.parse("-1.5e3") {
        Some(n) => n.text()
        None => "none"
    }
    say(io, "number text", decimal)
    let bad = match json.Number.parse("1.5x") {
        Some(n) => n.text()
        None => "none"
    }
    say(io, "bad number text", bad)

    let schema = json.Schema.object([
        json.Field.required("x", json.Schema.integer()),
        json.Field.required("y", json.Schema.integer().describe("The row.")),
        json.Field.optional("label", json.Schema.string()),
        json.Field.optional("kind", json.Schema.one_of(["dot", "cross"])),
        json.Field.optional("weights", json.Schema.list(json.Schema.number())),
        json.Field.optional("on", json.Schema.boolean()),
    ])
    say(io, "schema", json.encode(schema.to_value()))
    decoded(io, schema, "\{\"x\":1,\"y\":2,\"label\":\"here\"\}")
    decoded(io, schema, "\{\"x\":1\}")
    decoded(io, schema, "\{\"x\":1,")
    validated(io, schema, "\{\"x\":1,\"y\":2.5\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"extra\":true\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"kind\":\"star\"\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"kind\":\"dot\",\"on\":true\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"weights\":[1,2.5,\"3\"]\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"on\":\"yes\"\}")
    validated(io, schema, "[1]")
    validated(io, schema, "\{\"x\":99999999999999999999,\"y\":2\}")
    validated(io, schema, "\{\"x\":2.0,\"y\":3e2\}")
    validated(io, schema, "\{\"x\":1e400,\"y\":-0.0\}")
    validated(io, schema, "\{\"x\":30e-1,\"y\":1e-400\}")
    validated(io, schema, "\{\"x\":1,\"y\":2,\"a b\":true\}")
    decoded(io, schema, "\{\"x\":2.0,\"y\":2\}")
    Ok(())
}
"#;

#[test]
fn limits_accessors_built_values_and_schemas() {
    let outcome = assert_same(&Case::file("json_api", API));
    assert_eq!(
        outcome.stdout,
        r#"depth at the limit: [[[[
depth past the limit: invalid JSON at byte 128: nesting deeper than the limit
length past the limit: invalid JSON at byte 8388608: input longer than the limit
huge integer: none
largest Int: 9223372036854775807
smallest Int: -9223372036854775808
fraction: none
exponent: none
not a number: none
many keys: {"key0":0,"k
many keys, one repeated: invalid JSON at byte 27781: duplicate key
text: Ada
int: 36
bool: false
list: 2
object: London
wrong type: none
missing: none
null: yes
built: {"jsonrpc":"2.0","id":-7,"ok":true,"items":[null,"a\nb"]}
number text: -1.5e3
bad number text: none
schema: {"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer","description":"The row."},"label":{"type":"string"},"kind":{"type":"string","enum":["dot","cross"]},"weights":{"type":"array","items":{"type":"number"}},"on":{"type":"boolean"}},"required":["x","y"],"additionalProperties":false}
decode {"x":1,"y":2,"label":"here"}: 1,2 here
decode {"x":1}: $.y: expected a required member
decode {"x":1,: invalid JSON at byte 7: unexpected end of input
validate {"x":1,"y":2.5}: $.y: expected an integer
validate {"x":1,"y":2,"extra":true}: $.extra: expected no member by this name
validate {"x":1,"y":2,"kind":"star"}: $.kind: expected one of the schema's choices
validate {"x":1,"y":2,"kind":"dot","on":true}: ok
validate {"x":1,"y":2,"weights":[1,2.5,"3"]}: $.weights[2]: expected a number
validate {"x":1,"y":2,"on":"yes"}: $.on: expected a boolean
validate [1]: $: expected an object
validate {"x":99999999999999999999,"y":2}: ok
validate {"x":2.0,"y":3e2}: ok
validate {"x":1e400,"y":-0.0}: ok
validate {"x":30e-1,"y":1e-400}: $.y: expected an integer
validate {"x":1,"y":2,"a b":true}: $.(member 3): expected no member by this name
decode {"x":2.0,"y":2}: 0,2 none
"#
    );
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn modules_of_a_package_share_the_module() {
    let outcome = assert_same(&Case::package(
        "json_package",
        &[
            ("oska.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"),
            (
                "src/main.osk",
                "use std.json\nuse pkg.wire\n\n\
                 fn main(io: Stdout) -> Result<(), IoError> {\n    \
                 let reply: json.Value = wire.ping()\n    \
                 io.println(json.encode(reply))\n}\n",
            ),
            (
                "src/wire.osk",
                "use std.json\n\n\
                 pub fn ping() -> json.Value {\n    \
                 json.Value.Object([json.Member(key: \"ping\", value: json.Value.Bool(true))])\n}\n",
            ),
        ],
    ));
    assert_eq!(outcome.stdout, "{\"ping\":true}\n");
}

/// Parses every file under `suite`, printing `<name> ok`, `<name>
/// refused` or, for bytes that are not UTF-8, `<name> not text`.
const SUITE: &str = r#"use std.json

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let suite = fs.restrict_to("suite")
    for name in suite.list_dir(Path.new("."))? {
        let bytes = suite.read_bytes(Path.from(name))?
        let outcome = match bytes.to_text() {
            Some(text) => match json.parse(text) {
                Ok(_) => "ok"
                Err(_) => "refused"
            }
            None => "not text"
        }
        io.println("{name} {outcome}")?
    }
    Ok(())
}
"#;

/// The suite's `y_` files that `std.json` refuses by design: a key that
/// occurs twice in one object is an error, not "last wins" (spec #229).
const DUPLICATE_KEYS: [&str; 2] = [
    "y_object_duplicated_key.json",
    "y_object_duplicated_key_and_value.json",
];

/// The suite's files, in a sorted list.
fn suite_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_test_suite/test_parsing")
}

fn suite_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(suite_dir())
        .expect("the suite should list")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Copy the suite's files, byte for byte, into `suite` under `root`.
fn copy_suite(root: &Path) {
    let into = root.join("suite");
    std::fs::create_dir_all(&into).expect("the suite directory should be made");
    for name in suite_names() {
        std::fs::copy(suite_dir().join(&name), into.join(&name)).expect("a suite file should copy");
    }
}

#[test]
fn the_json_test_suite_is_parsed_as_it_requires() {
    let names = suite_names();
    assert!(names.len() > 300, "{names:?}");
    let case = Case::file("json_test_suite", SUITE).with_setup(copy_suite);
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let mut wrong = Vec::new();
    let mut seen = Vec::new();
    for line in outcome.stdout.lines() {
        let (name, result) = line.split_once(' ').expect("a line names a file");
        seen.push(name.to_string());
        let fine = match &name[..2] {
            _ if DUPLICATE_KEYS.contains(&name) => result == "refused",
            "y_" => result == "ok",
            "n_" => result != "ok",
            _ => true,
        };
        if !fine {
            wrong.push(line.to_string());
        }
    }
    seen.sort();
    assert_eq!(seen, names);
    assert!(
        wrong.is_empty(),
        "files parsed against the suite: {wrong:#?}"
    );
}
