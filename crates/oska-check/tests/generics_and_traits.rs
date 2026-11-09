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

// ---- Methods on generic parameters and bounds at call sites -----------------

#[test]
fn a_generic_body_can_call_a_method_of_its_bound() {
    assert_ok("fn show<T: Display>(x: T) -> String { x.to_string() }");
}

#[test]
fn a_method_its_bounds_do_not_provide_is_an_error() {
    assert_errors(
        "fn show<T: Eq>(x: T) -> String { x.to_string() }",
        &[("E0260", "to_string")],
    );
}

#[test]
fn the_fix_for_a_method_missing_from_the_bounds_adds_the_trait_that_has_it() {
    assert_fix(
        "fn show<T>(x: T) -> String { x.to_string() }",
        "E0260",
        "fn show<T: Display>(x: T) -> String { x.to_string() }",
    );
}

#[test]
fn a_method_of_a_bound_is_typed_by_the_trait() {
    assert_errors(
        "fn show<T: Display>(x: T) -> Int { x.to_string() }",
        &[("E0220", "x.to_string()")],
    );
}

#[test]
fn a_trait_default_method_can_call_the_trait_s_other_methods() {
    assert_ok(
        "trait Greet {\n    fn name(self) -> String\n    fn greet(self) -> String {\n        let n = self.name()\n        \"hi {n}\"\n    }\n}",
    );
}

#[test]
fn a_call_must_satisfy_the_callee_s_bounds() {
    assert_errors(
        "struct Point { x: Int }\n\
         fn same<T: Eq>(a: T, b: T) -> Bool { a == b }\n\
         fn f(p: Point) -> Bool { same(p, p) }",
        &[("E0265", "same(p, p)")],
    );
}

// ---- Trait impls -----------------------------------------------------------------

#[test]
fn an_impl_that_provides_every_required_method_is_fine() {
    assert_ok(
        "trait Greet {\n    fn name(self) -> String\n    fn greet(self) -> String { \"hi\" }\n}\n\
         struct Cat { name: String }\n\
         impl Greet for Cat {\n    fn name(self) -> String { self.name }\n}",
    );
}

#[test]
fn an_impl_missing_a_required_method_is_an_error() {
    assert_errors(
        "trait Greet {\n    fn name(self) -> String\n    fn age(self) -> Int\n}\n\
         struct Cat { name: String }\n\
         impl Greet for Cat {\n    fn name(self) -> String { self.name }\n}",
        &[("E0261", "Greet")],
    );
}

#[test]
fn an_impl_method_with_a_different_signature_is_an_error() {
    assert_errors(
        "struct Cat { name: String }\n\
         impl Display for Cat {\n    fn to_string(self) -> Int { 1 }\n}",
        &[("E0262", "to_string")],
    );
    assert_errors(
        "struct Cat { name: String }\n\
         impl Display for Cat {\n    fn to_string(var self) -> String { self.name }\n}",
        &[("E0262", "to_string")],
    );
}

#[test]
fn an_impl_method_matches_with_self_and_the_trait_s_arguments_replaced() {
    assert_ok(
        "struct Point { x: Int }\n\
         impl Eq for Point {\n    fn eq(self, other: Self) -> Bool { self.x == other.x }\n}\n\
         struct Meters { value: Int }\n\
         impl From<Int> for Meters {\n    fn from(value: Int) -> Meters { Meters(value: value) }\n}",
    );
}

#[test]
fn an_impl_method_the_trait_does_not_declare_is_an_error() {
    assert_errors(
        "struct Cat { name: String }\n\
         impl Display for Cat {\n    fn to_string(self) -> String { self.name }\n    fn purr(self) -> String { \"purr\" }\n}",
        &[("E0263", "purr")],
    );
}

// ---- `?` converts errors through `impl From` --------------------------------------

/// A failing function and an error type with no conversions yet.
const ERRORS: &str = "fn fallible() -> Result<Int, IoError> { Ok(1) }\n\
struct AppError { message: String }\n";

fn with_errors(src: &str) -> String {
    format!("{ERRORS}{src}")
}

#[test]
fn try_converts_an_error_through_impl_from() {
    assert_ok(&with_errors(
        "impl From<IoError> for AppError {\n    fn from(value: IoError) -> AppError { AppError(message: \"io\") }\n}\n\
         fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}",
    ));
}

