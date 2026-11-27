// SPDX-License-Identifier: Apache-2.0
//! A loop of comparisons under `--max-steps` (spec #660, ticket #664; ADR
//! 0021 and 0034): `<` and `cmp` on text, `Bool` and integers count the
//! same steps, and stop at the same step with the same output, on the
//! interpreter and natively.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

const ORD_LOOP: &str = r#"fn rank(o: Ordering) -> Int {
    match o {
        Ordering.Less => 0
        Ordering.Equal => 1
        Ordering.Greater => 2
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let words = ["pear", "apple", "fig", "", "apples", "éclair", "zebra"]
    var total = 0
    for a in words {
        for b in words {
            if a < b {
                total = total + 1
            }
            if a >= b {
                total = total + 10
            }
            total = total + rank(a.cmp(b)) * 100
            total = total + rank((a.len()).cmp(b.len()))
            total = total + rank((a < b).cmp(b < a))
        }
    }
    io.println("{total}")?
    Ok(())
}
"#;

/// The fewest steps the program takes: the smallest limit under which it
/// runs to its end, found on the interpreter.
fn steps_taken() -> u64 {
    let dir = program("ord_steps_count", ORD_LOOP);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let out = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        out.status.code() == Some(0)
    };
    let (mut low, mut high) = (1u64, 1u64 << 20);
    assert!(ends(high), "the loop ends under {high} steps");
    while low < high {
        let mid = (low + high) / 2;
        if ends(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

#[test]
fn a_loop_of_comparisons_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken();
    assert!(steps > 100, "the loop takes real work: {steps} steps");
    for limit in [1, steps / 7, steps / 2, steps - 1, steps, steps + 1] {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("ord_steps_{limit}"), ORD_LOOP).with_run_flags(&[&flag]);
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
