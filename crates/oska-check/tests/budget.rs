// SPDX-License-Identifier: Apache-2.0
//! The `Budget` capability (#251) in the checker: `main` may declare it, a
//! function that takes one has it in its authority, `limit` is an
//! attenuation in the report, and neither a budget nor a reservation can
//! be made by hand.

mod common;

use common::{assert_authority, assert_ok, check};
use oska_check::AttenuationRoot;

const SPENDER: &str = "fn spend(budget: Budget) -> Bool {
    match budget.reserve(100) {
        Ok(r) => {
            r.debit(80, 0, 0, 0)
            true
        }
        Err(_) => false
    }
}

fn main(budget: Budget) {
    let _ = spend(budget.limit(1000, 2))
}
";

#[test]
fn main_may_declare_a_budget() {
    assert_ok(SPENDER);
}

#[test]
fn a_function_that_takes_a_budget_has_it_in_its_authority() {
    assert_authority(SPENDER, "spend", &["Budget"], false);
}

#[test]
fn limit_is_an_attenuation_in_the_report() {
    let report = check(SPENDER);
    let found: Vec<(String, String, String, String, AttenuationRoot, &str)> = report
        .attenuations
        .into_iter()
        .map(|a| {
            (
                a.function.to_string(),
                a.method,
                a.source,
                a.result,
                a.root,
                &SPENDER[a.span.start..a.span.end],
            )
        })
        .collect();
    assert_eq!(
        found,
        [(
            "main".to_string(),
            "limit".to_string(),
            "Budget".to_string(),
            "Budget".to_string(),
            AttenuationRoot::Absent,
            "budget.limit(1000, 2)",
        )]
    );
}

#[test]
fn a_budget_cannot_be_made_by_hand() {
    let report = check("fn main() {\n    let _ = Budget(handle: 0)\n}\n");
    assert!(!report.diagnostics.is_empty(), "{report:#?}");
}

#[test]
fn a_reservation_cannot_be_made_by_hand() {
    let report = check(
        "fn main(budget: Budget) {\n    let r = Reservation(handle: 0)\n    r.release()\n}\n",
    );
    assert!(!report.diagnostics.is_empty(), "{report:#?}");
}
