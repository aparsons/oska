// SPDX-License-Identifier: Apache-2.0
//! `sorted`, `sort` and `sorted_by` on `List` (spec #660, ticket #666, ADR
//! 0034): a stable merge sort written in Oska in `std.core`. Every program
//! also runs natively on Linux (`common/mod.rs`), so both backends give the
//! same output. The step counts of both backends are compared in
//! `crates/oskac/tests/list_sort_steps.rs`.
//!
//! Sorting a list of `Option` or `List` items needs ticket #665 (`Ord` for
//! both); those tests join this file once it merges.

mod common;

use common::{assert_runs, run_under_limit};
use oska_run::{FakeHost, Limits};

fn main_with(body: &str) -> String {
    format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n{body}?\n    Ok(())\n}}\n\
         fn show(xs: List<Int>) -> String {{\n    \
         xs.fold(\"\", |acc, x| \"{{acc}}{{x}},\")\n}}\n"
    )
}

#[test]
fn sorted_on_empty_one_and_many() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         io.println(show(none.sorted()))?\n    \
         io.println(show([5].sorted()))?\n    \
         io.println(show([3, 1, 2].sorted()))?\n    \
         io.println(show([2, 2, 1, 1, 3, 3, 0].sorted()))?\n    \
         io.println(show([-5, 7, -1, 0, 7].sorted()))",
    );
    assert_runs(&src, &[], "\n5,\n1,2,3,\n0,1,1,2,2,3,3,\n-5,-1,0,7,7,\n");
}

#[test]
fn sorted_leaves_the_receiver_alone() {
    let src = main_with(
        "    let xs = [3, 1, 2]\n    \
         let ys = xs.sorted()\n    \
         io.println(show(xs))?\n    \
         io.println(show(ys))",
    );
    assert_runs(&src, &[], "3,1,2,\n1,2,3,\n");
}

#[test]
fn sort_changes_a_var_list_in_place() {
    let src = main_with(
        "    var xs = [9, 4, 7, 1]\n    \
         xs.sort()\n    \
         io.println(show(xs))?\n    \
         var none: List<Int> = []\n    \
         none.sort()\n    \
         io.println(show(none))",
    );
    assert_runs(&src, &[], "1,4,7,9,\n\n");
}

#[test]
fn sort_on_a_let_list_is_refused() {
    let src = main_with("    let xs = [2, 1]\n    xs.sort()\n    io.println(\"x\")");
    common::assert_refused(&src, "E0230");
}

#[test]
fn sorted_orders_text_and_bool() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let words = [\"pear\", \"apple\", \"\", \"apples\", \"Zed\", \"\u{e9}clair\"].sorted()\n    \
               io.println(\"|\".join(words))?\n    \
               let flags = [true, false, true, false].sorted()\n    \
               let s = flags.fold(\"\", |acc, b| if b { \"{acc}T\" } else { \"{acc}F\" })\n    \
               io.println(s)\n}\n";
    assert_runs(src, &[], "|Zed|apple|apples|pear|\u{e9}clair\nFFTT\n");
}

#[test]
fn sorted_by_takes_a_comparison_and_can_reverse() {
    let src = main_with(
        "    let xs = [3, 1, 2, 5, 4]\n    \
         io.println(show(xs.sorted_by(|a, b| b.cmp(a))))?\n    \
         io.println(show(xs.sorted_by(|a, b| a.cmp(b))))?\n    \
         io.println(show(xs.sorted_by(|a, b| (a % 2).cmp(b % 2))))",
    );
    // The last is stable: the evens 2, 4 and the odds 3, 1, 5 keep their order.
    assert_runs(&src, &[], "5,4,3,2,1,\n1,2,3,4,5,\n2,4,3,1,5,\n");
}

#[test]
fn sorted_by_is_stable_on_equal_keys() {
    // Sort pairs by the first field only: the second field shows the
    // original order of equal keys.
    let src = "struct Row { key: Int, tag: Int }\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               var rows: List<Row> = []\n    \
               for i in 0..40 {\n        \
               rows.push(Row(key: (i * 7) % 5, tag: i))\n    }\n    \
               let out = rows.sorted_by(|a, b| a.key.cmp(b.key))\n    \
               var ok = true\n    \
               var at = 1\n    \
               while at < out.len() {\n        \
               let p = out.at(at - 1)\n        \
               let q = out.at(at)\n        \
               if p.key > q.key {\n            ok = false\n        }\n        \
               if p.key == q.key && p.tag > q.tag {\n            ok = false\n        }\n        \
               at = at + 1\n    }\n    \
               if ok { io.println(\"stable\") } else { io.println(\"unstable\") }\n}\n";
    assert_runs(src, &[], "stable\n");
}

