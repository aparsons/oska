// SPDX-License-Identifier: Apache-2.0
//! `Deque` under `--max-steps` (spec #798, ticket #804, ADR 0021 and
//! ADR 0032): a deque is written in Oska, so each call and loop back-edge
//! counts, and both backends stop at the same step with the same output.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Pushes at both ends and pops from both: the ring grows, wraps and
/// shrinks on the way.
const DEQUE_HEAVY: &str = r#"fn main(io: Stdout) {
    var d: Deque<Int> = Deque.new()
    var i = 0
    while i < 100 {
        if i % 2 == 0 {
            d.push_front(i)
        } else {
            d.push_back(i)
        }
        i = i + 1
    }
    var sum = 0
    while d.len() > 10 {
        sum = sum + d.pop_front().unwrap_or(0)
        sum = sum - d.pop_back().unwrap_or(0)
    }
    let n = d.len()
    let first = d.get(0).unwrap_or(-1)
    let _ = io.println("{n} {sum} {first}")
}
"#;

/// A push past the item limit.
const OVER_LIMIT: &str = r#"fn main(io: Stdout) {
    var d: Deque<Int> = Deque.with_max_entries(5)
    var i = 0
    while i < 10 {
        d.push_back(i)
        i = i + 1
    }
    let n = d.len()
    let _ = io.println("{n}")
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
fn a_deque_heavy_program_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken("deque_steps_total", DEQUE_HEAVY);
    assert!(steps > 500, "a deque-heavy program takes steps: {steps}");
    let mut limits = vec![1, 2, 3, steps / 31, steps / 7, steps / 3, steps / 2];
    limits.extend([steps * 2 / 3, steps - 2, steps - 1, steps]);
    limits.retain(|&limit| limit >= 1);
    limits.sort_unstable();
    limits.dedup();
    for limit in limits {
        let flag = format!("--max-steps={limit}");
        let case =
            Case::file(&format!("deque_steps_{limit}"), DEQUE_HEAVY).with_run_flags(&[&flag]);
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

#[test]
fn a_push_past_the_limit_panics_with_p0051_the_same_on_both_backends() {
    let outcome = assert_same(&Case::file("deque_over_limit", OVER_LIMIT));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert!(
        outcome.stderr.contains("error[P0051]: "),
        "{}",
        outcome.stderr
    );
}
