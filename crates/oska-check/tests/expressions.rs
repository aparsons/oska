//! Expression checking (#36): function bodies are type-checked with local,
//! bidirectional inference.

mod common;

use common::{assert_errors, assert_ok};

// ---- Bindings and inference ------------------------------------------------

#[test]
fn a_let_type_is_inferred_from_its_initializer() {
    assert_ok("fn f() -> Int {\n    let x = 5\n    x\n}");
    assert_errors(
        "fn f() -> String {\n    let x = 5\n    x\n}",
        &[("E0220", "x")],
    );
}

#[test]
fn shadowing_replaces_a_binding() {
    assert_ok("fn f(raw: Int) -> String {\n    let raw = \"{raw}\"\n    raw\n}");
}

#[test]
fn an_annotated_let_checks_its_initializer() {
    assert_ok("fn f() {\n    let s: String = \"hi\"\n}");
    assert_errors(
        "fn f() {\n    let s: String = true\n}",
        &[("E0220", "true")],
    );
}

#[test]
fn a_binding_with_an_unknowable_type_is_an_error() {
    assert_ok("fn f() -> List<Int> {\n    var xs = List.new()\n    xs.push(1)\n    xs\n}");
    assert_errors("fn f() {\n    let xs = List.new()\n}", &[("E0233", "xs")]);
}

// ---- Literals ----------------------------------------------------------------

#[test]
fn an_integer_literal_takes_its_expected_type() {
    assert_ok("fn f() -> U8 {\n    let x: U8 = 200\n    x\n}");
    assert_ok("fn f() -> I8 { -128 }");
}

#[test]
fn an_integer_literal_defaults_to_int() {
    assert_errors(
        "fn f() -> U8 {\n    let x = 200\n    x\n}",
        &[("E0220", "x")],
    );
}

#[test]
fn an_out_of_range_literal_is_an_error() {
    assert_errors("fn f() -> U8 { 300 }", &[("E0222", "300")]);
    assert_errors("fn f() -> I8 { -129 }", &[("E0222", "-129")]);
    assert_errors("fn f() -> U32 { -1 }", &[("E0223", "-1")]);
    assert_errors(
        "fn f() -> Int { 9_223_372_036_854_775_808 }",
        &[("E0222", "9_223_372_036_854_775_808")],
    );
}

#[test]
fn literals_have_their_types() {
    assert_ok("fn f() -> Bool { true }\nfn g() -> String { \"s\" }\nfn h() -> () { () }");
    assert_errors("fn f() -> Bool { \"s\" }", &[("E0220", "\"s\"")]);
}

#[test]
fn a_constant_is_checked_against_its_type() {
    assert_ok("const LIMIT: U16 = 1000");
    assert_errors("const LIMIT: U8 = 1000", &[("E0222", "1000")]);
}

// ---- Operators ---------------------------------------------------------------

#[test]
fn arithmetic_works_on_numbers_of_one_type() {
    assert_ok("fn f(a: U8, b: U8) -> U8 { a * b + 1 - a / b % 2 }");
    assert_ok("fn f(a: I32) -> I32 { 1 + a }");
}

#[test]
fn there_are_no_implicit_numeric_conversions() {
    assert_errors("fn f(a: Int, b: U8) -> Int { a + b }", &[("E0220", "b")]);
}

#[test]
fn plus_is_only_for_numbers() {
    let src = "fn f(a: String, b: String) -> String { a + b }";
    assert_errors(src, &[("E0223", "a + b")]);
    assert!(common::check(src).diagnostics[0]
        .message
        .contains("interpolation"));
}

#[test]
fn comparisons_produce_bool() {
    assert_ok("fn f(a: Int, b: Int) -> Bool { a < b && a != b || !(a >= b) }");
    assert_ok("fn f(a: String, b: String) -> Bool { a == b }");
    assert_errors(
        "fn f(a: Bool, b: Bool) -> Bool { a < b }",
        &[("E0223", "a < b")],
    );
    assert_errors(
        "fn f(a: Int, b: String) -> Bool { a == b }",
        &[("E0220", "b")],
    );
}

#[test]
fn logical_operators_need_bool() {
    assert_ok("fn f(a: Bool, b: Bool) -> Bool { a implies b }");
    assert_errors("fn f(a: Int) -> Bool { !a }", &[("E0220", "a")]);
    assert_errors("fn f(a: Int, b: Bool) -> Bool { a && b }", &[("E0220", "a")]);
}

#[test]
fn negation_needs_a_signed_number() {
    assert_ok("fn f(a: I16) -> I16 { -a }");
    assert_errors("fn f(a: U16) -> U16 { -a }", &[("E0223", "-a")]);
}
