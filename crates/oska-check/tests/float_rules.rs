// SPDX-License-Identifier: Apache-2.0
//! The checker's rules for `Float` (spec #590, ticket #597): no implicit
//! `Int` to `Float` conversion (`E0237`), a literal too large for `Float`
//! (`E0222`, the code of every literal out of range), `Secret<Float>`
//! refused (`E0357`), and a secret that cannot reach text.
//!
//! The secret rules are security rules: float arithmetic is not
//! constant-time on common hardware (subnormals, ADR 0027), so a program
//! that computes on a secret `Float` or prints a secret must not check.

mod common;

use common::{assert_errors, assert_ok, check};

/// Assert that `src` reports exactly one diagnostic, `code` at `spanned`,
/// whose message contains `fragment`.
fn assert_refused(src: &str, code: &str, spanned: &str, fragment: &str) {
    assert_errors(src, &[(code, spanned)]);
    let message = &check(src).diagnostics[0].message;
    assert!(
        message.contains(fragment),
        "the message of {code} should contain {fragment:?}: {message}"
    );
}

// ---- Allowed forms ------------------------------------------------------------

#[test]
fn floats_with_floats_check() {
    assert_ok(
        "fn f(a: Float, b: Float) -> Float {\n\
         \x20   let c: Float = 1.5\n\
         \x20   (a + b) * c - 2.0 / a\n\
         }",
    );
}

// ---- E0237: no implicit Int to Float ------------------------------------------

#[test]
fn an_int_returned_as_a_float_is_refused() {
    assert_refused(
        "fn f(n: Int) -> Float {\n    n\n}",
        "E0237",
        "n",
        "no implicit conversion",
    );
}

#[test]
fn an_int_literal_where_a_float_is_wanted_is_refused() {
    assert_refused(
        "fn f() -> Float {\n    let x: Float = 1\n    x\n}",
        "E0237",
        "1",
        "write the literal with a decimal point",
    );
}

#[test]
fn an_int_argument_for_a_float_parameter_is_refused() {
    assert_errors(
        "fn g(x: Float) -> Float {\n    x\n}\nfn f(n: Int) -> Float {\n    g(n)\n}",
        &[("E0237", "n")],
    );
}

#[test]
fn a_sized_integer_is_not_a_float_either() {
    assert_errors("fn f(n: U8) -> Float {\n    n\n}", &[("E0237", "n")]);
}

#[test]
fn mixed_arithmetic_is_refused() {
    assert_errors(
        "fn f(x: Float, n: Int) -> Float {\n    x + n\n}",
        &[("E0237", "n")],
    );
    assert_errors("fn f(x: Float) -> Float {\n    x * 2\n}", &[("E0237", "2")]);
}

#[test]
fn mixed_comparison_is_refused() {
    assert_errors(
        "fn f(x: Float, n: Int) -> Bool {\n    x < n\n}",
        &[("E0237", "n")],
    );
    assert_errors(
        "fn f(x: Float, n: Int) -> Bool {\n    x == n\n}",
        &[("E0237", "n")],
    );
}

#[test]
fn a_float_is_not_an_int_and_keeps_the_mismatch_code() {
    assert_errors("fn f(x: Float) -> Int {\n    x\n}", &[("E0220", "x")]);
}

// ---- E0222: a literal out of range --------------------------------------------

#[test]
fn a_float_literal_that_rounds_to_infinity_is_e0222() {
    assert_refused(
        "fn f() -> Float {\n    1e999\n}",
        "E0222",
        "1e999",
        "does not fit in `Float`",
    );
    assert_errors("fn f() -> Float {\n    -1e999\n}", &[("E0222", "1e999")]);
}

#[test]
fn the_largest_float_and_a_literal_too_small_to_see_are_fine() {
    assert_ok(
        "fn f() -> Float {\n    let a = 1.7976931348623157e308\n    let b = 1e-999\n    a + b\n}",
    );
}

// ---- E0357: Secret<Float> ------------------------------------------------------

#[test]
fn secret_float_as_a_parameter_is_refused() {
    assert_refused(
        "fn f(k: Secret<Float>) {\n}",
        "E0357",
        "Secret<Float>",
        "not constant-time",
    );
}

#[test]
fn secret_float_as_a_return_type_is_refused() {
    assert_errors(
        "fn f(k: Secret<Float>) -> Secret<Float> {\n    k\n}",
        &[("E0357", "Secret<Float>"), ("E0357", "Secret<Float>")],
    );
}

#[test]
fn secret_float_in_a_field_is_refused() {
    assert_errors(
        "struct Key { value: Secret<Float> }",
        &[("E0357", "Secret<Float>")],
    );
}

#[test]
fn secret_float_in_a_let_annotation_is_refused() {
    assert_errors(
        "fn f() {\n    let k: Secret<Float> = Secret.new(1.5)\n}",
        &[("E0357", "Secret<Float>"), ("E0357", "Secret.new(1.5)")],
    );
}

#[test]
fn secret_float_inside_another_type_is_refused() {
    assert_errors(
        "fn f(ks: List<Secret<Float>>) {\n}",
        &[("E0357", "Secret<Float>")],
    );
}

#[test]
fn secret_new_of_a_float_is_refused_with_the_secret_float_code() {
    assert_refused(
        "fn f() {\n    let k = Secret.new(1.5)\n}",
        "E0357",
        "Secret.new(1.5)",
        "`Secret<Float>`",
    );
}

#[test]
fn secret_new_of_a_float_variable_is_refused() {
    assert_errors(
        "fn f(x: Float) {\n    let k = Secret.new(x)\n}",
        &[("E0357", "Secret.new(x)")],
    );
}

#[test]
fn the_secret_types_that_exist_still_check() {
    assert_ok("fn f(a: Secret<U32>, b: Secret<Bool>, c: Secret<Bytes>) {\n}");
}

// ---- A secret cannot reach text -------------------------------------------------

#[test]
fn a_secret_cannot_be_interpolated() {
    assert_errors(
        "fn f(k: Secret<U32>) -> String {\n    \"{k}\"\n}",
        &[("E0350", "{k}")],
    );
}

#[test]
fn a_secret_cannot_be_turned_into_text() {
    assert_errors(
        "fn f(k: Secret<U32>) -> String {\n    k.to_string()\n}",
        &[("E0352", "to_string")],
    );
}

#[test]
fn a_secret_cannot_be_passed_to_a_formatter() {
    // `fmt` is `Float`'s method, so a secret reaches it only as a
    // `Secret<Float>`, which cannot be written, or as its receiver: a
    // `Secret<U32>` has no `fmt`.
    let report = check("fn f(k: Secret<U32>) -> String {\n    k.fmt(2)\n}");
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        ["E0228"],
        "{:#?}",
        report.diagnostics
    );
}
