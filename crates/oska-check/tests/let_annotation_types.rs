// SPDX-License-Identifier: Apache-2.0
//! An unknown type in a `let` annotation (#84, spec #184) is reported
//! `E0200` at the annotation, with a closest-name fix, from the same
//! resolution as an unknown type in a signature.

mod common;

use common::{assert_errors, assert_fix, assert_ok, check};

#[test]
fn an_unknown_annotation_type_is_reported_at_the_annotation() {
    assert_errors(
        "fn f() {\n    let x: Nonexistent = 1\n}",
        &[("E0200", "Nonexistent")],
    );
}

#[test]
fn an_unknown_type_argument_is_reported_at_that_argument() {
    assert_errors(
        "fn f() {\n    let xs: List<Nonexistent> = []\n}",
        &[("E0200", "Nonexistent")],
    );
}

#[test]
fn the_fix_replaces_the_name_with_the_closest_type() {
    assert_fix(
        "fn f() {\n    let x: Strng = \"a\"\n}",
        "E0200",
        "fn f() {\n    let x: String = \"a\"\n}",
    );
}

#[test]
fn a_var_and_a_nested_block_are_checked_too() {
    assert_errors(
        "fn f(ok: Bool) {\n    if ok {\n        var n: Itn = 1\n    }\n}",
        &[("E0200", "Itn")],
    );
}

#[test]
fn a_method_and_a_test_are_checked_too() {
    assert_errors(
        "struct S { n: Int }\nimpl S {\n    fn m(self) {\n        let x: Strng = \"a\"\n    }\n}\n\
         test \"t\" {\n    let y: Itn = 1\n}",
        &[("E0200", "Strng"), ("E0200", "Itn")],
    );
}

#[test]
fn a_known_type_a_generic_and_self_are_accepted() {
    assert_ok(
        "struct S { n: Int }\n\
         impl S {\n    fn m(self) -> Int {\n        let me: Self = self\n        let n: Int = me.n\n        n\n    }\n}\n\
         fn id<T>(x: T) -> T {\n    let y: T = x\n    y\n}",
    );
}

#[test]
fn the_json_report_carries_the_code_span_and_fix() {
    let src = "fn f() {\n    let x: Strng = \"a\"\n}";
    let start = src.find("Strng").expect("in the source");
    let json = check(src).to_json();
    for expected in [
        r#""code":"E0200""#,
        r#""message":"unknown type `Strng`; did you mean `String`?""#,
        &format!(r#""span":{{"start":{start},"end":{}}}"#, start + 5),
        r#""replacement":"String""#,
    ] {
        assert!(json.contains(expected), "no {expected} in {json}");
    }
}
