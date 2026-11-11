//! `pub(pkg)`: an item visible to every module of its package, but not
//! part of the package's API (spec #115). And parsing one module of a
//! text that joins several, with spans in the whole text.

use oska_syntax::ast::{ItemKind, Visibility};
use oska_syntax::{module_to_json, parse_module, parse_module_at};

fn visibilities(src: &str) -> Vec<Visibility> {
    let module = parse_module(src).unwrap_or_else(|err| panic!("should parse: {err}"));
    module.items.iter().map(|item| item.visibility).collect()
}

#[test]
fn an_item_is_private_pub_pkg_or_pub() {
    let src = "fn a() {}\npub(pkg) fn b() {}\npub fn c() {}\n\
               pub(pkg) struct S { n: Int }\npub(pkg) cap struct C { out: Stdout }\n\
               pub(pkg) enum E { A }\npub(pkg) trait T {}\npub(pkg) const N: Int = 1\n";
    assert_eq!(
        visibilities(src),
        [
            Visibility::Private,
            Visibility::Package,
            Visibility::Public,
            Visibility::Package,
            Visibility::Package,
            Visibility::Package,
            Visibility::Package,
            Visibility::Package,
        ]
    );
}

#[test]
fn a_method_may_be_pub_pkg() {
    let module = parse_module("struct S {}\nimpl S {\n    pub(pkg) fn f(self) {}\n}\n")
        .unwrap_or_else(|err| panic!("should parse: {err}"));
    let ItemKind::Impl(imp) = &module.items[1].kind else {
        panic!("expected an impl");
    };
    assert_eq!(imp.methods[0].visibility, Visibility::Package);
}

#[test]
fn pub_pkg_on_a_use_is_rejected_with_a_fix_that_removes_all_of_it() {
    let err = parse_module("pub(pkg) use std.sql\n").expect_err("should not parse");
    assert_eq!(err.code, "E0164");
    let fix = err.fix.expect("a fix");
    assert_eq!(fix.message, "remove `pub(pkg)`");
    assert_eq!((fix.edits[0].span.start, fix.edits[0].span.end), (0, 9));
}

#[test]
fn the_json_export_marks_a_pub_pkg_item_package_and_not_public() {
    let json = module_to_json(
        &parse_module("pub(pkg) fn f() {}\n").expect("parses"),
        "m.osk",
    );
    assert!(
        json.contains("\"public\": false,\n      \"package\": true,"),
        "{json}"
    );
    let plain = module_to_json(&parse_module("fn f() {}\n").expect("parses"), "m.osk");
    assert!(!plain.contains("\"package\""), "{plain}");
}

#[test]
fn a_module_parsed_at_an_offset_has_spans_in_the_whole_text() {
    let text = "fn a() {}\nfn b() {}\n";
    let module = parse_module_at(text, 10).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(module.items.len(), 1);
    let ItemKind::Fn(f) = &module.items[0].kind else {
        panic!("expected a function");
    };
    assert_eq!(&text[f.name.span.start..f.name.span.end], "b");
    assert_eq!((module.span.start, module.span.end), (10, text.len()));
}

#[test]
fn an_error_in_a_module_parsed_at_an_offset_spans_the_whole_text() {
    let text = "fn a() {}\nfn b( {}\n";
    let err = parse_module_at(text, 10).expect_err("should not parse");
    assert!(err.span.start >= 10, "{err:?}");
}
