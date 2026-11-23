// SPDX-License-Identifier: Apache-2.0
//! The `Budget` capability (#251): authority to spend model tokens and
//! calls, whose counters live in the runtime. `main` receives an unlimited
//! one, `limit` narrows it, a reservation it cannot cover is refused with
//! `OverBudget` before anything is spent, and `t.budget` has fixed limits.
//! Every program here also runs natively (the differential harness).

mod common;

use common::{assert_runs, test_fake};
use oska_run::TestStatus;

/// A program whose `main` takes `io: Stdout` and `budget: Budget` and runs
/// `body`, with helpers that print what a budget reports.
fn program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{HELPERS}"
    )
}

const HELPERS: &str = r#"
fn amount(n: Option<Int>) -> String {
    match n {
        Some(n) => "{n}"
        None => "unlimited"
    }
}

fn left(budget: Budget) -> String {
    let left = budget.remaining()
    let tokens = amount(left.tokens)
    let calls = amount(left.calls)
    "{tokens} tokens, {calls} calls left"
}

fn spent(budget: Budget) -> String {
    let spent = budget.spent()
    let tokens = spent.tokens
    let calls = spent.calls
    "{tokens} tokens, {calls} calls spent"
}
"#;

#[test]
fn main_receives_an_unlimited_budget() {
    assert_runs(
        &program("    io.println(left(budget))?\n    io.println(spent(budget))?"),
        &[],
        "unlimited tokens, unlimited calls left\n0 tokens, 0 calls spent\n",
    );
}

#[test]
fn a_limit_is_clamped_to_what_its_parent_has_left() {
    assert_runs(
        &program(
            "    let outer = budget.limit(1000, 5)
    let inner = outer.limit(5000, 2)
    io.println(left(outer))?
    io.println(left(inner))?",
        ),
        &[],
        "1000 tokens, 5 calls left\n1000 tokens, 2 calls left\n",
    );
}

#[test]
fn a_reservation_the_budget_cannot_cover_is_refused_with_over_budget() {
    assert_runs(
        &program(
            "    let small = budget.limit(100, 3)
    match small.reserve(500) {
        Ok(_) => io.println(\"reserved\")?
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(small))?
    io.println(spent(small))?",
        ),
        &[],
        "over budget: asked for 500 tokens and 1 call, with 100 tokens and 3 calls left\n\
         100 tokens, 3 calls left\n0 tokens, 0 calls spent\n",
    );
}

#[test]
fn a_debit_spends_what_the_reply_used_and_returns_the_rest() {
    assert_runs(
        &program(
            "    let small = budget.limit(1000, 3)
    match small.reserve(400) {
        Ok(r) => {
            io.println(left(small))?
            r.debit(150, 0, 0, 0)
        }
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(small))?
    io.println(spent(small))?
    io.println(spent(budget))?",
        ),
        &[],
        "600 tokens, 2 calls left\n850 tokens, 2 calls left\n\
         150 tokens, 1 calls spent\n150 tokens, 1 calls spent\n",
    );
}

#[test]
fn copies_of_a_budget_draw_from_the_same_counters() {
    assert_runs(
        &program(
            "    let small = budget.limit(1000, 3)
    let copy = small
    match copy.reserve(100) {
        Ok(r) => r.debit(100, 0, 0, 0)
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(small))?
    io.println(spent(small))?",
        ),
        &[],
        "900 tokens, 2 calls left\n100 tokens, 1 calls spent\n",
    );
}

#[test]
fn a_reservation_must_fit_what_every_ancestor_has_left() {
    assert_runs(
        &program(
            "    let parent = budget.limit(1000, 10)
    let first = parent.limit(800, 5)
    let second = parent.limit(800, 5)
    match first.reserve(700) {
        Ok(r) => r.debit(700, 0, 0, 0)
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(second))?
    match second.reserve(500) {
        Ok(_) => io.println(\"reserved\")?
        Err(e) => io.println(e.to_string())?
    }",
        ),
        &[],
        "300 tokens, 5 calls left\n\
         over budget: asked for 500 tokens and 1 call, with 300 tokens and 5 calls left\n",
    );
}

#[test]
fn the_last_call_may_overshoot_by_what_its_reply_used_beyond_the_reservation() {
    assert_runs(
        &program(
            "    let small = budget.limit(1000, 5)
    match small.reserve(1000) {
        Ok(r) => r.debit(1200, 0, 0, 0)
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(small))?
    io.println(spent(small))?
    match small.reserve(0) {
        Ok(_) => io.println(\"reserved\")?
        Err(e) => io.println(e.to_string())?
    }",
        ),
        &[],
        "0 tokens, 4 calls left\n1200 tokens, 1 calls spent\n\
         over budget: asked for 0 tokens and 1 call, with 0 tokens and 4 calls left\n",
    );
}

#[test]
fn a_released_reservation_spends_nothing_and_settles_only_once() {
    assert_runs(
        &program(
            "    let small = budget.limit(1000, 3)
    match small.reserve(400) {
        Ok(r) => {
            r.release()
            r.release()
            r.debit(300, 0, 0, 0)
        }
        Err(e) => io.println(e.to_string())?
    }
    io.println(left(small))?
    io.println(spent(small))?",
        ),
        &[],
        "1000 tokens, 3 calls left\n0 tokens, 0 calls spent\n",
    );
}

#[test]
fn a_negative_amount_counts_as_zero() {
    assert_runs(
        &program(
            "    let none = budget.limit(-5, -1)
    io.println(left(none))?
    match none.reserve(-10) {
        Ok(_) => io.println(\"reserved\")?
        Err(e) => io.println(e.to_string())?
    }",
        ),
        &[],
        "0 tokens, 0 calls left\n\
         over budget: asked for 0 tokens and 1 call, with 0 tokens and 0 calls left\n",
    );
}

#[test]
fn dbg_shows_a_budget_and_a_reservation_without_their_counters() {
    let outcome = common::run_fake(
        &program(
            "    let _ = dbg(budget)
    match budget.reserve(10) {
        Ok(r) => {
            let _ = dbg(r)
        }
        Err(e) => io.println(e.to_string())?
    }",
        ),
        &[],
    );
    assert_eq!(
        outcome.stderr, "[dbg] <Budget>\n[dbg] <reservation>\n",
        "{outcome:#?}"
    );
}

#[test]
fn t_budget_has_fixed_limits_and_a_readable_spent_count() {
    let report = test_fake(&format!(
        "{HELPERS}
fn ask(budget: Budget, tokens: Int) -> Result<(), OverBudget> {{
    let r = budget.reserve(tokens)?
    r.debit(tokens, 0, 0, 0)
    Ok(())
}}

test \"spends from a fixed budget\" {{
    t.assert_eq(left(t.budget), \"100000 tokens, 100 calls left\")
    t.assert(ask(t.budget, 2500).is_ok())
    t.assert_eq(t.budget.spent().tokens, 2500)
    t.assert_eq(t.budget.spent().calls, 1)
}}

test \"stops when over budget\" {{
    let one = t.budget.limit(100, 1)
    t.assert(ask(one, 50).is_ok())
    t.assert(!ask(one, 10).is_ok())
    t.assert_eq(t.budget.spent().calls, 1)
}}
"
    ));
    let statuses: Vec<TestStatus> = report.tests.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [TestStatus::Passed, TestStatus::Passed],
        "{report:#?}"
    );
}

