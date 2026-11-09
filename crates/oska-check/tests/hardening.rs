//! Review fixes for the type checker (spec #32): capability-safety holes
//! found after the tickets merged, and the rules that close them.

mod common;

use common::{assert_errors, assert_fix, assert_ok};
use oska_check::{check_module_with, CheckOptions, FunctionAuthority, Via};

/// Each diagnostic's code and start offset, for snippets where the
/// spanned text alone is ambiguous.
fn codes_at(src: &str) -> Vec<(&'static str, usize)> {
    common::check(src)
        .diagnostics
        .iter()
        .map(|d| (d.code, d.span.start))
        .collect()
}

const RUN: &str = "fn run(f: fn()) {\n    f()\n}\n";

// ---- `cap fn` never flows into a plain `fn` ---------------------------------------

#[test]
fn question_mark_cannot_turn_a_cap_fn_error_into_a_plain_fn() {
    let src = "fn inner(g: cap fn()) -> Result<(), cap fn()> {\n    Err(g)\n}\n\
               fn outer(g: cap fn()) -> Result<(), fn()> {\n    inner(g)?\n    Ok(())\n}";
    assert_errors(src, &[("E0270", "inner(g)?")]);
}

#[test]
fn question_mark_may_widen_a_plain_fn_error_into_a_cap_fn() {
    assert_ok(
        "fn inner() -> Result<(), fn()> {\n    Ok(())\n}\n\
         fn outer() -> Result<(), cap fn()> {\n    inner()?\n    Ok(())\n}",
    );
}

#[test]
fn a_generic_identity_does_not_launder_a_cap_fn() {
    let src = format!(
        "{RUN}fn id<T>(x: T) -> T {{\n    x\n}}\n\
         fn f(g: cap fn()) {{\n    let h = id(g)\n    run(h)\n}}"
    );
    assert_errors(&src, &[("E0270", "h")]);
}

