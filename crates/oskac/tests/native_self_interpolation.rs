// SPDX-License-Identifier: Apache-2.0
//! `{self.field}` in a string (#85, spec #184), run on both backends
//! through the differential harness: the interpreter and a native
//! executable print the same text.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

#[test]
fn a_self_field_interpolates_the_same_on_both_backends() {
    let src = r#"struct Inner { n: Int }

struct Account { name: String, balance: Int, inner: Inner }

impl Account {
    fn show(self) -> String {
        "{self.name} has {self.balance} ({self.inner.n})"
    }

    fn deposit(var self, amount: Int) -> String {
        self.balance = self.balance + amount
        "now {self.balance}"
    }

    fn later(self) -> String {
        let f = || {
            "closure {self.name}"
        }
        f()
    }
}

trait Describe {
    fn describe(self) -> String
}

impl Describe for Account {
    fn describe(self) -> String {
        "account {self.name}"
    }
}

fn main(io: Stdout) {
    var a = Account(name: "ada", balance: 10, inner: Inner(n: 3))
    let _ = io.println(a.show())
    let _ = io.println(a.deposit(amount: 5))
    let _ = io.println(a.later())
    let _ = io.println(a.describe())
}
"#;
    let outcome = assert_same(&Case::file("self_interpolation", src));
    assert_eq!(
        outcome.stdout, "ada has 10 (3)\nnow 15\nclosure ada\naccount ada\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}
