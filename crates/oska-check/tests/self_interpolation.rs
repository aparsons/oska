// SPDX-License-Identifier: Apache-2.0
//! `{self.field}` in a string (#85, spec #184): `self` is the first segment
//! of an interpolation path inside a method, resolved like the receiver
//! anywhere else, with trust tracking unchanged.

mod common;

use common::{assert_errors, assert_ok};

#[test]
fn a_self_field_interpolates_in_a_method() {
    assert_ok(
        "struct Account { balance: Int }\n\
         impl Account {\n    fn show(self) -> String {\n        \"balance {self.balance}\"\n    }\n}",
    );
}

#[test]
fn a_self_field_interpolates_in_a_mutating_method_and_a_trait_impl() {
    assert_ok(
        "struct Account { balance: Int }\n\
         impl Account {\n    fn deposit(var self, n: Int) -> String {\n        \
         self.balance = self.balance + n\n        \"now {self.balance}\"\n    }\n}\n\
         trait Describe {\n    fn describe(self) -> String\n}\n\
         impl Describe for Account {\n    fn describe(self) -> String {\n        \"{self.balance}\"\n    }\n}",
    );
}

#[test]
fn a_nested_self_field_path_interpolates() {
    assert_ok(
        "struct Inner { n: Int }\nstruct Outer { inner: Inner }\n\
         impl Outer {\n    fn show(self) -> String {\n        \"{self.inner.n}\"\n    }\n}",
    );
}

#[test]
fn a_self_field_interpolates_inside_a_closure() {
    assert_ok(
        "struct Account { balance: Int }\n\
         impl Account {\n    fn show(self) -> String {\n        \
         let f = || {\n            \"{self.balance}\"\n        }\n        f()\n    }\n}",
    );
}

#[test]
fn a_missing_field_after_self_is_the_usual_unknown_field_error() {
    assert_errors(
        "struct Account { balance: Int }\n\
         impl Account {\n    fn show(self) -> String {\n        \"{self.balanse}\"\n    }\n}",
        &[("E0227", "balanse")],
    );
}

#[test]
fn self_outside_a_method_is_the_usual_error() {
    assert_errors(
        "fn show() -> String {\n    \"{self.balance}\"\n}",
        &[("E0221", "self")],
    );
}

#[test]
fn an_untrusted_self_field_makes_the_string_untrusted() {
    // The same refusal as `"{name}"` for a local of that type: the string
    // cannot pick a root.
    assert_errors(
        "struct Form { name: Untrusted<String> }\n\
         impl Form {\n    fn root(self, fs: Fs) {\n        \
         let d = fs.restrict_to(\"{self.name}\")\n    }\n}",
        &[("E0300", "\"{self.name}\"")],
    );
}

#[test]
fn a_trusted_self_field_keeps_the_string_trusted() {
    assert_ok(
        "struct Form { name: String }\n\
         impl Form {\n    fn root(self, fs: Fs) {\n        \
         let d = fs.restrict_to(\"{self.name}\")\n    }\n}",
    );
}

#[test]
fn a_secret_self_field_cannot_be_interpolated() {
    assert_errors(
        "struct Config { token: Secret<String> }\n\
         impl Config {\n    fn show(self) -> String {\n        \"{self.token}\"\n    }\n}",
        &[("E0350", "{self.token}")],
    );
}

#[test]
fn a_secret_holder_self_cannot_be_interpolated() {
    assert_errors(
        "struct Config { token: Secret<String> }\n\
         impl Config {\n    fn show(self) -> String {\n        \"{self}\"\n    }\n}",
        &[("E0350", "{self}")],
    );
}