/// Attack: a reservation of no tokens on a budget with none left must not
/// let a reply spend past the limit again and again.
#[test]
fn a_budget_with_no_tokens_left_refuses_even_a_reservation_of_none() {
    assert_runs(
        &program(
            "    let small = budget.limit(100, 5)
    for _ in 0..5 {
        match small.reserve(0) {
            Ok(r) => r.debit(1000, 0, 0, 0)
            Err(_) => {}
        }
    }
    io.println(spent(small))?",
        ),
        &[],
        "1000 tokens, 1 calls spent\n",
    );
}

#[test]
fn the_unlimited_root_takes_reservations_up_to_the_largest_int_and_returns_them() {
    assert_runs(
        &program(
            "    let most = 9223372036854775807
    for _ in 0..3 {
        match budget.reserve(most) {
            Ok(r) => r.release()
            Err(e) => io.println(e.to_string())?
        }
    }
    var held = List.new()
    for _ in 0..3 {
        match budget.reserve(most) {
            Ok(r) => held.push(r)
            Err(e) => io.println(e.to_string())?
        }
    }
    for r in held {
        r.debit(most, 0, 0, 0)
    }
    io.println(left(budget))?
    io.println(spent(budget))?
    io.println(left(budget.limit(10, 1)))?",
        ),
        &[],
        "unlimited tokens, unlimited calls left\n\
         9223372036854775807 tokens, 3 calls spent\n\
         10 tokens, 1 calls left\n",
    );
}

#[test]
fn a_debit_counts_input_output_and_cache_tokens() {
    assert_runs(
        &program(
            "    let small = budget.limit(1000, 3)
    match small.reserve(300) {
        Ok(r) => r.debit(120, 80, 40, 10)
        Err(e) => io.println(e.to_string())?
    }
    io.println(spent(small))?
    io.println(left(small))?",
        ),
        &[],
        "250 tokens, 1 calls spent\n750 tokens, 2 calls left\n",
    );
}