#[test]
fn try_without_an_impl_from_is_an_error() {
    assert_errors(
        &with_errors("fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}"),
        &[("E0242", "fallible()?")],
    );
}

#[test]
fn try_does_not_convert_through_a_from_method_of_another_trait() {
    assert_errors(
        &with_errors(
            "trait Convert {\n    fn from(value: IoError) -> Self\n}\n\
             impl Convert for AppError {\n    fn from(value: IoError) -> AppError { AppError(message: \"io\") }\n}\n\
             fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}",
        ),
        &[("E0242", "fallible()?")],
    );
}

#[test]
fn try_does_not_convert_through_an_impl_from_another_type() {
    assert_errors(
        &with_errors(
            "impl From<String> for AppError {\n    fn from(value: String) -> AppError { AppError(message: value) }\n}\n\
             fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}",
        ),
        &[("E0242", "fallible()?")],
    );
}

#[test]
fn try_converts_through_a_generic_impl_from() {
    assert_ok(&with_errors(
        "struct Wrapped<E> { inner: E }\n\
         impl<E> From<E> for Wrapped<E> {\n    fn from(value: E) -> Wrapped<E> { Wrapped(inner: value) }\n}\n\
         fn f() -> Result<Int, Wrapped<IoError>> {\n    let n = fallible()?\n    Ok(n)\n}",
    ));
}

// ---- Method lookup -------------------------------------------------------------

/// Two traits that both provide `describe`, and a type with both.
const TWO_TRAITS: &str = "trait Short {\n    fn describe(self) -> String\n}\n\
trait Long {\n    fn describe(self) -> String\n}\n\
struct Cat { name: String }\n\
impl Short for Cat {\n    fn describe(self) -> String { \"cat\" }\n}\n\
impl Long for Cat {\n    fn describe(self) -> String { \"a cat\" }\n}\n";

fn with_two_traits(src: &str) -> String {
    format!("{TWO_TRAITS}{src}")
}

#[test]
fn a_trait_method_is_found_on_an_implementing_type() {
    assert_ok(
        "struct Cat { name: String }\n\
         impl Display for Cat {\n    fn to_string(self) -> String { self.name }\n}\n\
         fn f(c: Cat) -> String { c.to_string() }",
    );
}

#[test]
fn a_type_s_own_method_wins_over_its_traits() {
    assert_ok(&with_two_traits(
        "impl Cat {\n    fn describe(self) -> Int { 1 }\n}\n\
         fn f(c: Cat) -> Int { c.describe() }",
    ));
}

#[test]
fn a_method_name_shared_by_two_traits_is_ambiguous() {
    assert_errors(
        &with_two_traits("fn f(c: Cat) -> String { c.describe() }"),
        &[("E0264", "describe")],
    );
}

#[test]
fn an_ambiguous_method_on_a_generic_parameter_is_an_error() {
    assert_errors(
        &with_two_traits("fn f<T: Short + Long>(c: T) -> String { c.describe() }"),
        &[("E0264", "describe")],
    );
}

#[test]
fn a_qualified_call_picks_the_trait() {
    assert_ok(&with_two_traits(
        "fn f(c: Cat) -> String { Long.describe(c) }\n\
         fn g<T: Short + Long>(c: T) -> String { Short.describe(c) }",
    ));
}

#[test]
fn a_qualified_call_needs_the_type_to_implement_the_trait() {
    assert_errors(
        &with_two_traits(
            "struct Dog { name: String }\nfn f(d: Dog) -> String { Long.describe(d) }",
        ),
        &[("E0265", "Long.describe(d)")],
    );
}

#[test]
fn a_qualified_call_of_a_method_the_trait_lacks_is_an_error() {
    assert_errors(
        &with_two_traits("fn f(c: Cat) -> String { Long.purr(c) }"),
        &[("E0228", "purr")],
    );
}

#[test]
fn a_call_that_satisfies_the_callee_s_bounds_is_fine() {
    assert_ok(
        "fn same<T: Eq>(a: T, b: T) -> Bool { a == b }\n\
         fn f(p: Int) -> Bool { same(p, 1) }",
    );
}
