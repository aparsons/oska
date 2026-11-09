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
