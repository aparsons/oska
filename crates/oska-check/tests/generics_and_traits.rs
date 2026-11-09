//! Generics and traits (#39): generic bodies are checked once against
//! their bounds, trait impls are complete, method calls resolve through
//! the type's own methods and then its traits, `==` needs `Eq`, and `?`
//! converts errors only through an explicit `impl From`.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- Generic bodies and equality --------------------------------------------

#[test]
fn a_generic_body_can_compare_values_of_an_eq_parameter() {
    assert_ok("fn same<T: Eq>(a: T, b: T) -> Bool { a == b }");
}

#[test]
fn a_generic_body_cannot_add_values_of_an_eq_parameter() {
    assert_errors(
        "fn sum<T: Eq>(a: T, b: T) -> T { a + b }",
        &[("E0223", "a + b")],
    );
}

#[test]
fn comparing_values_of_an_unbounded_parameter_needs_eq() {
    assert_errors(
        "fn same<T>(a: T, b: T) -> Bool { a != b }",
        &[("E0265", "a != b")],
    );
}

#[test]
fn the_fix_for_a_missing_eq_bound_adds_it() {
    assert_fix(
        "fn same<T>(a: T, b: T) -> Bool { a == b }",
        "E0265",
        "fn same<T: Eq>(a: T, b: T) -> Bool { a == b }",
    );
    assert_fix(
        "fn same<T: Hash>(a: T, b: T) -> Bool { a == b }",
        "E0265",
        "fn same<T: Hash + Eq>(a: T, b: T) -> Bool { a == b }",
    );
}

#[test]
fn primitives_and_options_of_them_are_eq() {
    assert_ok(
        "fn f(a: Int, b: U8, c: Bool, d: String, e: Option<Int>) -> Bool {\n    \
         a == 1 && b == 2 && c == true && d == \"x\" && e == Some(1)\n}",
    );
}

#[test]
fn a_struct_without_an_eq_impl_cannot_be_compared() {
    assert_errors(
        "struct Point { x: Int }\nfn f(a: Point, b: Point) -> Bool { a == b }",
        &[("E0265", "a == b")],
    );
}

#[test]
fn a_struct_with_an_eq_impl_can_be_compared() {
    assert_ok(
        "struct Point { x: Int }\n\
         impl Eq for Point {\n    fn eq(self, other: Point) -> Bool { self.x == other.x }\n}\n\
         fn f(a: Point, b: Point) -> Bool { a == b }",
    );
}

#[test]
fn an_option_of_a_type_without_eq_cannot_be_compared() {
    assert_errors(
        "struct Point { x: Int }\nfn f(a: Option<Point>, b: Option<Point>) -> Bool { a == b }",
        &[("E0265", "a == b")],
    );
}
