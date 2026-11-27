// SPDX-License-Identifier: Apache-2.0
//! `oskac doc`, the API reference (spec #294).
//!
//! The reading rules (descriptions, private items, signatures, authority)
//! are tested on fixture modules through `oska_check::api`, the code the
//! command lists the toolchain's modules with. The command itself is run
//! on the real prelude and `std`: coverage, `std.rt` left out, filters,
//! JSON against text, and the "checks only" marks against "What works
//! today".
//!
//! Golden files are in `tests/golden/`. To regenerate them after an
//! intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac --test
//! doc_cli` and review the diff.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oska_check::api::{module_items, ApiItem, Kind};
use oska_syntax::{parse_json, JsonValue};

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assert_golden(name: &str, actual: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// `oskac doc [args] --format=json`, parsed: its items.
fn json_items(args: &[&str]) -> Vec<JsonValue> {
    let mut all = vec!["doc"];
    all.extend_from_slice(args);
    all.push("--format=json");
    let output = oskac(&all);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let json = parse_json(&text(&output.stdout)).expect("the output should be JSON");
    json.get("items")
        .and_then(JsonValue::as_array)
        .expect("the output should have an items array")
        .to_vec()
}

fn field<'a>(item: &'a JsonValue, key: &str) -> &'a str {
    item.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_else(|| panic!("item {item:?} should have a string `{key}`"))
}

fn fixture(src: &str) -> Vec<ApiItem> {
    module_items("std.fixture", src).expect("the fixture should parse")
}

fn named<'a>(items: &'a [ApiItem], name: &str) -> &'a ApiItem {
    items
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("`{name}` should be listed in {items:#?}"))
}

// ---- Reading rules, on fixture modules -------------------------------------

#[test]
fn a_description_is_the_comment_block_directly_above_an_item() {
    let items = fixture(
        "// The module: a fixture.\n\
         //\n\
         // Its second paragraph.\n\
         \n\
         // Not about `apart`: a blank line follows.\n\
         \n\
         pub fn apart() -> Int\n\
         \n\
         // Doubles `n`,\n\
         // over two lines.\n\
         //\n\
         // A second paragraph.\n\
         pub fn double(n: Int) -> Int\n\
         pub fn bare() -> Int\n\
         \n\
         pub struct Point { pub x: Int }\n\
         \n\
         impl Point {\n    \
             // The x coordinate.\n    \
             pub fn x(self) -> Int\n\
         }\n",
    );
    let module = &items[0];
    assert_eq!(module.kind, Kind::Module);
    assert_eq!(
        module.description,
        "The module: a fixture.\n\nIts second paragraph."
    );
    assert_eq!(named(&items, "apart").description, "");
    assert_eq!(
        named(&items, "double").description,
        "Doubles `n`, over two lines.\n\nA second paragraph."
    );
    // The line above is code, not a comment.
    assert_eq!(named(&items, "bare").description, "");
    assert_eq!(named(&items, "Point.x").description, "The x coordinate.");
}

#[test]
fn private_items_and_fields_are_hidden() {
    let items = fixture(
        "pub struct Shown { pub open: Int, closed: Int }\n\
         struct Hidden { x: Int }\n\
         fn helper() -> Int\n\
         impl Shown {\n    \
             pub fn visible(self) -> Int\n    \
             fn internal(self) -> Int\n\
         }\n",
    );
    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["std.fixture", "Shown", "Shown.visible"]);
    assert_eq!(
        named(&items, "Shown").signature,
        "pub struct Shown { pub open: Int }"
    );
}

#[test]
fn signatures_are_as_written_on_one_line_and_authority_is_the_reports() {
    let items = fixture(
        "pub cap struct Handle { out: Stdout }\n\
         \n\
         pub fn write(\n    out: Stdout,\n    text: Untrusted<String>,\n) -> Result<(), IoError>\n\
         pub fn pure_one<T: Eq>(a: T, b: T) -> Bool\n\
         impl Handle {\n    \
             pub fn say(self, line: String) -> Result<(), IoError>\n\
         }\n\
         pub enum Choice<T> {\n    Yes(T),\n    No,\n}\n",
    );
    let write = named(&items, "write");
    assert_eq!(write.kind, Kind::Fn);
    assert_eq!(
        write.signature,
        "pub fn write(out: Stdout, text: Untrusted<String>) -> Result<(), IoError>"
    );
    assert_eq!(write.authority, "Stdout (out)");
    let pure_one = named(&items, "pure_one");
    assert_eq!(
        pure_one.signature,
        "pub fn pure_one<T: Eq>(a: T, b: T) -> Bool"
    );
    assert_eq!(pure_one.authority, "generic T (a), generic T (b)");
    let handle = named(&items, "Handle");
    assert_eq!(handle.kind, Kind::Struct);
    assert_eq!(handle.signature, "pub cap struct Handle");
    assert_eq!(handle.authority, "Stdout");
    assert_eq!(named(&items, "Handle.say").authority, "Stdout (self)");
    let choice = named(&items, "Choice");
    assert_eq!(choice.signature, "pub enum Choice<T> { Yes(T), No }");
    assert_eq!(choice.authority, "pure");
}

