// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s typed queries and narrowing (#852, ADR 0040): a query is
//! built from a record's fields as functions, never from text, so an
//! unknown field or a wrong type is a check error, and a read-only store
//! cannot name a write method. The CLI's rendering of these diagnostics is
//! the golden file in `crates/oskac/tests/golden/check_store_queries.txt`.

mod common;

use common::{assert_errors, assert_ok, check};

const HEAD: &str = "use std.store\n\nstruct Item { name: String, qty: Int }\n\n";

fn with(body: &str) -> String {
    format!("{HEAD}{body}\n")
}

#[test]
fn a_query_built_from_the_records_fields_checks() {
    assert_ok(&with(
        "fn q() -> store.Query<Item> {\n    \
         let a = store.Query.all().int_ge(|i: Item| i.qty, 2)\n    \
         let b = a.int_le(|i: Item| i.qty, 9)\n    \
         let c = b.text_eq(|i: Item| i.name, \"pen\")\n    \
         c.text_starts_with(|i: Item| i.name, \"p\")\n}\n\
         fn run(r: store.Reader<Item>) -> Result<List<store.Row<Item>>, store.StoreError> {\n    \
         r.scan(q(), 100)\n}\n\
         fn row(x: store.Row<Item>) -> Bool {\n    \
         x.label.is_trusted() && x.value.qty == 1 && x.key == \"k\"\n}\n",
    ));
}

#[test]
fn a_writer_scans_and_narrows_to_a_reader() {
    assert_ok(&with(
        "fn f(s: store.Store<Item>) -> Result<Int, store.StoreError> {\n    \
         let rows = s.scan(store.Query.all(), 10)?\n    \
         let r: store.Reader<Item> = s.read_only()\n    \
         Ok(rows.len() + r.len())\n}\n",
    ));
}

#[test]
fn an_unknown_field_is_a_check_error_that_suggests_the_nearest() {
    let src = with(
        "fn q() -> store.Query<Item> {\n    store.Query.all().int_ge(|i: Item| i.qyt, 2)\n}\n",
    );
    assert_errors(&src, &[("E0227", "qyt")]);
    let report = check(&src);
    let message = &report.diagnostics[0].message;
    assert!(
        message.contains("qty"),
        "suggests the nearest field: {message}"
    );
}

#[test]
fn a_wrong_type_in_a_query_is_a_check_error() {
    // A text where the field is an `Int`, a field of the wrong type, and
    // a number where the field is text.
    assert_errors(
        &with("fn q() -> store.Query<Item> {\n    store.Query.all().int_ge(|i: Item| i.qty, \"two\")\n}\n"),
        &[("E0220", "\"two\"")],
    );
    assert_errors(
        &with(
            "fn q() -> store.Query<Item> {\n    store.Query.all().int_ge(|i: Item| i.name, 2)\n}\n",
        ),
        &[("E0220", "i.name")],
    );
    assert_errors(
        &with("fn q() -> store.Query<Item> {\n    store.Query.all().text_eq(|i: Item| i.name, 2)\n}\n"),
        &[("E0220", "2")],
    );
}

#[test]
fn a_scan_needs_a_row_budget_that_is_an_int() {
    assert_errors(
        &with("fn f(r: store.Reader<Item>) -> Int {\n    match r.scan(store.Query.all()) {\n        Ok(rows) => rows.len()\n        Err(_) => 0\n    }\n}\n"),
        &[("E0224", "r.scan(store.Query.all())")],
    );
}

#[test]
fn attack_a_read_only_store_has_no_write_methods() {
    // The narrowed store cannot call put, delete or snapshot, and cannot
    // be widened back to a writer.
    for call in [
        "r.put(\"k\", Item(name: \"a\", qty: 1))",
        "r.delete(\"k\")",
        "r.snapshot()",
    ] {
        let src = with(&format!(
            "fn f(s: store.Store<Item>) -> Bool {{\n    var r = s.read_only()\n    let _ = {call}\n    true\n}}\n"
        ));
        let report = check(&src);
        let method = call
            .split('(')
            .next()
            .unwrap_or("")
            .trim_start_matches("r.");
        let found = report.diagnostics.iter().any(|d| {
            d.code == "E0228"
                && d.message.contains("store.Reader<Item>")
                && d.message.contains(&format!("no method `{method}`"))
        });
        assert!(
            found,
            "{call} should be a check error: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn a_reader_cannot_write_either() {
    let src = with(
        "fn f(r: store.Reader<Item>) -> Bool {\n    let _ = r.put(\"k\", Item(name: \"a\", qty: 1))\n    true\n}\n",
    );
    let report = check(&src);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "E0228" && d.message.contains("no method `put`")),
        "{:#?}",
        report.diagnostics
    );
}
