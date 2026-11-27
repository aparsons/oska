// SPDX-License-Identifier: Apache-2.0
//! `math.isqrt` and `math.pow` under `--max-steps` (spec #590, follow-up
//! #796; ADR 0021): their loops are Oska, so each back-edge is a step, and
//! a program that calls them stops at the same step, with the same output,
//! on the interpreter and natively. `gcd` is in `std_math_int.rs`.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

const INT_LOOP: &str = r#"use std.math

fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0
    for i in 1..6 {
        total = total + math.isqrt(i * 1_000_003) + math.pow(i, 9) % 1_000
    }
    total = total + math.isqrt(9223372036854775807)
    io.println("{total}")?
    Ok(())
}
"#;

/// The fewest steps the program takes: the smallest limit under which it
/// runs to its end, found on the interpreter.
fn steps_taken() -> u64 {
    let dir = program("math_int_steps_count", INT_LOOP);
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
fn isqrt_and_pow_stop_at_the_same_step_on_both_backends() {
    let steps = steps_taken();
    assert!(steps > 100, "the loop takes real work: {steps} steps");
    for limit in [1, steps / 7, steps / 2, steps - 1, steps, steps + 1] {
        let flag = format!("--max-steps={limit}");
        let case =
            Case::file(&format!("math_int_steps_{limit}"), INT_LOOP).with_run_flags(&[&flag]);
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
