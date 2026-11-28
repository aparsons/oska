// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s typed queries, scan budgets and narrowing (#852, spec #842,
//! ADR 0040). A query is built from a record's fields as functions, a scan
//! takes a row budget and stops with an error when the store holds more
//! live records than that, every row comes back with its label (the higher
//! of the record's and the capability's), and `read_only` narrows a writer
//! to a reader that cannot write. Each program runs on the interpreter and
//! natively (`common/mod.rs`); the check-time half (unknown field, wrong
//! type, no write methods) is `oska-check`'s `store_queries`.

mod common;

use common::{assert_refused, run_differential};
use oska_run::FakeHost;

/// `main` with `io`, `read` and `dir` on `/data`, `key`, an untrusted
/// codec `codec` and a validating one `vcodec`; `body` runs inside it.
fn program(body: &str) -> String {
    format!(
        r#"use std.json
use std.store

struct Item {{ name: Untrusted<String>, qty: Int }}

fn to_value(item: Item) -> json.Value {{
    json.Value.Object([
        json.Member(key: "name", value: json.Value.Text(item.name)),
        json.Member(key: "qty", value: json.Value.Number(json.Number.from_int(item.qty))),
    ])
}}

fn from_value(v: json.Value) -> Option<Item> {{
    let name = v.text("name")?
    let qty = v.int("qty")?
    Some(Item(name: name, qty: qty))
}}

fn show(io: Stdout, result: Result<List<store.Row<Item>>, store.StoreError>) -> Result<(), IoError> {{
    match result {{
        Ok(rows) => {{
            let n = rows.len()
            io.println("{{n}} rows")?
            for row in rows {{
                let trust = if row.label.is_trusted() {{ "trusted" }} else {{ "untrusted" }}
                let class = row.label.classification_name()
                io.println("{{row.key}} {{row.value.name}} {{row.value.qty}} {{class}} {{trust}}")?
            }}
        }}
        Err(e) => io.println(e.message())?
    }}
    Ok(())
}}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{
    let read = fs.read_only().restrict_to("/data")
    let dir = fs.write_only().restrict_to("/data")
    let key = Secret.new("query key".to_bytes())
    let codec = store.Codec.new(to_value, from_value)
    let vcodec = store.Codec.validated(to_value, from_value)
{body}
    Ok(())
}}
"#
    )
}

fn run_with(host: &mut FakeHost, body: &str) -> String {
    let out = run_differential(host, &program(body), &[]);
    assert!(
        out.diagnostics.is_empty() && out.panic.is_none() && out.exit_code == 0,
        "{body}: {out:#?}"
    );
    out.stdout
}

/// Four records: three through the untrusted codec, then `d` through the
/// validating one; then `b` is deleted.
const WRITE: &str = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("a", Item(name: "pen", qty: 3))
        let _ = st.put("b", Item(name: "ink", qty: 5))
        let _ = st.put("c", Item(name: "pad", qty: 9))
        let _ = st.delete("b")
        let _ = st.put("e", Item(name: "pin", qty: 1))
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }
    let r2 = store.open(read, dir, key, vcodec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("d", Item(name: "cap", qty: 7))
        "done"
    })
    match r2 {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;

fn written() -> FakeHost {
    let mut host = FakeHost::new().with_dir("data");
    assert_eq!(run_with(&mut host, WRITE), "done\ndone\n");
    host
}

fn scan(query: &str, budget: &str) -> String {
    format!(
        r#"    match store.read(read, key, codec) {{
        Ok(rd) => {{
            let q: store.Query<Item> = {query}
            show(io, rd.scan(q, {budget}))?
        }}
        Err(e) => io.println(e.message())?
    }}"#
    )
}

#[test]
fn a_scan_with_no_condition_returns_the_live_records_in_order_with_labels() {
    let mut host = written();
    assert_eq!(
        run_with(&mut host, &scan("store.Query.all()", "10")),
        "4 rows\n\
         a pen 3 public untrusted\n\
         c pad 9 public untrusted\n\
         e pin 1 public untrusted\n\
         d cap 7 public trusted\n"
    );
}

#[test]
fn a_query_filters_by_the_records_fields() {
    let mut host = written();
    let cases = [
        (
            "store.Query.all().int_ge(|i: Item| i.qty, 3)",
            "a",
            "3 rows",
        ),
        (
            "store.Query.all().int_le(|i: Item| i.qty, 3)",
            "a",
            "2 rows",
        ),
        (
            "store.Query.all().text_eq(|i: Item| i.name, \"pad\")",
            "c",
            "1 rows",
        ),
        (
            "store.Query.all().text_starts_with(|i: Item| i.name, \"p\")",
            "a",
            "3 rows",
        ),
    ];
    for (query, first, count) in cases {
        let out = run_with(&mut host, &scan(query, "10"));
        assert!(
            out.starts_with(&format!("{count}\n{first} ")),
            "{query}: {out}"
        );
    }
    // Conditions join with "and".
    let out = run_with(
        &mut host,
        &scan(
            "store.Query.all().text_starts_with(|i: Item| i.name, \"p\").int_ge(|i: Item| i.qty, 3)",
            "10",
        ),
    );
    assert_eq!(
        out,
        "2 rows\na pen 3 public untrusted\nc pad 9 public untrusted\n"
    );
    // A query that matches nothing is an empty list, not an error.
    assert_eq!(
        run_with(
            &mut host,
            &scan("store.Query.all().int_ge(|i: Item| i.qty, 100)", "10")
        ),
        "0 rows\n"
    );
}

