// SPDX-License-Identifier: Apache-2.0
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

/// Assert that `src` reports one error, `E0228` at the name of `method`,
/// which a value of type `ty` does not have.
fn assert_no_method(src: &str, ty: &str, method: &str) {
    assert_errors(src, &[("E0228", method)]);
    assert!(
        check(src).diagnostics[0]
            .message
            .starts_with(&format!("`{ty}` has no method `{method}`")),
        "{src}: {:#?}",
        check(src).diagnostics
    );
}

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
        assert_no_method(
            &format!("fn f(a: {ty}) -> {ty} {{\n    a.wrapping_add(a)\n}}"),
            ty,
            "wrapping_add",
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
    for (from, to) in [("U8", "U32"), ("U32", "U32"), ("Int", "U8")] {
        let method = format!("wrapping_to_{}", to.to_lowercase());
        assert_no_method(
            &format!("fn f(a: {from}) -> {to} {{\n    a.{method}()\n}}"),
            from,
            &method,
        );
    }
}

#[test]
fn the_signed_sized_types_have_no_conversions() {
    for ty in ["I8", "I16", "I32", "I64"] {
        assert_no_method(
            &format!("fn f(a: {ty}) -> Int {{\n    a.to_int()\n}}"),
            ty,
            "to_int",
        );
    }
}

/// Every method this ticket adds, by the name the checked program calls
/// it by: the wrapping arithmetic, the conversions and the wrapping
/// conversions.
fn every_method() -> Vec<(String, String)> {
    let mut all = Vec::new();
    for ty in ["U8", "U32", "U64"] {
        for op in ["add", "sub", "mul"] {
            all.push((ty.to_string(), format!("wrapping_{op}")));
        }
    }
    for (from, _) in CONVERTIBLE {
        for (to, suffix) in CONVERTIBLE {
            if from != to {
                all.push((from.to_string(), format!("to_{suffix}")));
            }
        }
    }
    for (from, targets) in [
        ("U16", &["u8"][..]),
        ("U32", &["u8", "u16"][..]),
        ("U64", &["u8", "u16", "u32"][..]),
    ] {
        for suffix in targets {
            all.push((from.to_string(), format!("wrapping_to_{suffix}")));
        }
    }
    all
}

/// The Lean model of Oska Core needs no change for these methods: the
/// core IR export refuses every built-in it does not list, and each of
/// them takes or gives a `U8`, `U16`, `U32` or `U64`, which it refuses
/// wherever it appears, so no program it accepts can call one.
#[test]
fn no_oska_core_program_can_call_a_conversion_or_wrapping_method() {
    let export = |src: &str| {
        let report = oska_check::check_module(src);
        let program = report
            .program
            .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics));
        oska_check::core_ir::export(&program)
    };
    let methods = every_method();
    assert_eq!(methods.len(), 35);
    for (ty, method) in methods {
        // The receiver, made from an `Int` (`Int.to_u8` and the like are
        // refused too, but the outer call first).
        let receiver = if ty == "Int" {
            "12".to_string()
        } else {
            format!("12.to_{}()", ty.to_lowercase())
        };
        let argument = if method.starts_with("wrapping_") && !method.contains("_to_") {
            "1"
        } else {
            ""
        };
        let src = format!("fn main() {{\n    let b = {receiver}.{method}({argument})\n}}");
        let refusal = export(&src).expect_err("outside Oska Core");
        let name = format!("`{ty}.{method}`");
        assert!(
            refusal.construct.contains(&name),
            "{src}: refused for {}, not {name}",
            refusal.construct
        );
    }
}
