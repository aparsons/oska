//! Bitwise operators, shifts and rotations (ticket #220): `& | ^ ~ << >>`
//! and `rotl`/`rotr` on `U8`, `U32` and `U64` only, `E0223` on any other
//! type with a message naming the three, and `E0108`, with its machine
//! fix, where one meets a different binary operator without parentheses.

mod common;

use common::{assert_errors, assert_fix, assert_ok, check};

const TYPES: [&str; 3] = ["U8", "U32", "U64"];

#[test]
fn each_bitwise_operator_checks_on_each_allowed_type() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(a: {ty}, b: {ty}) -> {ty} {{\n    \
             let both = a & b\n    let either = a | b\n    let one = a ^ b\n    \
             let flipped = ~a\n    let up = a << b\n    let down = a >> b\n    \
             ((both | either) ^ one) & (flipped | up | down)\n}}"
        ));
    }
}

#[test]
fn a_literal_operand_or_shift_amount_takes_the_other_operands_type() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(a: {ty}) -> {ty} {{\n    let m = a & 15\n    let s = 1 << a\n    m | (s >> 3)\n}}"
        ));
        assert_ok(&format!(
            "fn f() -> {ty} {{\n    let x: {ty} = ~0\n    x\n}}"
        ));
    }
}

#[test]
fn the_result_has_the_operands_type() {
    assert_errors(
        "fn f(a: U8, b: U8) -> U32 {\n    a & b\n}",
        &[("E0220", "a & b")],
    );
}

#[test]
fn both_operands_must_have_the_same_type() {
    assert_errors(
        "fn f(a: U8, b: U32) -> U8 {\n    a & b\n}",
        &[("E0220", "b")],
    );
    // A shift amount has the left operand's type, not `Int`.
    assert_errors(
        "fn f(a: U32, n: Int) -> U32 {\n    a << n\n}",
        &[("E0220", "n")],
    );
}

#[test]
fn a_bitwise_operator_on_any_other_type_is_e0223_naming_the_three_types() {
    for ty in ["Int", "I8", "I16", "I32", "I64", "U16", "Bool", "String"] {
        for op in ["&", "|", "^", "<<", ">>"] {
            let src = format!("fn f(a: {ty}, b: {ty}) {{\n    let c = a {op} b\n}}");
            assert_errors(&src, &[("E0223", &format!("a {op} b"))]);
            let report = check(&src);
            assert_eq!(
                report.diagnostics[0].message,
                format!(
                    "`{op}` is not defined for `{ty}`; the bitwise and shift operators \
                     are defined for `U8`, `U32` and `U64` only"
                ),
            );
        }
        assert_errors(
            &format!("fn f(a: {ty}) {{\n    let c = ~a\n}}"),
            &[("E0223", "~a")],
        );
    }
}

#[test]
fn literals_alone_default_to_int_which_has_no_bitwise_operators() {
    assert_errors("fn f() {\n    let c = 1 & 2\n}", &[("E0223", "1 & 2")]);
    assert_errors("fn f() {\n    let c = ~0\n}", &[("E0223", "~0")]);
}

#[test]
fn rotations_take_an_int_amount_and_give_the_receivers_type() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(a: {ty}, n: Int) -> {ty} {{\n    a.rotl(n).rotr(3)\n}}"
        ));
        assert_errors(
            &format!("fn f(a: {ty}, n: {ty}) -> {ty} {{\n    a.rotl(n)\n}}"),
            &[("E0220", "n")],
        );
    }
}

#[test]
fn other_integer_types_have_no_rotations() {
    for ty in ["Int", "I32", "U16"] {
        let report = check(&format!("fn f(a: {ty}) -> {ty} {{\n    a.rotl(1)\n}}"));
        assert!(
            !report.diagnostics.is_empty(),
            "`{ty}.rotl` should not check"
        );
    }
}

#[test]
fn mixing_without_parentheses_is_e0108_with_a_fix_that_adds_them() {
    assert_errors(
        "fn f(x: U8, mask: U8) -> Bool {\n    x & mask == 0\n}",
        &[("E0108", "x & mask == 0")],
    );
    assert_fix(
        "fn f(x: U8, mask: U8) -> Bool {\n    x & mask == 0\n}",
        "E0108",
        "fn f(x: U8, mask: U8) -> Bool {\n    (x & mask) == 0\n}",
    );
    assert_fix(
        "fn f(a: U32, b: U32) -> U32 {\n    a + b << 3\n}",
        "E0108",
        "fn f(a: U32, b: U32) -> U32 {\n    (a + b) << 3\n}",
    );
    // The fixed programs check.
    assert_ok("fn f(x: U8, mask: U8) -> Bool {\n    (x & mask) == 0\n}");
    assert_ok("fn f(a: U32, b: U32) -> U32 {\n    (a + b) << 3\n}");
}

#[test]
fn bitwise_operators_are_outside_oska_core() {
    let report = oska_check::check_module(
        "fn main() {\n    let a: U8 = 12\n    let b = ~(a & 10) ^ a.rotl(1)\n}",
    );
    let program = report.program.expect("the program checks");
    assert!(oska_check::core_ir::export(&program).is_err());
}
