// SPDX-License-Identifier: Apache-2.0
//! A float-heavy loop of exact-tier calls under `--max-steps` (spec #590,
//! ticket #606; ADR 0021): the `std.math` `_exact` functions are Oska, so each
//! call and each loop back-edge inside them, the fast path's and the slow
//! path's, is a step, and a loop that calls them stops at the same step, with
//! the same output, on the interpreter and natively.
//!
//! The loop includes one hard-to-round input for every function (the first
//! row of its `<name>_hard.txt` in `oska-run`'s `tests/float_reference/`), so
//! the slow path of each, a long loop of fixed-point or integer arithmetic,
//! is counted too.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Calls every exact function of `std.math` for a few arguments and for one
/// hard case each, and prints the bits of what they sum to, so a result that
/// differs between backends shows.
const EXACT_LOOP: &str = r#"use std.math

fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0.0
    for i in 1..3 {
        let x = i.to_float() * 0.75
        total = total + math.exp_exact(x) + math.ln_exact(x) + math.sin_exact(x) + math.cos_exact(x)
        total = total + math.atan2_exact(x, 2.0) + math.hypot_exact(x, 3.0) + math.powf_exact(x, 1.5)
    }
    total = total + math.hypot_exact(Float.from_bits(4842270319348757301), Float.from_bits(4844071759199705500))
    total = total + math.exp_exact(Float.from_bits(13870451010117120503))
    total = total + math.ln_exact(Float.from_bits(8151573746558919786))
    total = total + math.sin_exact(Float.from_bits(4699340879621517046))
    total = total + math.cos_exact(Float.from_bits(4698713005960405305))
    total = total + math.atan2_exact(Float.from_bits(4703767871042243225), Float.from_bits(4751908070650882394))
    total = total + math.powf_exact(Float.from_bits(4613937818241073152), Float.from_bits(4629981891913580544))
    let bits = total.to_bits().to_int()
    io.println("{bits}")?
    Ok(())
}
"#;

/// The fewest steps the program takes: the smallest limit under which it
/// runs to its end, found on the interpreter.
fn steps_taken() -> u64 {
    let dir = program("float_exact_steps_count", EXACT_LOOP);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let out = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        out.status.code() == Some(0)
    };
    let (mut low, mut high) = (1u64, 1u64 << 22);
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
fn a_loop_of_exact_calls_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken();
    assert!(
        steps > 100_000,
        "the loop takes the slow paths too: {steps} steps"
    );
    for limit in [1, steps / 7, steps / 2, steps - 1, steps, steps + 1] {
        let flag = format!("--max-steps={limit}");
        let case =
            Case::file(&format!("float_exact_steps_{limit}"), EXACT_LOOP).with_run_flags(&[&flag]);
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
