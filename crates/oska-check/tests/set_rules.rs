// SPDX-License-Identifier: Apache-2.0
//! The checker's rules for `Set` (#802, spec #798, ADR 0032): `Set` is a
//! prelude name (`E0202` for a user type of that name), an item type needs
//! `Eq` and `Hash` (`E0266`), a `Secret` item is refused with its own codes
//! (`E0351` for `Eq`, `E0354` for `Hash`), and `add` and `remove` need a
//! `var` (`E0230`). A `Float` item is refused once `Float` exists (#592).

mod common;

use common::{assert_errors, assert_modules_errors, assert_modules_ok, assert_ok, check};

/// A program that makes a set of `item`, with `body` after the binding
/// `var s`.
fn program(item: &str, decls: &str, body: &str) -> String {
    format!("{decls}\nfn main() {{\n    var s: Set<{item}> = Set.new()\n{body}\n}}")
}

fn message(src: &str) -> String {
    check(src).diagnostics[0].message.clone()
}

#[test]
fn set_checks_with_no_import() {
    assert_ok(&program(
        "String",
        "",
        "    let _a = s.add(\"a\")\n    let n = s.len()",
    ));
}

#[test]
fn set_checks_in_a_package_module_with_no_import() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util\n\nfn main() {\n    let n = util.count(Set.new())\n}\n",
        ),
        (
            "util",
            "pub fn count(s: Set<Int>) -> Int {\n    s.len()\n}\n",
        ),
    ]);
}

#[test]
fn a_user_struct_named_set_is_refused_as_a_prelude_name() {
    assert_errors(
        "struct Set { n: Int }\n\nfn main() {\n    let s = Set(n: 1)\n}\n",
        &[("E0202", "Set")],
    );
}

#[test]
fn a_package_module_cannot_declare_set_either() {
    assert_modules_errors(
        &[
            ("main", "fn main() {}\n"),
            ("util", "pub enum Set {\n    A,\n}\n"),
        ],
        &[("src/util.osk", "E0202", "Set")],
    );
}

#[test]
fn a_user_function_named_set_is_fine() {
    assert_ok("fn set(n: Int) -> Int {\n    n\n}\n\nfn main() {\n    let n = set(1)\n}\n");
}

#[test]
fn every_core_item_type_passes() {
    for item in [
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
    ] {
        assert_ok(&program(item, "", "    let n = s.len()"));
    }
}

#[test]
fn an_item_type_without_eq_is_refused_with_e0266() {
    let decls = "struct P { x: Int }\n";
    let src = program("P", decls, "    let _a = s.add(P(x: 1))");
    assert_errors(&src, &[("E0266", "Set.new()"), ("E0266", "s.add(P(x: 1))")]);
    let text = message(&src);
    assert!(text.contains("`P` cannot be a `Set` item"), "{text}");
    assert!(text.contains("`Eq`"), "{text}");
    assert!(text.contains("impl Eq"), "{text}");
}

#[test]
fn an_item_that_holds_a_non_key_part_is_refused() {
    for item in ["Result<Int, String>", "List<Result<Int, Int>>"] {
        let src = program(item, "", "");
        assert!(
            check(&src).diagnostics.iter().any(|d| d.code == "E0266"),
            "{item}: {:#?}",
            check(&src).diagnostics
        );
    }
}

#[test]
fn a_secret_item_is_refused_with_the_secret_codes() {
    for item in ["Secret<Bytes>", "Secret<U32>", "Secret<Bool>"] {
        let src = program(item, "", "");
        let codes: Vec<&str> = check(&src).diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, ["E0351", "E0354"], "{item}");
    }
}

#[test]
fn a_struct_that_holds_a_secret_is_refused_as_an_item() {
    let decls = "struct K { k: Secret<Bytes> }\n\
                 impl Eq for K {\n    fn eq(self, other: Self) -> Bool { true }\n}\n";
    let src = program("K", decls, "");
    let codes: Vec<&str> = check(&src).diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0354"]);
}

#[test]
fn add_and_remove_need_a_var() {
    let src = "fn main() {\n    let s: Set<Int> = Set.new()\n    let _a = s.add(2)\n}";
    assert_errors(src, &[("E0230", "s")]);
    let src = "fn main() {\n    let s: Set<Int> = Set.new()\n    let _gone = s.remove(1)\n}";
    assert_errors(src, &[("E0230", "s")]);
    let src = "fn main() {\n    var s: Set<Int> = Set.new()\n    let _a = s.add(2)\n    let _gone = s.remove(1)\n}";
    assert_ok(src);
}

#[test]
fn a_generic_function_over_an_item_type_is_told_the_bounds() {
    let src = "fn count<T>(item: T) -> Int {\n    var s: Set<T> = Set.new()\n    let _a = s.add(item)\n    s.len()\n}\nfn main() {}";
    let report = check(src);
    assert!(!report.diagnostics.is_empty());
    assert!(
        report.diagnostics.iter().all(|d| d.code == "E0265"),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn the_set_operations_check_for_an_item_type_with_eq_and_hash() {
    assert_ok(&program(
        "String",
        "",
        "    let u = s.union(s)\n    let i = s.intersection(u)\n    let d = s.difference(i)\n    let n = d.len()",
    ));
}

#[test]
fn the_set_operations_refuse_a_secret_item_as_the_core_does() {
    let body = "    let u = s.union(s)\n    let i = s.intersection(s)\n    let d = s.difference(s)";
    let src = program("Secret<Bytes>", "", body);
    let codes: Vec<&str> = check(&src).diagnostics.iter().map(|d| d.code).collect();
    assert!(
        codes.iter().all(|c| ["E0351", "E0354"].contains(c)),
        "{codes:?}"
    );
    assert!(
        codes.contains(&"E0351") && codes.contains(&"E0354"),
        "{codes:?}"
    );
}

#[test]
fn the_set_operations_refuse_a_float_item_with_e0266() {
    let body = "    let u = s.union(s)\n    let i = s.intersection(s)\n    let d = s.difference(s)";
    let src = program("Float", "", body);
    let diagnostics = check(&src).diagnostics;
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics.iter().all(|d| d.code == "E0266"),
        "{diagnostics:#?}"
    );
}
