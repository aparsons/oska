//! Prelude mode: `parse_prelude` accepts body-less `fn` items and the
//! `#[capability]` attribute, which only the embedded prelude may use.
//! `parse_module` keeps rejecting both with its existing errors.

mod common;

use common::{compact, ident, module_err, path_ty, FILE};
use oska_syntax::ast::ItemKind;
use oska_syntax::{module_to_json, parse_prelude, Span};

/// Assert that `src`, parsed in prelude mode, exports exactly the envelope
/// whose `items` array is the inline JSON `items`.
fn assert_prelude_items(src: &str, items: &str) {
    let module = match parse_prelude(src) {
        Ok(module) => module,
        Err(err) => panic!("{src:?} should parse in prelude mode: {err}"),
    };
    let expected = compact(&format!(
        r#"{{"ast_version": 1, "file": "{FILE}", "items": {items}}}"#
    ));
    assert_eq!(
        compact(&module_to_json(&module, FILE)),
        expected,
        "export of {src:?}"
    );
}

#[test]
fn prelude_accepts_a_free_function_without_a_body() {
    // A body-less function omits `body` from its export.
    assert_prelude_items(
        "fn args() -> Args",
        &format!(
            r#"[{{
                "kind": "FnItem", "span": {{"start": 0, "end": 17}},
                "public": false, "attributes": [],
                "name": {name},
                "generics": [],
                "params": [],
                "return_type": {args},
                "requires": [], "ensures": []
            }}]"#,
            name = ident("args", 3),
            args = path_ty("Args", 13),
        ),
    );
}

#[test]
fn prelude_accepts_a_capability_struct() {
    // A built-in capability is an opaque struct whose fields are private.
    assert_prelude_items(
        "#[capability]\npub struct Stdout { handle: Int }",
        &format!(
            r#"[{{
                "kind": "StructItem", "span": {{"start": 0, "end": 47}},
                "public": true,
                "attributes": [
                    {{"kind": "Attribute", "span": {{"start": 0, "end": 13}},
                      "name": {capability}, "args": []}}
                ],
                "cap": false,
                "name": {name},
                "generics": [],
                "form": "named",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 34, "end": 45}},
                      "public": false, "name": {handle}, "type": {int}}}
                ]
            }}]"#,
            capability = ident("capability", 2),
            name = ident("Stdout", 25),
            handle = ident("handle", 34),
            int = path_ty("Int", 42),
        ),
    );
}

#[test]
fn prelude_accepts_a_method_without_a_body() {
    let src = "impl Stdout {\n    pub fn print(self, s: String)\n}";
    let module = parse_prelude(src).expect("a prelude method may omit its body");
    let ItemKind::Impl(decl) = &module.items[0].kind else {
        panic!("expected an impl, got {:?}", module.items[0].kind);
    };
    let ItemKind::Fn(method) = &decl.methods[0].kind else {
        panic!("expected a method, got {:?}", decl.methods[0].kind);
    };
    assert!(method.body.is_none(), "{method:?}");
}

#[test]
fn prelude_rejects_the_capability_attribute_on_a_function() {
    // Only a struct can be a built-in capability. The error covers the
    // attribute.
    let err = parse_prelude("#[capability]\nfn args() -> Args").expect_err("should fail");
    assert_eq!(err.code, "E0170");
    assert_eq!(err.span, Span { start: 0, end: 13 });
}

#[test]
fn prelude_rejects_the_capability_attribute_on_a_method() {
    let src = "impl Stdout {\n    #[capability]\n    fn print(self, s: String)\n}";
    let err = parse_prelude(src).expect_err("should fail");
    assert_eq!(err.code, "E0170");
    assert_eq!(err.span, Span { start: 18, end: 31 });
}

#[test]
fn prelude_rejects_the_capability_attribute_on_an_enum() {
    let err = parse_prelude("#[capability]\nenum Fs { Root }").expect_err("should fail");
    assert_eq!(err.code, "E0170");
    assert_eq!(err.span, Span { start: 0, end: 13 });
}

#[test]
fn user_code_still_needs_a_function_body() {
    // Without a body, the parser expects the `{` that starts one. At end
    // of input the error is an empty span there.
    let err = module_err("fn args() -> Args");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 17, end: 17 });
}

