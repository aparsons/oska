//! Module-level constants (#83, spec #184): a `const NAME: T = expr` is in
//! scope in every function, method and test of its module, with its
//! declared type, and its initializer is checked against that type like a
//! `let`'s.

mod common;

use common::{assert_authority, assert_errors, assert_ok};

#[test]
fn a_constant_is_usable_in_a_function() {
    assert_ok("const LIMIT: Int = 5\nfn limit() -> Int {\n    LIMIT\n}");
}

#[test]
fn a_constant_has_its_declared_type() {
    assert_ok(
        "const LIMIT: Int = 5\nconst NAME: String = \"hi\"\n\
         fn next() -> Int {\n    LIMIT + 1\n}\n\
         fn name() -> String {\n    NAME\n}",
    );
    assert_errors(
        "const LIMIT: Int = 5\nfn name() -> String {\n    LIMIT\n}",
        &[("E0220", "LIMIT")],
    );
}

#[test]
fn a_constant_is_usable_in_an_interpolation() {
    assert_ok("const NAME: String = \"hi\"\nfn greet() -> String {\n    \"hello {NAME}\"\n}");
}

#[test]
fn a_constant_is_usable_in_a_method_a_test_and_another_constant() {
    assert_ok(
        "const BASE: Int = 10\n\
         const NEXT: Int = BASE + 1\n\
         struct Meter { n: Int }\n\
         impl Meter {\n    fn over(self) -> Bool {\n        self.n > NEXT\n    }\n}\n\
         test \"constants\" {\n    t.assert_eq(NEXT, 11)\n}",
    );
}

#[test]
fn a_constant_declared_after_its_use_is_in_scope() {
    assert_ok("fn limit() -> Int {\n    LIMIT\n}\nconst LIMIT: Int = 5");
}

#[test]
fn a_wrong_typed_initializer_is_a_type_error_at_the_initializer() {
    assert_errors(
        "const LIMIT: Int = \"five\"\nfn limit() -> Int {\n    LIMIT\n}",
        &[("E0220", "\"five\"")],
    );
}

#[test]
fn a_local_shadows_a_constant() {
    assert_ok("const limit: Int = 5\nfn name(limit: String) -> String {\n    limit\n}");
}

#[test]
fn a_constant_is_pure() {
    assert_authority(
        "const LIMIT: Int = 5\nfn limit() -> Int {\n    LIMIT\n}",
        "limit",
        &[],
        true,
    );
}

#[test]
fn an_unknown_name_still_reports_e0221() {
    assert_errors(
        "const LIMIT: Int = 5\nfn limit() -> Int {\n    LIMT\n}",
        &[("E0221", "LIMT")],
    );
}
