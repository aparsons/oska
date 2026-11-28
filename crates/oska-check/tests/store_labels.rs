// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s record labels (#850, ADR 0040): the label API checks, and
//! the trust flag holds in the types. Text decoded from a store is
//! untrusted and never fills a trusted slot (`E0300`); a label's trust is
//! read through a method, never forged from text.

mod common;

use common::{assert_errors, assert_ok};

const HEAD: &str = "use std.store\n";

#[test]
fn the_label_api_checks() {
    assert_ok(&format!(
        "{HEAD}fn f(r: store.Reader<Int>, k: Untrusted<String>) -> Bool {{\n    \
         match r.label(k) {{\n        Some(l) => l.is_trusted()\n        None => false\n    }}\n}}\n\
         fn g(a: store.Label, b: store.Label) -> store.Label {{\n    store.higher(a, b)\n}}\n\
         fn h() -> store.Label {{\n    \
         store.Label.new(store.Classification.Public, store.Trust.Untrusted)\n}}\n"
    ));
}

#[test]
fn a_validating_codec_is_a_codec() {
    assert_ok(&format!(
        "{HEAD}use std.json\nfn enc(n: Int) -> json.Value {{\n    \
         json.Value.Number(json.Number.from_int(n))\n}}\n\
         fn dec(v: json.Value) -> Option<Int> {{\n    v.int(\"n\")\n}}\n\
         fn c() -> store.Codec<Int> {{\n    store.Codec.validated(enc, dec)\n}}\n"
    ));
}

#[test]
fn attack_a_label_cannot_be_built_from_text() {
    // A label's trust is a value of `Trust`, not text from a file.
    assert_errors(
        &format!(
            "{HEAD}fn f(t: Untrusted<String>) -> store.Label {{\n    \
             store.Label.new(store.Classification.Public, t)\n}}\n"
        ),
        &[("E0220", "t")],
    );
}
