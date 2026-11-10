//! Review fixes for spec #105, at runtime: contracts that could perform an
//! effect are refused before the program starts, a `P0007` message counts
//! its column in characters, and an inherent impl method keeps its own
//! contract while a trait impl method may not declare one.

mod common;

use common::{assert_panics, assert_refused, run_fake};

/// The message of the panic `src` stops with.
fn panic_message(src: &str) -> String {
    run_fake(src, &[]).panic.expect("a panic").message
}

#[test]
fn the_column_of_the_breaking_return_counts_characters() {
    // `return` is the 23rd character of its line, and its 28th byte.
    let src = "fn f(s: String) -> Int
    ensures |r| r > 0
{
    if s == \"ééééé\" { return -1 }
    1
}
fn main(out: Stdout) {
    let n = f(\"ééééé\")
}
";
    assert_panics(src, "P0007", "ensures |r| r > 0");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `f` ensures `r > 0`, \
         broken by the return at line 4, column 23"
    );
}

#[test]
fn a_contract_that_would_print_through_a_type_parameter_is_refused() {
    let src = "trait Speak {
    fn ok(self) -> Bool
}

impl Speak for Stdout {
    fn ok(self) -> Bool {
        let _ = self.println(\"effect\")
        true
    }
}

fn check<T: Speak>(x: T) -> Int
    requires x.ok()
{
    1
}

fn main(out: Stdout) {
    let n = check(out)
}
";
    assert_refused(src, "E0274");
}

#[test]
fn a_contract_that_would_print_through_equality_is_refused() {
    let src = "cap struct L { out: Stdout }

impl Eq for L {
    fn eq(self, other: L) -> Bool {
        let _ = self.out.println(\"effect\")
        true
    }
}

fn f(x: L) -> Int
    requires x == x
{
    1
}

fn main(out: Stdout) {
    let n = f(L(out: out))
}
";
    assert_refused(src, "E0280");
}

#[test]
fn an_inherent_impl_method_keeps_its_contract_and_a_trait_impl_method_may_not() {
    let inherent = "struct Account { balance: Int }

impl Account {
    fn withdraw(self, amount: Int) -> Int
        requires amount <= self.balance
    {
        self.balance - amount
    }
}

fn main() {
    let left = Account(balance: 5).withdraw(10)
}
";
    assert_panics(inherent, "P0006", "Account(balance: 5).withdraw(10)");
    let trait_impl = "struct Account { balance: Int }

trait Withdraw {
    fn withdraw(self, amount: Int) -> Int
}

impl Withdraw for Account {
    fn withdraw(self, amount: Int) -> Int
        requires amount <= self.balance
    {
        self.balance - amount
    }
}

fn main() {
    let left = Account(balance: 5).withdraw(10)
}
";
    assert_refused(trait_impl, "E0218");
}
