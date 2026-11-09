//! `parse_module` tests for `trait` and `impl` declarations, generic
//! parameters on type declarations, and where a `self` receiver may appear,
//! asserted through the JSON export.

mod common;

use common::{assert_items, module_err};
use oska_syntax::ast::ItemKind;
use oska_syntax::{parse_module, Span};

/// The JSON for an `Ident` node.
fn ident(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(r#"{{"kind": "Ident", "span": {{"start": {start}, "end": {end}}}, "name": "{name}"}}"#)
}

/// The JSON for a single-segment path type such as `Int` at `start`.
fn path_ty(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(
        r#"{{"kind": "PathType", "span": {{"start": {start}, "end": {end}}},
            "segments": [{segment}], "args": []}}"#,
        segment = ident(name, start),
    )
}

/// The JSON for an empty block `{}` at `start`.
fn empty_block(start: usize) -> String {
    format!(
        r#"{{"kind": "Block", "span": {{"start": {start}, "end": {}}}, "stmts": []}}"#,
        start + 2
    )
}

#[test]
fn inherent_impl_with_a_method_and_an_associated_function() {
    let src = "impl Counter {\n    pub fn new() {}\n    fn bump(var self) {}\n}";
    assert_items(
        src,
        &format!(
            r#"[{{
                "kind": "ImplItem", "span": {{"start": 0, "end": 61}},
                "public": false, "attributes": [],
                "generics": [],
                "target": {target},
                "methods": [
                    {{"kind": "FnItem", "span": {{"start": 19, "end": 34}},
                      "public": true, "attributes": [],
                      "name": {new},
                      "generics": [],
                      "params": [],
                      "requires": [], "ensures": [],
                      "body": {body1}}},
                    {{"kind": "FnItem", "span": {{"start": 39, "end": 59}},
                      "public": false, "attributes": [],
                      "name": {bump},
                      "generics": [],
                      "receiver": {{"kind": "SelfParam", "span": {{"start": 47, "end": 55}}, "mutable": true}},
                      "params": [],
                      "requires": [], "ensures": [],
                      "body": {body2}}}
                ]
            }}]"#,
            target = path_ty("Counter", 5),
            new = ident("new", 26),
            body1 = empty_block(32),
            bump = ident("bump", 42),
            body2 = empty_block(57),
        ),
    );
}

#[test]
fn generic_trait_impl() {
    assert_items(
        "impl<T: Eq> Eq for List<T> {}",
        &format!(
            r#"[{{
                "kind": "ImplItem", "span": {{"start": 0, "end": 29}},
                "public": false, "attributes": [],
                "generics": [
                    {{"kind": "GenericParam", "span": {{"start": 5, "end": 10}},
                      "name": {t}, "bounds": [{eq1}]}}
                ],
                "trait": {eq2},
                "target": {{"kind": "PathType", "span": {{"start": 19, "end": 26}},
                           "segments": [{list}], "args": [{t_arg}]}},
                "methods": []
            }}]"#,
            t = ident("T", 5),
            eq1 = path_ty("Eq", 8),
            eq2 = path_ty("Eq", 12),
            list = ident("List", 19),
            t_arg = path_ty("T", 24),
        ),
    );
}

#[test]
fn trait_with_a_signature_and_a_default_method() {
    // A method without a body omits `body`; a default method has one.
    let src = "trait Shape {\n    fn area(self) -> Int\n    fn describe(self) {}\n}";
    assert_items(
        src,
        &format!(
            r#"[{{
                "kind": "TraitItem", "span": {{"start": 0, "end": 65}},
                "public": false, "attributes": [],
                "name": {name},
                "generics": [],
                "methods": [
                    {{"kind": "FnItem", "span": {{"start": 18, "end": 38}},
                      "public": false, "attributes": [],
                      "name": {area},
                      "generics": [],
                      "receiver": {{"kind": "SelfParam", "span": {{"start": 26, "end": 30}}, "mutable": false}},
                      "params": [],
                      "return_type": {int},
                      "requires": [], "ensures": []}},
                    {{"kind": "FnItem", "span": {{"start": 43, "end": 63}},
                      "public": false, "attributes": [],
                      "name": {describe},
                      "generics": [],
                      "receiver": {{"kind": "SelfParam", "span": {{"start": 55, "end": 59}}, "mutable": false}},
                      "params": [],
                      "requires": [], "ensures": [],
                      "body": {body}}}
                ]
            }}]"#,
            name = ident("Shape", 6),
            area = ident("area", 21),
            int = path_ty("Int", 35),
            describe = ident("describe", 46),
            body = empty_block(61),
        ),
    );
}

