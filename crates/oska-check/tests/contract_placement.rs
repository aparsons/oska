//! Where a contract may live (#108): a trait method carries the contract
//! its implementations are held to, so a method of `impl Trait for Type`
//! declares none of its own (`E0218`); nothing calls `main`, so it
//! declares none either (`E0219`); and `?` or `return` directly in a
//! `requires` or `ensures` expression has nothing to leave (`E0282`).

mod common;

use common::{assert_errors, assert_ok};

const SHAPE: &str = "trait Shape {
    fn area(self) -> Int
        requires self.sides() > 0
        ensures |r| r >= 0
    fn sides(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn sides(self) -> Int { 4 }
";

// ---- Contracts on impl methods ----------------------------------------------------

#[test]
fn a_trait_method_may_declare_a_contract_for_its_impls_to_honour() {
    assert_ok(&format!(
        "{SHAPE}    fn area(self) -> Int {{ self.side * self.side }}\n}}\n"
    ));
}

#[test]
fn a_requires_on_a_trait_impl_method_is_an_error_pointing_at_the_clause() {
    let src = format!(
        "{SHAPE}    fn area(self) -> Int
        requires self.side > 0
    {{ self.side * self.side }}
}}
"
    );
    assert_errors(&src, &[("E0218", "requires self.side > 0")]);
}

#[test]
fn an_ensures_on_a_trait_impl_method_is_an_error_pointing_at_the_clause() {
    let src = format!(
        "{SHAPE}    fn area(self) -> Int
        ensures |r| r > 0
    {{ self.side * self.side }}
}}
"
    );
    assert_errors(&src, &[("E0218", "ensures |r| r > 0")]);
}

#[test]
fn every_contract_clause_of_an_impl_method_is_reported() {
    let src = format!(
        "{SHAPE}    fn area(self) -> Int
        requires self.side > 0
        ensures |r| r > 0
    {{ self.side * self.side }}
}}
"
    );
    assert_errors(
        &src,
        &[
            ("E0218", "requires self.side > 0"),
            ("E0218", "ensures |r| r > 0"),
        ],
    );
}

#[test]
fn a_method_of_an_inherent_impl_keeps_its_contract() {
    assert_ok(
        "struct Account { balance: Int }

impl Account {
    fn withdraw(self, amount: Int) -> Int
        requires amount > 0
        ensures |r| r >= 0
    {
        self.balance - amount
    }
}
",
    );
}

// ---- Contracts on `main` ------------------------------------------------------------

#[test]
fn a_requires_on_main_is_an_error_pointing_at_the_clause() {
    assert_errors(
        "fn main(args: Args)\n    requires 1 > 0\n{\n}\n",
        &[("E0219", "requires 1 > 0")],
    );
}

#[test]
fn an_ensures_on_main_is_an_error_pointing_at_the_clause() {
    assert_errors(
        "fn main()\n    ensures |r| r == ()\n{\n}\n",
        &[("E0219", "ensures |r| r == ()")],
    );
}

#[test]
fn a_function_other_than_main_may_take_a_contract() {
    assert_ok("fn helper(n: Int) -> Int\n    requires n > 0\n{\n    n\n}\nfn main() {\n}\n");
}

// ---- `?` and `return` in a contract --------------------------------------------------

#[test]
fn a_question_mark_directly_in_a_requires_is_an_error() {
    assert_errors(
        "fn maybe(n: Int) -> Option<Int> {\n    Some(n)\n}

fn f(n: Int) -> Option<Int>
    requires maybe(n)? > 0
{
    Some(n)
}
",
        &[("E0282", "maybe(n)?")],
    );
}

#[test]
fn a_question_mark_in_an_ensures_is_an_error() {
    assert_errors(
        "fn maybe(n: Int) -> Option<Int> {\n    Some(n)\n}

fn f(n: Int) -> Int
    ensures |r| maybe(r)? > 0
{
    n
}
",
        &[("E0282", "maybe(r)?")],
    );
}

#[test]
fn a_return_directly_in_a_contract_is_an_error() {
    assert_errors(
        "fn g(n: Int) -> Int\n    requires { return true }\n{\n    n\n}\n",
        &[("E0282", "return true")],
    );
    assert_errors(
        "fn g(n: Int) -> Int\n    ensures |r| { return r > 0 }\n{\n    n\n}\n",
        &[("E0282", "return r > 0")],
    );
}

#[test]
fn a_trait_methods_contract_is_held_to_the_same_rule() {
    assert_errors(
        "trait T {\n    fn f(self) -> Int\n        requires { return true }\n}\n",
        &[("E0282", "return true")],
    );
}

#[test]
fn a_closure_in_a_contract_may_use_question_mark_and_return_for_itself() {
    assert_ok(
        "fn count(xs: List<Int>) -> Int
    requires xs.map(|x| { return x > 0 }).len() == 2
{
    xs.len()
}
",
    );
}

#[test]
fn question_mark_and_return_in_a_body_are_unaffected() {
    assert_ok(
        "fn maybe(n: Int) -> Option<Int> {\n    Some(n)\n}

fn f(n: Int) -> Option<Int>
    requires n > 0
{
    let m = maybe(n)?
    if m > 1 {
        return Some(m)
    }
    Some(n)
}
",
    );
}
