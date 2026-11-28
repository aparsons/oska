// SPDX-License-Identifier: Apache-2.0
//! Trust flags through `Set`, `Deque`, `SortedSet` and `SortedMap` (#892, #806, spec #798, ADR 0006):
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

#[test]
fn a_sorted_set_keeps_its_untrusted_items_untrusted() {
    let src = with_greet(
        "fn f(s: SortedSet<Untrusted<String>>) -> String {\n    let items = s.to_list()\n    greet(items.at(0))\n}",
    );
    assert_errors(&src, &[("E0300", "items.at(0)")]);
    let src = with_greet(
        "fn f(s: SortedSet<Untrusted<String>>) -> String {\n    let first = s.first().unwrap_or(\"\")\n    greet(first)\n}",
    );
    assert_errors(&src, &[("E0300", "first")]);
}

#[test]
fn a_sorted_set_of_untrusted_text_answers_with_trusted_values() {
    assert_ok(&with_greet(
        "fn f(s: SortedSet<Untrusted<String>>, x: Untrusted<String>) -> String {\n    var t = s\n    let added = t.add(x)\n    let has = t.contains(x)\n    let n = t.len()\n    let none = t.is_empty()\n    let ok = if added && has && !none { 1 } else { 0 }\n    greet(\"{ok} {n}\")\n}",
    ));
}

#[test]
fn a_sorted_map_keeps_untrusted_keys_and_values_untrusted() {
    let src = with_greet(
        "fn f(m: SortedMap<Untrusted<String>, Int>) -> String {\n    let entries = m.to_list()\n    greet(entries.at(0).key)\n}",
    );
    assert_errors(&src, &[("E0300", "entries.at(0).key")]);
    let src = with_greet(
        "fn f(m: SortedMap<Int, Untrusted<String>>) -> String {\n    let v = m.get(1).unwrap_or(\"\")\n    greet(v)\n}",
    );
    assert_errors(&src, &[("E0300", "v")]);
    let src = with_greet(
        "fn f(m: SortedMap<Int, Untrusted<String>>) -> String {\n    let e = m.first().map(|x| x.value).unwrap_or(\"\")\n    greet(e)\n}",
    );
    assert_errors(&src, &[("E0300", "e")]);
}

#[test]
fn a_sorted_map_answers_with_trusted_lengths_and_membership() {
    assert_ok(&with_greet(
        "fn f(m: SortedMap<Untrusted<String>, Untrusted<String>>, k: Untrusted<String>, v: Untrusted<String>) -> String {\n    var t = m\n    let fresh = t.put(k, v)\n    let has = t.contains(k)\n    let n = t.len()\n    let gone = t.remove(k).is_some()\n    let ok = if fresh && has && gone { 1 } else { 0 }\n    greet(\"{ok} {n}\")\n}",
    ));
}
