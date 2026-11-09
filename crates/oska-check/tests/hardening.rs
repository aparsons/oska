//! Review fixes for the type checker (spec #32): capability-safety holes
//! found after the tickets merged, and the rules that close them.

mod common;

use common::{assert_errors, assert_ok};

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