#[test]
fn a_generic_call_expected_to_return_a_plain_fn_rejects_a_cap_fn_argument() {
    let src = format!(
        "{RUN}fn id<T>(x: T) -> T {{\n    x\n}}\n\
         fn f(g: cap fn()) {{\n    run(id(g))\n}}"
    );
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn an_option_of_a_cap_fn_is_not_an_option_of_a_plain_fn() {
    let src = "fn f(g: cap fn()) -> Option<fn()> {\n    let o = Some(g)\n    o\n}";
    assert_errors(src, &[("E0270", "o")]);
}

#[test]
fn a_list_of_cap_fns_is_not_a_list_of_plain_fns() {
    let src = "fn f(g: cap fn()) -> List<fn()> {\n    [g]\n}";
    assert_errors(src, &[("E0270", "g")]);
}

#[test]
fn a_list_typed_for_plain_fns_cannot_receive_a_cap_fn() {
    let src = "fn f(g: cap fn()) -> List<fn()> {\n    var xs = List.new()\n    let ys: List<fn()> = xs\n    xs.push(g)\n    ys\n}";
    assert_errors(src, &[("E0270", "g")]);
}

#[test]
fn a_cap_fn_cannot_be_returned_as_a_plain_fn() {
    let src = "fn f(g: cap fn()) -> fn() {\n    return g\n}";
    assert_errors(src, &[("E0270", "g")]);
}

#[test]
fn a_closure_parameter_cannot_narrow_a_cap_fn_argument_to_a_plain_fn() {
    let src = format!(
        "{RUN}fn give(f: fn(cap fn()), c: cap fn()) {{\n    f(c)\n}}\n\
         fn launder(c: cap fn()) {{\n    give(|g: fn()| run(g), c)\n}}"
    );
    assert_errors(&src, &[("E0270", "fn()")]);
}

#[test]
fn a_plain_fn_can_be_used_where_a_cap_fn_is_expected() {
    assert_ok(
        "fn run(f: cap fn()) {\n    f()\n}\n\
         fn f(g: fn()) -> Option<cap fn()> {\n    run(g)\n    Some(g)\n}",
    );
}

// ---- Purity in contracts, through generics --------------------------------------

#[test]
fn a_generic_call_whose_argument_is_a_capability_is_impure_in_a_contract() {
    let src = "fn log(out: Stdout) {\n    let _ = out.println(\"x\")\n}\n\
               fn apply<T>(f: fn(T), x: T) {\n    f(x)\n}\n\
               fn f(out: Stdout) -> Int\n    requires apply(log, out) == ()\n{\n    1\n}";
    assert_errors(src, &[("E0280", "apply(log, out)")]);
}

#[test]
fn a_generic_method_on_a_capability_bearing_receiver_is_impure_in_a_contract() {
    let src = "struct Box<T> { value: T }\n\
               impl<T> Box<T> {\n    fn size(self) -> Int {\n        1\n    }\n}\n\
               fn f(b: Box<Stdout>) -> Int\n    requires b.size() == 1\n{\n    1\n}";
    assert_errors(src, &[("E0280", "b.size()")]);
}

#[test]
fn a_generic_call_on_pure_values_is_allowed_in_a_contract() {
    assert_ok(
        "fn id<T>(x: T) -> T {\n    x\n}\n\
         fn f(n: Int) -> Int\n    requires id(n) == n\n{\n    n\n}",
    );
}

// ---- Duplicates -------------------------------------------------------------------

#[test]
fn a_function_declared_twice_is_reported_at_the_second() {
    let src = "fn f() -> Int {\n    1\n}\nfn f() -> String {\n    \"a\"\n}";
    assert_eq!(
        codes_at(src),
        vec![("E0202", src.rfind("f()").expect("second"))]
    );
}

#[test]
fn a_function_cannot_redeclare_a_prelude_function() {
    let src = "fn todo() -> Int {\n    1\n}";
    assert_errors(src, &[("E0202", "todo")]);
}

#[test]
fn an_inherent_method_declared_twice_across_impls_is_reported_at_the_second() {
    let src = "struct Foo { x: Int }\n\
               impl Foo {\n    fn a(self) -> Int {\n        1\n    }\n}\n\
               impl Foo {\n    fn a(self) -> String {\n        \"s\"\n    }\n}";
    let second = src.rfind("fn a").expect("second method") + "fn ".len();
    assert_eq!(codes_at(src), vec![("E0202", second)]);
}

#[test]
fn an_impl_of_a_trait_for_a_type_twice_is_reported_at_the_second() {
    let src = "struct Foo { x: Int }\n\
               impl Display for Foo {\n    fn to_string(self) -> String {\n        \"a\"\n    }\n}\n\
               impl Display for Foo {\n    fn to_string(self) -> String {\n        \"b\"\n    }\n}";
    let second = src.rfind("impl Display").expect("second impl") + "impl ".len();
    assert_eq!(codes_at(src), vec![("E0290", second)]);
}

#[test]
fn an_impl_that_overlaps_a_prelude_impl_conflicts() {
    assert_errors(
        "impl Eq for Int {\n    fn eq(self, other: Int) -> Bool {\n        true\n    }\n}",
        &[("E0290", "Eq")],
    );
}

#[test]
fn impls_of_one_trait_for_different_types_do_not_conflict() {
    assert_ok(
        "struct A { x: Int }\nstruct B { x: Int }\n\
         impl Display for A {\n    fn to_string(self) -> String {\n        \"a\"\n    }\n}\n\
         impl Display for B {\n    fn to_string(self) -> String {\n        \"b\"\n    }\n}",
    );
}

#[test]
fn a_generic_impl_conflicts_with_an_impl_for_one_of_its_instances() {
    let src = "struct Box<T> { value: T }\n\
               impl<T> Display for Box<T> {\n    fn to_string(self) -> String {\n        \"a\"\n    }\n}\n\
               impl Display for Box<Int> {\n    fn to_string(self) -> String {\n        \"b\"\n    }\n}";
    let second = src.rfind("impl Display").expect("second impl") + "impl ".len();
    assert_eq!(codes_at(src), vec![("E0290", second)]);
}

// ---- Orphan rule for inherent methods ---------------------------------------------

#[test]
fn user_code_cannot_add_inherent_methods_to_a_prelude_type() {
    let src = "impl FsRead {\n    fn size(self) -> Int {\n        1\n    }\n}";
    assert_errors(src, &[("E0291", "FsRead")]);
}

#[test]
fn user_code_cannot_add_inherent_methods_to_a_generic_prelude_type() {
    let src = "impl<T> Option<T> {\n    fn size(self) -> Int {\n        1\n    }\n}";
    assert_errors(src, &[("E0291", "Option<T>")]);
}

// ---- The `scope` handle cannot escape ----------------------------------------------

#[test]
fn a_function_cannot_return_a_scope() {
    let src = "fn leak() -> Scope {\n    scope |s| { s }\n}";
    assert_errors(src, &[("E0292", "Scope"), ("E0292", "scope |s| { s }")]);
}

#[test]
fn a_function_cannot_return_a_scope_inside_a_wrapper() {
    assert_errors(
        "fn leak(s: Scope) -> Option<Scope> {\n    Some(s)\n}",
        &[("E0292", "Option<Scope>")],
    );
}

#[test]
fn a_struct_or_enum_field_cannot_hold_a_scope() {
    assert_errors(
        "struct Keep { s: Scope }\nenum Held { One(List<Scope>), Zero }",
        &[("E0292", "Scope"), ("E0292", "List<Scope>")],
    );
}

#[test]
fn a_scope_block_cannot_produce_its_handle() {
    assert_errors(
        "fn leak() -> Int {\n    let s = scope |s| { s }\n    1\n}",
        &[("E0292", "scope |s| { s }")],
    );
}

#[test]
fn a_scope_block_cannot_produce_its_handle_inside_a_wrapper() {
    assert_errors(
        "fn leak() -> Int {\n    let s = scope |s| { [s] }\n    1\n}",
        &[("E0292", "scope |s| { [s] }")],
    );
}

#[test]
fn a_scope_can_be_passed_as_a_parameter() {
    assert_ok(
        "fn work(s: Scope) -> Int {\n    s.spawn(|| 1).join()\n}\n\
         fn main() {\n    let _ = scope |s| { work(s) }\n}",
    );
}

// ---- `todo()` under `--release` ---------------------------------------------------

/// `(code, spanned text)` for each diagnostic of `src` checked with
/// `--release`.
fn release_errors(src: &str) -> Vec<(&'static str, &str)> {
    check_module_with(src, CheckOptions { release: true })
        .diagnostics
        .iter()
        .map(|d| (d.code, &src[d.span.start..d.span.end]))
        .collect()
}

#[test]
fn todo_is_allowed_outside_release() {
    assert_ok("fn ship() -> Int {\n    todo()\n}");
}

#[test]
fn todo_is_an_error_under_release() {
    assert_eq!(
        release_errors("fn ship() -> Int {\n    todo()\n}"),
        [("E0293", "todo()")]
    );
}

// ---- `use` ------------------------------------------------------------------------

#[test]
fn a_use_outside_std_is_an_error() {
    assert_errors("use foo.bar", &[("E0294", "foo.bar")]);
}

#[test]
fn a_use_of_a_std_module_that_does_not_exist_is_an_error() {
    // No `std.*` module exists yet, so every `use` of one is unknown.
    assert_errors("use std.sql", &[("E0295", "std.sql")]);
}

#[test]
fn a_use_with_a_brace_group_names_its_module() {
    assert_errors("use std.sql.{Query}", &[("E0295", "std.sql")]);
}

// ---- JSON ---------------------------------------------------------------------------

#[test]
fn the_authority_report_escapes_its_strings() {
    let function = FunctionAuthority {
        name: "a\"b\\c".to_string(),
        authority: vec!["Std\"out".to_string()],
        via: vec![Via {
            param: "o\nut".to_string(),
            capabilities: vec!["Std\"out".to_string()],
        }],
        pure: false,
    };
    assert_eq!(
        function.to_json(),
        r#"{"name":"a\"b\\c","authority":["Std\"out"],"via":[{"param":"o\nut","capabilities":["Std\"out"]}],"pure":false}"#
    );
}

// ---- Fixes --------------------------------------------------------------------------

#[test]
fn a_missing_trait_method_fix_inserts_a_todo_stub() {
    assert_fix(
        "struct Foo { x: Int }\nimpl Display for Foo {\n}",
        "E0261",
        "struct Foo { x: Int }\nimpl Display for Foo {\n    fn to_string(self) -> String {\n        todo()\n    }\n}",
    );
}

#[test]
fn a_missing_trait_method_fix_works_on_a_one_line_impl() {
    assert_fix(
        "struct Foo { x: Int }\nimpl Display for Foo {}",
        "E0261",
        "struct Foo { x: Int }\nimpl Display for Foo {\n    fn to_string(self) -> String {\n        todo()\n    }\n}",
    );
}

#[test]
fn a_missing_trait_method_fix_goes_after_the_existing_methods() {
    assert_fix(
        "trait Shape {\n    fn area(self) -> Int\n    fn name(self, short: Bool) -> String\n}\n\
         struct Sq { side: Int }\n\
         impl Shape for Sq {\n    fn area(self) -> Int {\n        self.side * self.side\n    }\n}",
        "E0261",
        "trait Shape {\n    fn area(self) -> Int\n    fn name(self, short: Bool) -> String\n}\n\
         struct Sq { side: Int }\n\
         impl Shape for Sq {\n    fn area(self) -> Int {\n        self.side * self.side\n    }\n    \
         fn name(self, short: Bool) -> String {\n        todo()\n    }\n}",
    );
}

#[test]
fn a_dbg_in_release_fix_keeps_only_its_argument() {
    let src = "fn twice(n: Int) -> Int {\n    dbg(n * 2)\n}";
    let report = check_module_with(src, CheckOptions { release: true });
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code == "E0216")
        .expect("E0216");
    let fix = diagnostic.fix.as_ref().expect("E0216 should carry a fix");
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.to_string();
    for edit in edits {
        fixed.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    assert_eq!(fixed, "fn twice(n: Int) -> Int {\n    n * 2\n}");
}
