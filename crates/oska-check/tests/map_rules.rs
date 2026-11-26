// SPDX-License-Identifier: Apache-2.0
//! The checker's rules for `Map` (#602, spec #591, ADR 0028): a key type
//! needs `Eq` and `Hash` (`E0266`), a `Secret` key is refused with its own
//! codes (`E0351` for `Eq`, `E0354` for `Hash`), and `put` and `remove`
//! need a `var` (`E0230`). A `Float` key is refused once `Float` exists
//! (#592); until then nothing here can name one.

mod common;

use common::{assert_errors, assert_ok, check};

/// A program that makes a map of `key` to `Int`, with `body` after the
/// binding `var m`.
fn program(key: &str, decls: &str, body: &str) -> String {
    format!("{decls}\nfn main() {{\n    var m: Map<{key}, Int> = Map.new()\n{body}\n}}")
}

/// The first diagnostic's message.
fn message(src: &str) -> String {
    check(src).diagnostics[0].message.clone()
}

#[test]
fn every_core_key_type_passes() {
    for key in [
        "Int",
        "I8",
        "U8",
        "U64",
        "Bool",
        "String",
        "Untrusted<String>",
        "Path",
        "Bytes",
        "Option<Int>",
        "List<String>",
        "Option<List<Path>>",
    ] {
        assert_ok(&program(key, "", "    let n = m.len()"));
    }
}

#[test]
fn a_struct_with_an_eq_impl_and_hashable_fields_is_a_key() {
    let decls = "struct P { x: Int, name: String }\n\
                 impl Eq for P {\n    fn eq(self, other: Self) -> Bool { self.x == other.x && self.name == other.name }\n}\n";
    assert_ok(&program("P", decls, "    m.put(P(x: 1, name: \"a\"), 1)"));
}

#[test]
fn a_key_type_without_eq_is_refused() {
    let decls = "struct P { x: Int }\n";
    let src = program("P", decls, "    m.put(P(x: 1), 1)");
    assert_errors(
        &src,
        &[("E0266", "Map.new()"), ("E0266", "m.put(P(x: 1), 1)")],
    );
    let text = message(&src);
    assert!(text.contains("`P` cannot be a `Map` key"), "{text}");
    assert!(text.contains("`Eq`"), "{text}");
    assert!(
        text.contains("impl Eq"),
        "the message says how to fix it: {text}"
    );
}

#[test]
fn a_key_type_without_hash_is_refused() {
    let decls = "struct R { r: Result<Int, String> }\n\
                 impl Eq for R {\n    fn eq(self, other: Self) -> Bool { true }\n}\n";
    let src = program("R", decls, "");
    assert_errors(&src, &[("E0266", "Map.new()")]);
    assert!(message(&src).contains("`Hash`"));
}

#[test]
fn a_key_that_holds_a_non_key_part_is_refused() {
    for key in [
        "Result<Int, String>",
        "List<Result<Int, Int>>",
        "Option<Scope>",
    ] {
        let src = program(key, "", "");
        assert!(
            check(&src).diagnostics.iter().any(|d| d.code == "E0266"),
            "{key}: {:#?}",
            check(&src).diagnostics
        );
    }
}

#[test]
fn a_secret_key_is_refused_with_the_secret_codes() {
    for key in ["Secret<Bytes>", "Secret<U32>", "Secret<Bool>"] {
        let src = program(key, "", "");
        let codes: Vec<&str> = check(&src).diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, ["E0351", "E0354"], "{key}");
    }
}

#[test]
fn a_struct_that_holds_a_secret_is_refused_as_a_key() {
    let decls = "struct K { k: Secret<Bytes> }\n\
                 impl Eq for K {\n    fn eq(self, other: Self) -> Bool { true }\n}\n";
    let src = program("K", decls, "");
    let codes: Vec<&str> = check(&src).diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0354"]);
}

#[test]
fn put_and_remove_need_a_var() {
    let src = "fn main() {\n    let m: Map<Int, Int> = Map.new()\n    m.put(1, 2)\n}";
    assert_errors(src, &[("E0230", "m")]);
    let src = "fn main() {\n    let m: Map<Int, Int> = Map.new()\n    let gone = m.remove(1)\n}";
    assert_errors(src, &[("E0230", "m")]);
    let src = "fn main() {\n    var m: Map<Int, Int> = Map.new()\n    m.put(1, 2)\n    let gone = m.remove(1)\n}";
    assert_ok(src);
}

#[test]
fn a_generic_function_over_a_key_type_is_told_the_bounds() {
    let src = "fn count<K>(k: K) -> Int {\n    var m: Map<K, Int> = Map.new()\n    m.put(k, 1)\n    m.len()\n}\nfn main() {}";
    let report = check(src);
    assert!(
        report.diagnostics.iter().all(|d| d.code == "E0265"),
        "a type parameter keeps E0265, whose fix adds the bound: {:#?}",
        report.diagnostics
    );
    assert!(!report.diagnostics.is_empty());
}

#[test]
fn a_type_that_holds_a_type_parameter_keeps_e0265() {
    let src = "fn count<K>(k: List<K>) -> Int {\n    var m: Map<List<K>, Int> = Map.new()\n    m.put(k, 1)\n    m.len()\n}\nfn main() {}";
    let report = check(src);
    assert!(!report.diagnostics.is_empty());
    assert!(
        report.diagnostics.iter().all(|d| d.code == "E0265"),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn a_map_type_that_is_only_written_is_accepted() {
    assert_ok("struct Point { x: Int }\nfn f(m: Map<Point, Int>) {}\nfn main() {}");
}