// ---- The command, on the real prelude and std ------------------------------

/// The public items each module's source declares, read apart from the
/// reference: each public function, type and trait, each trait's methods,
/// and each public method of an inherent `impl`, as `Type.method`.
fn declared_public(src: &str) -> Vec<String> {
    use oska_syntax::ast::ItemKind;
    let module = oska_syntax::parse_prelude(src).expect("the source should parse");
    let mut names = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) if item.is_public() => names.push(f.name.name.clone()),
            ItemKind::Struct(s) if item.is_public() => names.push(s.name.name.clone()),
            ItemKind::Enum(e) if item.is_public() => names.push(e.name.name.clone()),
            ItemKind::Trait(t) if item.is_public() => {
                names.push(t.name.name.clone());
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        names.push(format!("{}.{}", t.name.name, f.name.name));
                    }
                }
            }
            ItemKind::Impl(imp) if imp.trait_ref.is_none() => {
                let target = &src[imp.target.span.start..imp.target.span.end];
                let target = target.split('<').next().unwrap_or(target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        if method.is_public() {
                            names.push(format!("{target}.{}", f.name.name));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// A method's name without its impl's type arguments, as
/// `declared_public` names it: `Secret.rotl` for `Secret<U8>.rotl`.
fn unqualified(name: &str) -> String {
    match (name.find('<'), name.rfind(">.")) {
        (Some(open), Some(close)) if open < close && !name.contains(" for ") => {
            format!("{}{}", &name[..open], &name[close + 1..])
        }
        _ => name.to_string(),
    }
}

#[test]
fn every_public_item_of_the_prelude_and_std_is_listed() {
    let items = json_items(&[]);
    let sources = [
        ("prelude", "crates/oska-check/src/prelude.osk"),
        ("std.html", "crates/oska-check/src/std/html.osk"),
        ("std.json", "crates/oska-check/src/std/json.osk"),
        ("std.mcp", "crates/oska-check/src/std/mcp.osk"),
        ("std.sql", "crates/oska-check/src/std/sql.osk"),
        ("std.proc", "crates/oska-check/src/std/proc.osk"),
        ("std.map", "crates/oska-check/src/std/map.osk"),
    ];
    for (module, path) in sources {
        let src = std::fs::read_to_string(repo_root().join(path)).expect("the source should read");
        let declared = declared_public(&src);
        assert!(!declared.is_empty(), "{path} should declare public items");
        for name in declared {
            assert!(
                items
                    .iter()
                    .any(|i| field(i, "module") == module && unqualified(field(i, "name")) == name),
                "`{name}` of {module} should be in the reference"
            );
        }
    }
}

#[test]
fn the_modules_are_listed_in_order_and_std_rt_is_left_out() {
    let items = json_items(&[]);
    let mut modules: Vec<&str> = items.iter().map(|i| field(i, "module")).collect();
    modules.dedup();
    assert_eq!(
        modules,
        [
            "prelude",
            "std.html",
            "std.json",
            "std.mcp",
            "std.sql",
            "std.proc",
            "std.sha2",
            "std.dns",
            "std.hmac",
            "std.ed25519",
            "std.sshsig",
            "std.ecdsa",
            "std.rsa",
            "std.p256",
            "std.x509",
            "std.hkdf",
            "std.x25519",
            "std.chacha20poly1305",
            "std.aes128gcm",
            "std.tls",
            "std.http",
            "std.map"
        ]
    );
    assert!(items.iter().all(|i| !field(i, "name").starts_with("rt_")));
    let output = oskac(&["doc", "std.rt"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn every_item_has_every_field_and_a_unique_url() {
    let items = json_items(&[]);
    let mut urls = Vec::new();
    for item in &items {
        for key in [
            "module",
            "name",
            "kind",
            "signature",
            "description",
            "authority",
            "url",
        ] {
            field(item, key);
        }
        assert!(item.get("runs").and_then(JsonValue::as_bool).is_some());
        let url = field(item, "url");
        assert!(url.starts_with("https://oskalang.dev/api/"), "{url}");
        assert!(!urls.contains(&url), "two items share {url}");
        urls.push(url);
    }
}

#[test]
fn methods_that_impls_of_one_type_share_are_named_with_its_type_arguments() {
    let items = fixture(
        "pub struct Box<T> { value: T }\n\
         impl<T> Box<T> {\n    \
             // Shared by every box.\n    \
             pub fn get(self) -> T\n\
         }\n\
         impl Box<Int> {\n    \
             // An `Int` box's double.\n    \
             pub fn double(self) -> Int\n    \
             // An `Int` box's half.\n    \
             pub fn half(self) -> Int\n\
         }\n\
         impl Box<Bool> {\n    \
             // A `Bool` box's double.\n    \
             pub fn double(self) -> Bool\n\
         }\n",
    );
    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "std.fixture",
            "Box",
            "Box.get",
            "Box<Int>.double",
            "Box.half",
            "Box<Bool>.double"
        ]
    );
    assert_eq!(named(&items, "Box<Int>.double").anchor(), "boxintdouble");
    assert_eq!(named(&items, "Box<Bool>.double").anchor(), "boxbooldouble");
    assert_eq!(named(&items, "Box.half").anchor(), "boxhalf");
    assert_eq!(
        named(&items, "Box<Bool>.double").description,
        "A `Bool` box's double."
    );
    assert_eq!(
        named(&items, "Box<Int>.double").unqualified_name(),
        "Box.double"
    );
}

#[test]
fn a_function_whose_anchor_a_type_has_is_named_with_its_module() {
    let items = fixture(
        "// A hasher.\npub struct Sha256 { n: Int }\n\
         // The digest.\npub fn sha256(n: Int) -> Int\n\
         // Another.\npub fn other(n: Int) -> Int\n",
    );
    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["std.fixture", "Sha256", "fixture.sha256", "other"]);
    assert_ne!(
        named(&items, "Sha256").anchor(),
        named(&items, "fixture.sha256").anchor()
    );
    assert_eq!(named(&items, "fixture.sha256").unqualified_name(), "sha256");
}

#[test]
fn a_filter_without_type_arguments_lists_every_impls_method() {
    let items = json_items(&["Secret.rotl"]);
    let names: Vec<&str> = items.iter().map(|i| field(i, "name")).collect();
    assert_eq!(
        names,
        ["Secret<U8>.rotl", "Secret<U32>.rotl", "Secret<U64>.rotl"]
    );
    let one = json_items(&["Secret<U32>.rotl"]);
    assert_eq!(one.len(), 1);
    assert_eq!(
        field(&one[0], "url"),
        "https://oskalang.dev/api/prelude.html#secretu32rotl"
    );
}

#[test]
fn the_text_says_what_the_json_says() {
    let output = oskac(&["doc"]);
    assert_eq!(output.status.code(), Some(0));
    let human = text(&output.stdout);
    for item in json_items(&[]) {
        if field(&item, "kind") == "module" {
            assert!(human.contains(&format!("# {}\n", field(&item, "module"))));
            continue;
        }
        let heading = format!(
            "## {} {}\n\n    {}\n\nAuthority: {}.",
            field(&item, "kind"),
            field(&item, "name"),
            field(&item, "signature"),
            field(&item, "authority")
        );
        assert!(human.contains(&heading), "the text should have\n{heading}");
        assert!(human.contains(field(&item, "description")));
    }
}

#[test]
fn a_module_filter_lists_that_module() {
    let output = oskac(&["doc", "std.sql"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    assert_golden("doc_std_sql.txt", &text(&output.stdout));
    let output = oskac(&["doc", "std.sql", "--format=json"]);
    assert_golden("doc_std_sql.json", &text(&output.stdout));
}

#[test]
fn an_item_filter_lists_the_item_and_its_methods() {
    let names = |args: &[&str]| -> Vec<String> {
        json_items(args)
            .iter()
            .map(|i| field(i, "name").to_string())
            .collect()
    };
    assert_eq!(names(&["List.push"]), ["List.push"]);
    assert_eq!(names(&["std.sql.connect"]), ["connect"]);
    // A function renamed with its module (`sha2.sha256`) is still found by
    // its path.
    assert_eq!(names(&["std.sha2.sha256"]), ["sha2.sha256"]);
    let list = names(&["List"]);
    assert_eq!(list[0], "List");
    assert!(list.iter().skip(1).all(|n| n.starts_with("List.")));
    assert!(list.contains(&"List.map".to_string()));
}

#[test]
fn each_core_list_method_has_its_own_doc_entry() {
    // Spec #660, ticket #667: the methods whose bodies are in `std.core`
    // are still listed as methods of `List`, with a signature and a
    // description.
    let expected = [
        (
            "filter",
            "pub fn filter(self, keep: fn(T) -> Bool) -> List<T>",
        ),
        (
            "fold",
            "pub fn fold<A>(self, init: A, f: fn(A, T) -> A) -> A",
        ),
        ("any", "pub fn any(self, check: fn(T) -> Bool) -> Bool"),
        ("all", "pub fn all(self, check: fn(T) -> Bool) -> Bool"),
        (
            "find",
            "pub fn find(self, check: fn(T) -> Bool) -> Option<T>",
        ),
        ("contains", "pub fn contains(self, item: T) -> Bool"),
        ("first", "pub fn first(self) -> Option<T>"),
        ("last", "pub fn last(self) -> Option<T>"),
        ("is_empty", "pub fn is_empty(self) -> Bool"),
        ("reverse", "pub fn reverse(self) -> List<T>"),
        ("concat", "pub fn concat(self, other: List<T>) -> List<T>"),
    ];
    for (method, signature) in expected {
        let items = json_items(&[&format!("List.{method}")]);
        assert_eq!(items.len(), 1, "List.{method}");
        assert_eq!(field(&items[0], "signature"), signature);
        assert!(!field(&items[0], "description").is_empty());
        let output = oskac(&["doc", &format!("List.{method}")]);
        assert_eq!(output.status.code(), Some(0));
        assert!(text(&output.stdout).contains(signature));
    }
}

#[test]
fn an_unknown_filter_is_a_usage_error() {
    let output = oskac(&["doc", "List.no_such_method"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&output.stdout), "");
    assert!(text(&output.stderr).contains("`List.no_such_method`"));
    let output = oskac(&["doc", "nothing", "--format=json"]);
    assert_eq!(output.status.code(), Some(2));
    let json = parse_json(&text(&output.stdout)).expect("JSON");
    assert_eq!(
        json.get("error").and_then(JsonValue::as_str),
        Some("unknown item")
    );
    assert_eq!(
        json.get("item").and_then(JsonValue::as_str),
        Some("nothing")
    );
}

/// The rows of the "What works today" table: each Feature cell's names in
/// backticks, and whether the Interpreter column says it runs.
fn what_works_today() -> Vec<(Vec<String>, bool)> {
    let page = std::fs::read_to_string(repo_root().join("docs/guide/what-works-today.md"))
        .expect("the page should read");
    let mut rows = Vec::new();
    for line in page
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Feature") && !l.starts_with("| ---"))
    {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (feature, interpreter) = (cells[1], cells[3]);
        let names = feature
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
        rows.push((names, interpreter == "Yes"));
    }
    rows
}

#[test]
fn the_checks_only_marks_agree_with_what_works_today() {
    let items = json_items(&[]);
    let mut compared = 0;
    for (names, runs) in what_works_today() {
        for name in names {
            for item in &items {
                let module = field(item, "module");
                let matches = module == name
                    || (module == "prelude"
                        && field(item, "kind") == "capability"
                        && field(item, "name") == name);
                if matches {
                    compared += 1;
                    assert_eq!(
                        item.get("runs").and_then(JsonValue::as_bool),
                        Some(runs),
                        "`{}` of {module}: \"What works today\" says its row {} run",
                        field(item, "name"),
                        if runs { "does" } else { "does not" }
                    );
                }
            }
        }
    }
    assert!(
        compared > 20,
        "the table should name modules and capabilities the reference lists"
    );
    for module in ["std.sql", "std.proc"] {
        assert!(items
            .iter()
            .filter(|i| field(i, "module") == module)
            .all(|i| i.get("runs") == Some(&JsonValue::Bool(false))));
    }
}

#[test]
fn every_item_but_a_trait_impl_has_a_description() {
    let missing: Vec<String> = json_items(&[])
        .iter()
        .filter(|i| field(i, "kind") != "impl" && field(i, "description").is_empty())
        .map(|i| format!("{} {}", field(i, "module"), field(i, "name")))
        .collect();
    assert!(
        missing.is_empty(),
        "write a comment directly above each of these in its source: {missing:?}"
    );
}