#[test]
fn rejects_a_receiver_on_a_free_function() {
    // The error covers the receiver, including `var`, wherever it sits.
    let err = module_err("fn f(self) {}");
    assert_eq!(err.code, "E0150");
    assert_eq!(err.span, Span { start: 5, end: 9 });
    assert!(err.message.contains("`impl` or `trait`"), "{}", err.message);

    let err = module_err("fn f(var self, n: Int) {}");
    assert_eq!(err.code, "E0150");
    assert_eq!(err.span, Span { start: 5, end: 13 });

    let err = module_err("fn f(n: Int, self) {}");
    assert_eq!(err.code, "E0150");
    assert_eq!(err.span, Span { start: 13, end: 17 });
}

#[test]
fn syntax_sample_impl_block_parses() {
    // The `impl Account` block from the Syntax section of the design notes.
    let doc = include_str!("../../../docs/design.md");
    let section = &doc[doc.find("\n## Syntax\n").expect("the heading should exist")..];
    let start = section
        .find("```oska\n")
        .expect("the section should have a sample")
        + 8;
    let len = section[start..].find("```").expect("the sample should end");
    let module = parse_module(&section[start..start + len])
        .unwrap_or_else(|err| panic!("should parse: {err}"));
    let imp = module
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Impl(decl) => Some(decl),
            _ => None,
        })
        .expect("the sample should have an impl block");
    assert!(imp.trait_.is_none());
    let [method] = imp.methods.as_slice() else {
        panic!("expected one method, got {:?}", imp.methods);
    };
    assert!(method.public);
    let ItemKind::Fn(deposit) = &method.kind else {
        panic!("expected a method, got {method:?}");
    };
    assert_eq!(deposit.name.name, "deposit");
    assert!(deposit.receiver.as_ref().is_some_and(|r| r.mutable));
    assert_eq!(deposit.params.len(), 1);
    assert_eq!(deposit.requires.len(), 1);
    assert!(deposit.body.is_some());
}

/// The JSON for an unbounded `GenericParam` at `start`.
fn generic(name: &str, start: usize) -> String {
    let end = start + name.len();
    format!(
        r#"{{"kind": "GenericParam", "span": {{"start": {start}, "end": {end}}},
            "name": {name}, "bounds": []}}"#,
        name = ident(name, start),
    )
}

#[test]
fn generic_struct() {
    assert_items(
        "struct Pair<A, B> { a: A, b: B }",
        &format!(
            r#"[{{
                "kind": "StructItem", "span": {{"start": 0, "end": 32}},
                "public": false, "attributes": [],
                "cap": false,
                "name": {name},
                "generics": [{a_param}, {b_param}],
                "form": "named",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 20, "end": 24}},
                      "public": false, "name": {a}, "type": {a_ty}}},
                    {{"kind": "Field", "span": {{"start": 26, "end": 30}},
                      "public": false, "name": {b}, "type": {b_ty}}}
                ]
            }}]"#,
            name = ident("Pair", 7),
            a_param = generic("A", 12),
            b_param = generic("B", 15),
            a = ident("a", 20),
            a_ty = path_ty("A", 23),
            b = ident("b", 26),
            b_ty = path_ty("B", 29),
        ),
    );
}

#[test]
fn generic_enum() {
    assert_items(
        "enum Option<T> { Some(T), None }",
        &format!(
            r#"[{{
                "kind": "EnumItem", "span": {{"start": 0, "end": 32}},
                "public": false, "attributes": [],
                "cap": false,
                "name": {name},
                "generics": [{t_param}],
                "variants": [
                    {{"kind": "Variant", "span": {{"start": 17, "end": 24}},
                      "name": {some}, "form": "positional",
                      "fields": [
                          {{"kind": "Field", "span": {{"start": 22, "end": 23}},
                            "public": false, "type": {t}}}
                      ]}},
                    {{"kind": "Variant", "span": {{"start": 26, "end": 30}},
                      "name": {none}, "form": "unit", "fields": []}}
                ]
            }}]"#,
            name = ident("Option", 5),
            t_param = generic("T", 12),
            some = ident("Some", 17),
            t = path_ty("T", 22),
            none = ident("None", 26),
        ),
    );
}

#[test]
fn rejects_pub_on_an_impl_block() {
    // Visibility belongs on each method; the error covers `pub`.
    let err = module_err("pub impl Counter {}");
    assert_eq!(err.code, "E0151");
    assert_eq!(err.span, Span { start: 0, end: 3 });
}

#[test]
fn impl_methods_need_a_body() {
    // Only a trait method may stop after its signature; the error points
    // at the `}` where the body's `{` belongs.
    let err = module_err("impl T { fn f() }");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 16, end: 17 });
}

#[test]
fn rejects_non_method_items_in_a_trait() {
    let err = module_err("trait T {\n    struct S {}\n}");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 14, end: 20 });
}