/// A fixed-seed list of 300 numbers from a linear congruential generator,
/// sorted by the number mod 10 (many equal keys), checked for order and
/// stability, and summarised by a checksum so both backends must agree.
const RANDOM: &str = "struct Row { key: Int, tag: Int }\n\
fn main(io: Stdout) -> Result<(), IoError> {\n    \
var seed = 12345\n    \
var rows: List<Row> = []\n    \
for i in 0..300 {\n        \
seed = (seed * 1103515245 + 12345) % 2147483648\n        \
rows.push(Row(key: (seed / 65536) % 10, tag: i))\n    }\n    \
let out = rows.sorted_by(|a, b| a.key.cmp(b.key))\n    \
var ok = true\n    \
var sum = 0\n    \
var at = 0\n    \
while at < out.len() {\n        \
let q = out.at(at)\n        \
sum = (sum * 31 + q.key * 1000 + q.tag) % 1000000007\n        \
if at > 0 {\n            \
let p = out.at(at - 1)\n            \
if p.key > q.key {\n                ok = false\n            }\n            \
if p.key == q.key && p.tag > q.tag {\n                ok = false\n            }\n        }\n        \
at = at + 1\n    }\n    \
let count = out.len()\n    io.println(\"{count} {sum}\")?\n    \
if ok { io.println(\"ordered\") } else { io.println(\"broken\") }\n}\n";

#[test]
fn a_seeded_random_list_sorts_stably() {
    let outcome = common::run_fake(RANDOM, &[]);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert!(
        outcome.stdout.starts_with("300 ") && outcome.stdout.ends_with("ordered\n"),
        "{outcome:#?}"
    );
}

#[test]
fn a_comparison_that_is_not_a_total_order_stays_in_bounds() {
    // Always `Less`, always `Greater`, and a parity-based mix: the result
    // has the same items in some order, and nothing reads out of bounds.
    let src = main_with(
        "    let xs = [5, 3, 8, 1, 9, 2, 7]\n    \
         let a = xs.sorted_by(|p, q| Ordering.Less)\n    \
         let b = xs.sorted_by(|p, q| Ordering.Greater)\n    \
         let c = xs.sorted_by(|p, q| if p % 2 == 0 { Ordering.Less } else { Ordering.Greater })\n    \
         io.println(show(a.sorted()))?\n    \
         io.println(show(b.sorted()))?\n    \
         io.println(show(c.sorted()))",
    );
    let each = "1,2,3,5,7,8,9,\n";
    assert_runs(&src, &[], &each.repeat(3));
}

#[test]
fn a_panicking_comparison_stops_the_sort_and_never_changes_the_receiver() {
    // `sorted_by` builds a new list and `sort` assigns it only after the
    // sort has finished, so a panic in `cmp` cannot leave a half-sorted
    // list behind. Here `cmp` panics (`P0002`) before anything is printed.
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               var xs = [4, 2, 9, 1]\n    \
               let none: List<Int> = []\n    \
               xs = xs.sorted_by(|a, b| none.at(0).cmp(a))\n    \
               let n = xs.len()\n    io.println(\"{n}\")\n}\n";
    let outcome = common::run_fake(src, &[]);
    let panic = outcome.panic.as_ref().expect("the comparison panics");
    assert_eq!(panic.code, "P0002", "{outcome:#?}");
    assert_eq!(outcome.stdout, "");
}

/// A comparison that panics only once the sort is under way: the items
/// before the one it panics on have already been compared.
const PANICS_MIDWAY: &str = "fn main(io: Stdout) -> Result<(), IoError> {
    var xs = [4, 2, 1, 7, 3, 9, 5]
    let none: List<Int> = []
    let n = xs.len()
    io.println(\"before {n}\")?
    xs = xs.sorted_by(|a, b| if a == 9 { none.at(0).cmp(b) } else { a.cmp(b) })
    let m = xs.len()
    io.println(\"after {m}\")?
    Ok(())
}
";

#[test]
fn a_comparison_that_panics_midway_ends_the_program_before_anything_uses_the_list() {
    // A panic ends the program, so the list cannot be seen half sorted:
    // the run prints what came before the sort and nothing after. The
    // receiver is never written, because `sorted_by` makes a new list.
    let outcome = common::run_fake(PANICS_MIDWAY, &[]);
    let panic = outcome.panic.as_ref().expect("the comparison panics");
    assert_eq!(panic.code, "P0002", "{outcome:#?}");
    assert_eq!(outcome.stdout, "before 7\n");
}

/// A program that sorts `n` numbers in descending order.
fn big_sort(n: i64) -> String {
    format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n    \
         var xs: List<Int> = []\n    \
         for i in 0..{n} {{\n        xs.push({n} - i)\n    }}\n    \
         let ys = xs.sorted()\n    \
         let first = ys.at(0)\n    let n = ys.len()\n    io.println(\"{{first}} {{n}}\")\n}}\n"
    )
}

#[test]
fn a_large_sort_counts_against_the_step_limit() {
    let src = big_sort(2000);
    let limits = Limits {
        max_steps: Some(5000),
        ..Limits::NONE
    };
    let outcome = oska_run::run_with_limits(
        &src,
        &mut FakeHost::new(),
        &[common::PROGRAM_PATH.to_string()],
        limits,
    );
    let panic = outcome.panic.as_ref().expect("the step limit stops it");
    assert_eq!(panic.code, "P0038", "{outcome:#?}");
    assert_runs(&src, &[], "1 2000\n");
}

#[test]
fn a_large_sort_counts_against_the_memory_limit() {
    let src = big_sort(50_000);
    let outcome = run_under_limit(&src, 1 << 20);
    let panic = outcome.panic.as_ref().expect("the memory limit stops it");
    assert_eq!(panic.code, "P0040", "{outcome:#?}");
}
