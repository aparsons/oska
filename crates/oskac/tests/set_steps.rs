// SPDX-License-Identifier: Apache-2.0
//! `Set` under `--max-steps` (spec #798, ticket #802, ADR 0021 and
//! ADR 0032): a set is written in Oska over `Map`, so each call and probe
//! is a step, and both backends count them alike. A set-heavy program
//! stops at the same step, with the same output and report, on the
//! interpreter and in native code.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Adds, repeats, removes and reads 150 items: the table grows, probes and
/// compacts on the way.
const SET_HEAVY: &str = r#"fn main(io: Stdout) {
    var s: Set<String> = Set.new()
    var i = 0
    while i < 150 {
        let _a = s.add("item{i}")
        i = i + 1
    }
    i = 0
    while i < 150 {
        let _a = s.add("item{i}")
        i = i + 3
    }
    i = 0
    while i < 150 {
        let _gone = s.remove("item{i}")
        i = i + 5
    }
    var found = 0
    i = 0
    while i < 150 {
        if s.contains("item{i}") {
            found = found + 1
        }
        i = i + 1
    }
    let n = s.len()
    let _ = io.println("{n} {found}")
}
"#;

/// The steps `src` takes on the interpreter: the least `--max-steps` under
/// which it ends in its own way.
fn steps_taken(name: &str, src: &str) -> u64 {
    let dir = program(name, src);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let output = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        !String::from_utf8_lossy(&output.stderr).contains("error[P0038]")
    };
    let (mut low, mut high) = (1, 1);
    while !ends(high) {
        low = high;
        high *= 2;
    }
    while low + 1 < high {
        let middle = (low + high) / 2;
        if ends(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    if ends(low) {
        low
    } else {
        high
    }
}

#[test]
fn a_set_heavy_program_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken("set_steps_total", SET_HEAVY);
    assert!(
        steps > 1_000,
        "a set-heavy program takes many steps: {steps}"
    );
    let mut limits = vec![1, 2, 3, steps / 97, steps / 31, steps / 7, steps / 3];
    limits.extend([steps / 2, steps * 2 / 3, steps - 2, steps - 1, steps]);
    limits.retain(|&limit| limit >= 1);
    limits.sort_unstable();
    limits.dedup();
    for limit in limits {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("set_steps_{limit}"), SET_HEAVY).with_run_flags(&[&flag]);
        let outcome = assert_same(&case);
        if limit < steps {
            assert_eq!(outcome.exit_code, Some(101), "{flag}: {outcome:#?}");
            assert!(
                outcome.stderr.contains("error[P0038]"),
                "{flag}: {outcome:#?}"
            );
        } else {
            assert_eq!(outcome.exit_code, Some(0), "{flag}: {outcome:#?}");
        }
    }
}
