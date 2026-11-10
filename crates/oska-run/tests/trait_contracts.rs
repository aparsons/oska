//! Contracts on trait methods (#108, spec #105): the contract a trait
//! method declares binds every implementation, however the call reaches
//! it: through the trait (a generic function), by a qualified call such as
//! `Shape.area(x)`, or straight to the type with `x.area()`.

mod common;

use common::{assert_panics, assert_refused, assert_runs, run_fake};

/// `Shape`, whose `scale` needs a positive factor and whose result must
/// not shrink, with two impls: `Square` honours the trait and `Cheat`
/// breaks it. The caller's `main` is appended.
fn shapes(main: &str) -> String {
    format!(
        "trait Shape {{
    fn scale(self, k: Int) -> Int
        requires k > 0
        ensures |r| r >= self.size()
    fn size(self) -> Int
}}

struct Square {{ side: Int }}
struct Cheat {{ side: Int }}

impl Shape for Square {{
    fn scale(self, factor: Int) -> Int {{ self.side * factor }}
    fn size(self) -> Int {{ self.side }}
}}

impl Shape for Cheat {{
    fn scale(self, _: Int) -> Int {{ 0 }}
    fn size(self) -> Int {{ self.side }}
}}

fn through_the_trait<S: Shape>(s: S, k: Int) -> Int {{
    s.scale(k)
}}

{main}
"
    )
}

// ---- A trait method's requires, on each route ---------------------------------------

#[test]
fn a_call_through_the_trait_checks_the_requires() {
    let src = shapes("fn main() {\n    let n = through_the_trait(Square(side: 2), 0)\n}");
    assert_panics(&src, "P0006", "s.scale(k)");
}

#[test]
fn a_direct_method_call_checks_the_requires() {
    let src = shapes("fn main() {\n    let n = Square(side: 2).scale(-1)\n}");
    assert_panics(&src, "P0006", "Square(side: 2).scale(-1)");
}

#[test]
fn a_qualified_call_checks_the_requires() {
    let src = shapes("fn main() {\n    let n = Shape.scale(Square(side: 2), 0)\n}");
    assert_panics(&src, "P0006", "Shape.scale(Square(side: 2), 0)");
}

#[test]
fn the_requires_message_quotes_the_traits_clause_and_names_the_impl() {
    let src = shapes("fn main() {\n    let n = Square(side: 2).scale(0)\n}");
    let panic = run_fake(&src, &[]).panic.expect("a panic");
    assert_eq!(
        panic.message,
        "precondition violated: `<Square as Shape>.scale` requires `k > 0`"
    );
}

#[test]
fn an_impl_that_names_a_parameter_differently_is_still_checked() {
    // `Square` calls the parameter `factor` and `Cheat` ignores it: the
    // clause is the trait's, over the trait's `k`.
    let src = shapes("fn main() {\n    let n = Cheat(side: 2).scale(-3)\n}");
    assert_panics(&src, "P0006", "Cheat(side: 2).scale(-3)");
}

// ---- A trait method's ensures, on each route ----------------------------------------

#[test]
fn a_call_through_the_trait_checks_the_ensures_of_the_impl_that_ran() {
    let src = shapes("fn main() {\n    let n = through_the_trait(Cheat(side: 2), 3)\n}");
    assert_panics(&src, "P0007", "ensures |r| r >= self.size()");
}

#[test]
fn a_direct_method_call_checks_the_ensures() {
    let src = shapes("fn main() {\n    let n = Cheat(side: 2).scale(3)\n}");
    assert_panics(&src, "P0007", "ensures |r| r >= self.size()");
}

#[test]
fn a_qualified_call_checks_the_ensures() {
    let src = shapes("fn main() {\n    let n = Shape.scale(Cheat(side: 2), 3)\n}");
    assert_panics(&src, "P0007", "ensures |r| r >= self.size()");
}

#[test]
fn the_ensures_message_names_the_impl_and_the_return() {
    let src = shapes("fn main() {\n    let n = Cheat(side: 2).scale(3)\n}");
    let panic = run_fake(&src, &[]).panic.expect("a panic");
    assert_eq!(
        panic.message,
        "postcondition violated: `<Cheat as Shape>.scale` ensures `r >= self.size()`, \
         broken by the return at line 17, column 37"
    );
}

#[test]
fn an_impl_that_honours_the_contract_runs_on_every_route() {
    let src = shapes(
        "fn main(io: Stdout) {
    let a = through_the_trait(Square(side: 2), 3)
    let b = Square(side: 2).scale(4)
    let c = Shape.scale(Square(side: 2), 5)
    let _ = io.println(\"{a} {b} {c}\")
}",
    );
    assert_runs(&src, &[], "6 8 10\n");
}

// ---- Default methods and several impls ----------------------------------------------

#[test]
fn a_default_method_keeps_its_contract_in_every_impl_that_inherits_it() {
    let src = "trait Halve {
    fn half(self, n: Int) -> Int
        requires n % 2 == 0
    { n / 2 }
}

struct A { x: Int }
struct B { x: Int }
impl Halve for A {}
impl Halve for B {}

fn main() {
    let ok = A(x: 0).half(4)
    let bad = B(x: 0).half(3)
}
";
    assert_panics(src, "P0006", "B(x: 0).half(3)");
}

#[test]
fn an_overriding_impl_of_a_default_method_is_held_to_the_trait_contract() {
    let src = "trait Halve {
    fn half(self, n: Int) -> Int
        requires n % 2 == 0
    { n / 2 }
}

struct A { x: Int }
impl Halve for A {
    fn half(self, n: Int) -> Int { n }
}

fn main() {
    let bad = A(x: 0).half(3)
}
";
    assert_panics(src, "P0006", "A(x: 0).half(3)");
}

#[test]
fn a_trait_method_without_a_contract_is_unchecked() {
    let src = "trait Twice {
    fn twice(self, n: Int) -> Int
}

struct A { x: Int }
impl Twice for A {
    fn twice(self, n: Int) -> Int { n * 2 }
}

fn main(io: Stdout) {
    let n = A(x: 0).twice(-2)
    let _ = io.println(\"{n}\")
}
";
    assert_runs(src, &[], "-4\n");
}

// ---- Where a contract may not live ----------------------------------------------------

#[test]
fn a_contract_on_an_impl_method_refuses_the_program() {
    let src = "trait Shape {
    fn size(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn size(self) -> Int
        ensures |r| r > 0
    { self.side }
}

fn main() {
}
";
    assert_refused(src, "E0218");
}

#[test]
fn a_contract_on_main_refuses_the_program() {
    assert_refused("fn main()\n    requires 1 > 0\n{\n}\n", "E0219");
}

#[test]
fn a_question_mark_or_return_in_a_contract_refuses_the_program() {
    let question_mark = "fn maybe(n: Int) -> Option<Int> {
    Some(n)
}

fn f(n: Int) -> Option<Int>
    requires maybe(n)? > 0
{
    Some(n)
}

fn main() {
    let x = f(1)
}
";
    assert_refused(question_mark, "E0282");
    let early_return = "fn g(n: Int) -> Int
    requires { return true }
{
    n
}

fn main() {
    let x = g(1)
}
";
    assert_refused(early_return, "E0282");
}