#[test]
fn user_code_still_needs_a_method_body() {
    // Inside an `impl`, the error points at the line break after the
    // signature.
    let err = module_err("impl Stdout {\n    fn print(self, s: String)\n}");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 43, end: 44 });
}

#[test]
fn user_code_rejects_the_capability_attribute() {
    // `#[capability]` is an unknown attribute outside the prelude. The
    // error covers the attribute's name.
    let err = module_err("#[capability]\nstruct Stdout { handle: Int }");
    assert_eq!(err.code, "E0132");
    assert_eq!(err.span, Span { start: 2, end: 12 });
    assert!(
        err.message.contains("only `#[integration]`"),
        "{}",
        err.message
    );
}

#[test]
fn prelude_signature_ending_in_a_generic_type_ends_before_a_pub_item() {
    // A line ending in `>` emits no line break, but a body-less signature
    // still ends there when the next line starts another item.
    assert_prelude_items(
        "fn a() -> Option<Int>\npub fn b() -> Int",
        &format!(
            r#"[{{
                "kind": "FnItem", "span": {{"start": 0, "end": 21}},
                "public": false, "attributes": [],
                "name": {a},
                "generics": [],
                "params": [],
                "return_type": {{"kind": "PathType", "span": {{"start": 10, "end": 21}},
                                "segments": [{option}], "args": [{int1}]}},
                "requires": [], "ensures": []
            }}, {{
                "kind": "FnItem", "span": {{"start": 22, "end": 39}},
                "public": true, "attributes": [],
                "name": {b},
                "generics": [],
                "params": [],
                "return_type": {int2},
                "requires": [], "ensures": []
            }}]"#,
            a = ident("a", 3),
            option = ident("Option", 10),
            int1 = path_ty("Int", 17),
            b = ident("b", 29),
            int2 = path_ty("Int", 36),
        ),
    );
}

#[test]
fn prelude_signature_ending_in_a_generic_type_ends_before_an_attribute() {
    assert_prelude_items(
        "fn a() -> List<Int>\n#[capability]\nstruct S { x: Int }",
        &format!(
            r#"[{{
                "kind": "FnItem", "span": {{"start": 0, "end": 19}},
                "public": false, "attributes": [],
                "name": {a},
                "generics": [],
                "params": [],
                "return_type": {{"kind": "PathType", "span": {{"start": 10, "end": 19}},
                                "segments": [{list}], "args": [{int1}]}},
                "requires": [], "ensures": []
            }}, {{
                "kind": "StructItem", "span": {{"start": 20, "end": 53}},
                "public": false,
                "attributes": [
                    {{"kind": "Attribute", "span": {{"start": 20, "end": 33}},
                      "name": {capability}, "args": []}}
                ],
                "cap": false,
                "name": {s},
                "generics": [],
                "form": "named",
                "fields": [
                    {{"kind": "Field", "span": {{"start": 45, "end": 51}},
                      "public": false, "name": {x}, "type": {int2}}}
                ]
            }}]"#,
            a = ident("a", 3),
            list = ident("List", 10),
            int1 = path_ty("Int", 15),
            capability = ident("capability", 22),
            s = ident("S", 41),
            x = ident("x", 45),
            int2 = path_ty("Int", 48),
        ),
    );
}

#[test]
fn prelude_method_ending_in_a_generic_type_ends_before_the_next_method() {
    // The same holds for body-less `impl` methods, including the last one
    // before the closing `}`.
    let module = parse_prelude(
        "impl Box {\n    fn get(self) -> Option<Int>\n    fn all(self) -> List<Int>\n}",
    )
    .expect("the impl should parse in prelude mode");
    let ItemKind::Impl(decl) = &module.items[0].kind else {
        panic!("expected an impl");
    };
    let spans: Vec<_> = decl.methods.iter().map(|m| m.span).collect();
    assert_eq!(
        spans,
        [Span { start: 15, end: 42 }, Span { start: 47, end: 72 }]
    );
}

#[test]
fn a_signature_still_needs_a_line_break_before_the_next_item() {
    // Two signatures on one line are still an error.
    let err = parse_prelude("fn a() -> Option<Int> fn b() -> Int")
        .expect_err("two items on one line should not parse");
    assert_eq!(err.code, "E0101");
    assert_eq!(err.span, Span { start: 22, end: 24 });
}
