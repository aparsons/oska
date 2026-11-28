// SPDX-License-Identifier: Apache-2.0
//! Trust flags through `Set` and `Deque` (#892, spec #798, ADR 0006):
//! an item keeps its flag, and a length, a position or a membership answer
//! is trusted.

mod common;

use common::{assert_errors, assert_ok};

/// `fn greet(name: String)`, a trusted `String` parameter, then `body`.
fn with_greet(body: &str) -> String {
    format!("fn greet(name: String) -> String {{\n    name\n}}\n{body}")
}

#[test]
fn a_set_keeps_its_untrusted_items_untrusted() {
    let src = with_greet(
        "fn f(s: Set<Untrusted<String>>) -> String {\n    let items = s.to_list()\n    greet(items.at(0))\n}",
    );
    assert_errors(&src, &[("E0300", "items.at(0)")]);
}

#[test]
fn a_set_of_untrusted_text_answers_with_trusted_values() {
    assert_ok(&with_greet(
        "fn f(s: Set<Untrusted<String>>, x: Untrusted<String>) -> String {\n    var t = s\n    let added = t.add(x)\n    let has = t.contains(x)\n    let n = t.len()\n    let ok = if added && has { 1 } else { 0 }\n    greet(\"{ok} {n}\")\n}",
    ));
}

#[test]
fn a_deque_keeps_its_untrusted_items_untrusted() {
    let src = with_greet(
        "fn f(d: Deque<Untrusted<String>>) -> String {\n    let items = d.to_list()\n    greet(items.at(0))\n}",
    );
    assert_errors(&src, &[("E0300", "items.at(0)")]);
}

#[test]
fn a_deque_of_untrusted_text_answers_with_trusted_lengths_and_positions() {
    assert_ok(&with_greet(
        "fn f(d: Deque<Untrusted<String>>, x: Untrusted<String>) -> String {\n    var q = d\n    q.push_back(x)\n    let n = q.len()\n    let have = q.get(0).is_some()\n    let ok = if have { 1 } else { 0 }\n    greet(\"{n} {ok}\")\n}",
    ));
}
