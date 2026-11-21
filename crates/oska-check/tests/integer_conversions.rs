//! Wrapping arithmetic and integer conversions (ticket #221):
//! `wrapping_add`, `wrapping_sub` and `wrapping_mul` on `U8`, `U32` and
//! `U64`; `to_<type>()` between `Int`, `U8`, `U16`, `U32` and `U64`; and
//! `wrapping_to_<type>()` from a wider unsigned type to a narrower one.

mod common;

use common::{assert_errors, assert_ok, check};

/// The types with the conversions, by name and by the method suffix
/// that converts to each.
const CONVERTIBLE: [(&str, &str); 5] = [
    ("Int", "int"),
    ("U8", "u8"),
    ("U16", "u16"),
    ("U32", "u32"),
    ("U64", "u64"),
];

#[test]
fn wrapping_arithmetic_takes_and_gives_the_receivers_type() {
    for ty in ["U8", "U32", "U64"] {
        assert_ok(&format!(
            "fn f(a: {ty}, b: {ty}) -> {ty} {{\n    \
             a.wrapping_add(b).wrapping_sub(1).wrapping_mul(b)\n}}"
        ));
        assert_errors(
            &format!("fn f(a: {ty}, n: Int) -> {ty} {{\n    a.wrapping_add(n)\n}}"),
            &[("E0220", "n")],
        );
    }
}

#[test]
fn other_integer_types_have_no_wrapping_arithmetic() {
    for ty in ["Int", "I8", "I64", "U16"] {
        let report = check(&format!(
            "fn f(a: {ty}) -> {ty} {{\n    a.wrapping_add(a)\n}}"
        ));
        assert!(
            !report.diagnostics.is_empty(),
            "`{ty}.wrapping_add` should not check"
        );
    }
}

#[test]
fn each_type_converts_to_each_other_type() {
    for (from, _) in CONVERTIBLE {
        for (to, suffix) in CONVERTIBLE {
            if from == to {
                continue;
            }
            assert_ok(&format!(
                "fn f(a: {from}) -> {to} {{\n    a.to_{suffix}()\n}}"
            ));
        }
    }
}

#[test]
fn a_conversion_gives_its_target_type_not_the_receivers() {
    assert_errors(
        "fn f(a: U8) -> U8 {\n    a.to_u32()\n}",
        &[("E0220", "a.to_u32()")],
    );
}

#[test]
fn wrapping_conversions_narrow_a_wider_unsigned_type() {
    for (from, targets) in [
        ("U16", &["u8"][..]),
        ("U32", &["u8", "u16"][..]),
        ("U64", &["u8", "u16", "u32"][..]),
    ] {
        for suffix in targets {
            let to = suffix.to_uppercase();
            assert_ok(&format!(
                "fn f(a: {from}) -> {to} {{\n    a.wrapping_to_{suffix}()\n}}"
            ));
        }
    }
}

#[test]
fn there_is_no_wrapping_conversion_that_widens_or_starts_from_int() {
    for src in [
        "fn f(a: U8) -> U32 {\n    a.wrapping_to_u32()\n}",
        "fn f(a: U32) -> U32 {\n    a.wrapping_to_u32()\n}",
        "fn f(a: Int) -> U8 {\n    a.wrapping_to_u8()\n}",
    ] {
        assert!(!check(src).diagnostics.is_empty(), "{src} should not check");
    }
}

#[test]
fn the_signed_sized_types_have_no_conversions() {
    for ty in ["I8", "I16", "I32", "I64"] {
        let report = check(&format!("fn f(a: {ty}) -> Int {{\n    a.to_int()\n}}"));
        assert!(
            !report.diagnostics.is_empty(),
            "`{ty}.to_int` should not check"
        );
    }
}

/// The Lean model of Oska Core needs no change for these methods: each
/// takes or gives a `U8`, `U16`, `U32` or `U64`, which the core IR export
/// refuses wherever it appears, and the export refuses every built-in it
/// does not list, so no program it accepts can call one.
#[test]
fn no_oska_core_program_can_call_a_conversion() {
    let export = |src: &str| {
        let report = oska_check::check_module(src);
        let program = report
            .program
            .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics));
        oska_check::core_ir::export(&program)
    };
    // Even one that starts and ends at `Int`.
    let refusal = export("fn main() {\n    let a = 12\n    let b = a.to_u8().to_int()\n}")
        .expect_err("a conversion is outside Oska Core");
    assert!(
        refusal.construct.contains("U8.to_int"),
        "{}",
        refusal.construct
    );
}
