//! A module constant's name is declared once (#83, spec #184): a second
//! constant, function or type of the same name is `E0202` at the second,
//! never a silent overwrite of the first.

mod common;

use common::{assert_errors, assert_modules_errors, assert_modules_ok, assert_ok, check};

/// The start offsets of the report's diagnostics.
fn starts(src: &str) -> Vec<usize> {
    check(src)
        .diagnostics
        .iter()
        .map(|d| d.span.start)
        .collect()
}

#[test]
fn two_constants_with_one_name_are_a_duplicate() {
    let src = "const LIMIT: Int = 5\nconst LIMIT: Int = 6\nfn limit() -> Int {\n    LIMIT\n}";
    assert_errors(src, &[("E0202", "LIMIT")]);
    assert_eq!(starts(src), [src.rfind("LIMIT: Int").expect("second")]);
}

#[test]
fn a_constant_and_a_later_function_with_one_name_are_a_duplicate() {
    let src = "const limit: Int = 5\nfn limit() -> Int {\n    5\n}";
    assert_errors(src, &[("E0202", "limit")]);
    assert_eq!(starts(src), [src.rfind("limit").expect("function")]);
}

#[test]
fn a_function_and_a_later_constant_with_one_name_are_a_duplicate() {
    let src = "fn limit() -> Int {\n    5\n}\nconst limit: Int = 5";
    assert_errors(src, &[("E0202", "limit")]);
    assert_eq!(starts(src), [src.rfind("limit").expect("constant")]);
}

#[test]
fn a_constant_and_a_type_with_one_name_are_a_duplicate() {
    assert_errors(
        "struct Limit { n: Int }\nconst Limit: Int = 5",
        &[("E0202", "Limit")],
    );
    assert_errors(
        "const Limit: Int = 5\nenum Limit { A }",
        &[("E0202", "Limit")],
    );
}

#[test]
fn a_constant_named_like_a_prelude_function_is_a_duplicate() {
    assert_errors("const todo: Int = 5", &[("E0202", "todo")]);
}

#[test]
fn constants_with_different_names_are_fine() {
    assert_ok(
        "const A: Int = 1\nconst B: Int = 2\nfn sum() -> Int {\n    A + B\n}\n\
         struct Meter { n: Int }\nimpl Meter {\n    fn a(self) -> Int {\n        A\n    }\n}",
    );
}

#[test]
fn a_package_reports_a_duplicate_constant_in_one_module() {
    let main = "const LIMIT: Int = 5\nconst LIMIT: Int = 6\nfn main() {\n}\n";
    assert_modules_errors(&[("main", main)], &[("src/main.osk", "E0202", "LIMIT")]);
}

#[test]
fn the_same_constant_name_in_two_modules_is_fine() {
    let util = "pub(pkg) const LIMIT: Int = 7\n";
    let main = "const LIMIT: Int = 5\nfn main() {\n}\n";
    assert_modules_ok(&[("main", main), ("util", util)]);
}
