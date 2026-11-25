// SPDX-License-Identifier: Apache-2.0
//! The failure message of a comparison of two long outputs (#740): it names
//! the first differing line and both lengths, and stays short.

mod common;

use common::{assert_text_eq, first_difference};

#[test]
fn equal_texts_have_no_difference() {
    assert_eq!(first_difference("a\nb\n", "a\nb\n"), None);
    assert_eq!(first_difference("", ""), None);
}

#[test]
fn a_different_line_is_named_with_its_number_and_both_sides() {
    let found = first_difference("a\nb\nc", "a\nx\nc").expect("they differ");
    assert!(found.contains("line 2"), "{found}");
    assert!(
        found.contains("\"b\"") && found.contains("\"x\""),
        "{found}"
    );
}

#[test]
fn a_text_that_ends_early_differs_at_the_first_missing_line() {
    let found = first_difference("a\nb", "a\nb\nc").expect("they differ");
    assert!(
        found.contains("line 3") && found.contains("None"),
        "{found}"
    );
    assert!(
        found.contains("3 bytes") && found.contains("5 bytes"),
        "{found}"
    );
}

#[test]
fn an_empty_text_differs_from_a_non_empty_one() {
    assert!(first_difference("", "x").is_some());
}

#[test]
fn a_failure_over_long_outputs_is_short() {
    let long: String = (0..4000).map(|i| format!("line {i}\n")).collect();
    let mut other = long.clone();
    other.push_str("extra\n");
    let found = first_difference(&long, &other).expect("they differ");
    assert!(found.len() < 1024, "{} bytes", found.len());
}

#[test]
#[should_panic(expected = "stdout differs: first difference at line 1")]
fn the_assertion_panics_with_that_message() {
    assert_text_eq("a", "b", "stdout");
}
