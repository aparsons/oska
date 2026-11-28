// SPDX-License-Identifier: Apache-2.0
//! A seeded random sort under `--max-steps` (spec #660, ticket #666; ADR
//! 0021 and 0034): `sorted_by` and `sort` count the same steps, and stop
//! at the same step with the same output, on the interpreter and natively.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

const SORT_LOOP: &str = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var seed = 12345
    var xs: List<Int> = []
    for i in 0..120 {
        seed = (seed * 1103515245 + 12345) % 2147483648
        xs.push((seed / 65536) % 50)
    }
    let ys = xs.sorted_by(|a, b| (a % 7).cmp(b % 7))
    var zs = xs
    zs.sort()
    var sum = 0
    for y in ys {
        sum = (sum * 31 + y) % 1000000007
    }
    for z in zs {
        sum = (sum * 17 + z) % 1000000007
    }
    io.println("{sum}")?
    Ok(())
}
"#;

/// The fewest steps the program takes: the smallest limit under which it
/// runs to its end, found on the interpreter.
fn steps_taken() -> u64 {
    let dir = program("sort_steps_count", SORT_LOOP);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let out = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        out.status.code() == Some(0)
    };
    let (mut low, mut high) = (1u64, 1u64 << 20);
    assert!(ends(high), "the sort ends under {high} steps");
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
fn a_seeded_sort_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken();
    assert!(steps > 100, "the sort takes real work: {steps} steps");
    for limit in [1, steps / 7, steps / 2, steps - 1, steps, steps + 1] {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("sort_steps_{limit}"), SORT_LOOP).with_run_flags(&[&flag]);
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