#[test]
fn a_scan_over_the_budget_stops_with_a_clear_error() {
    let mut host = written();
    // Four live records (the deleted one does not count).
    assert_eq!(
        run_with(&mut host, &scan("store.Query.all()", "3")),
        "store: scan over budget: 4 live records, budget 3\n"
    );
    // The budget counts the rows examined, not the rows returned.
    assert_eq!(
        run_with(
            &mut host,
            &scan("store.Query.all().int_ge(|i: Item| i.qty, 100)", "2")
        ),
        "store: scan over budget: 4 live records, budget 2\n"
    );
    assert!(run_with(&mut host, &scan("store.Query.all()", "4")).starts_with("4 rows\n"));
    assert_eq!(
        run_with(&mut host, &scan("store.Query.all()", "0")),
        "store: scan over budget: 4 live records, budget 0\n"
    );
    // A negative budget admits nothing.
    assert_eq!(
        run_with(&mut host, &scan("store.Query.all()", "-1")),
        "store: scan over budget: 4 live records, budget -1\n"
    );
}

#[test]
fn an_empty_store_scans_within_any_budget() {
    let mut host = FakeHost::new().with_dir("data");
    assert_eq!(
        run_with(&mut host, &scan("store.Query.all()", "0")),
        "0 rows\n"
    );
}

#[test]
fn a_writer_scans_what_it_has_written_and_a_delete_leaves_the_scan() {
    let mut host = written();
    let body = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("z", Item(name: "zip", qty: 2))
        let _ = st.delete("a")
        let q: store.Query<Item> = store.Query.all()
        match st.scan(q, 10) {
            Ok(rows) => {
                let n = rows.len()
                "{n}"
            }
            Err(e) => "error"
        }
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    assert_eq!(run_with(&mut host, body), "4\n");
}

#[test]
fn a_read_only_store_scans_and_reads_but_the_file_does_not_change() {
    let mut host = written();
    let before = host.file("data/log").expect("the log").to_string();
    let body = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        let ro = s.read_only()
        let q: store.Query<Item> = store.Query.all().int_ge(|i: Item| i.qty, 7)
        match ro.scan(q, 10) {
            Ok(rows) => {
                let n = rows.len()
                let total = ro.len()
                "{n} rows, {total} records"
            }
            Err(e) => "error"
        }
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    assert_eq!(run_with(&mut host, body), "2 rows, 4 records\n");
    assert_eq!(host.file("data/log").expect("the log"), before);
}

#[test]
fn rows_carry_the_higher_label_and_never_a_lower_one() {
    // `d` was written trusted and `a` untrusted; a scan through a reader
    // gives each its own label, and a record rewritten untrusted over a
    // trusted one comes back untrusted.
    let mut host = written();
    let body = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("d", Item(name: "cap", qty: 8))
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    assert_eq!(run_with(&mut host, body), "done\n");
    let out = run_with(
        &mut host,
        &scan("store.Query.all().text_eq(|i: Item| i.name, \"cap\")", "10"),
    );
    assert_eq!(out, "1 rows\nd cap 8 public untrusted\n");
}

#[test]
fn a_scan_after_a_snapshot_and_a_restart_gives_the_same_rows() {
    let mut host = written();
    let body = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.snapshot()
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    assert_eq!(run_with(&mut host, body), "done\n");
    let out = run_with(&mut host, &scan("store.Query.all()", "10"));
    assert!(out.starts_with("4 rows\n"), "{out}");
    assert!(out.contains("d cap 7 public trusted\n"), "{out}");
}

#[test]
fn text_from_a_scanned_row_stays_untrusted() {
    let src = program(
        "    match store.read(read, key, codec) {\n        Ok(rd) => {\n            \
         match rd.scan(store.Query.all(), 10) {\n                Ok(rows) => {\n                    \
         for row in rows {\n                        let n: String = row.value.name\n                        \
         io.println(n)?\n                    }\n                }\n                Err(_) => {}\n            }\n        }\n        \
         Err(_) => {}\n    }",
    );
    assert_refused(&src, "E0300");
}

#[test]
fn the_query_code_adds_no_audit_site() {
    let source = include_str!("../../oska-check/src/std/store.osk");
    assert!(!source.contains(".assume_trusted("));
    let exposures = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains(".expose("))
        .count();
    assert_eq!(exposures, 1);
}
