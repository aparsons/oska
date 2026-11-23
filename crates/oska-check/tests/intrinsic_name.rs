// SPDX-License-Identifier: Apache-2.0
//! A call site's intrinsic name keeps what a backend resolved it to
//! (#502), so the interpreter looks each call site's intrinsic up once.

use std::cell::Cell;

use oska_check::program::{Callee, IntrinsicName};

#[test]
fn a_name_is_resolved_on_its_first_use_only_and_keeps_the_result() {
    let name = IntrinsicName::new("Stdout.println");
    assert_eq!(name.resolved(), None);
    let calls = Cell::new(0);
    let resolve = |text: &str| {
        assert_eq!(text, "Stdout.println");
        calls.set(calls.get() + 1);
        7
    };
    assert_eq!(name.resolve_with(resolve), 7);
    assert_eq!(name.resolve_with(|_| unreachable!("resolved already")), 7);
    assert_eq!(name.resolved(), Some(7));
    assert_eq!(calls.get(), 1);
}

#[test]
fn two_names_are_equal_when_their_text_is_whether_or_not_either_is_resolved() {
    let resolved = IntrinsicName::new("text");
    resolved.resolve_with(|_| 3);
    assert_eq!(resolved, IntrinsicName::new("text"));
    assert_ne!(resolved, IntrinsicName::new("html.text"));
    assert_eq!(Callee::intrinsic("text"), Callee::Intrinsic(resolved));
}

#[test]
fn a_name_reads_as_its_text() {
    let name = IntrinsicName::new("Int.to_string");
    assert!(name == "Int.to_string");
    let owned = String::from("Int.to_string");
    assert!(name == owned);
    assert_eq!(name.as_str(), "Int.to_string");
    assert_eq!(name.to_string(), "Int.to_string");
    assert!(name.starts_with("Int."), "a name derefs to its text");
}
