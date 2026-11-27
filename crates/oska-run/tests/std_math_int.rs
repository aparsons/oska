// SPDX-License-Identifier: Apache-2.0
//! The integer helpers of `std.math` (spec #590, ticket #595): `gcd`,
//! `isqrt`, `pow`, `floor_div` and `rem_euclid`. Each is checked against
//! Rust's exact integer arithmetic (`i128` where `i64` could overflow), on
//! the interpreter; the `assert_runs` cases also run natively. A
//! broken `requires` is `P0006`, an overflow is `P0001`, and a loop counts
//! steps (ADR 0021), so `--max-steps` stops one with `P0038`.

mod common;

use common::{assert_panics, assert_runs, run_fake, PROGRAM_PATH};
use oska_run::{run_with_limits, FakeHost, Limits};

const MIN: i64 = i64::MIN;
const MAX: i64 = i64::MAX;

/// Values that take each helper to its edges.
const EDGES: [i64; 14] = [
    MIN,
    MIN + 1,
    -1_000_000_007,
    -12,
    -7,
    -3,
    -1,
    0,
    1,
    2,
    6,
    35,
    4_294_967_296,
    MAX,
];

fn lit(n: i64) -> String {
    if n == MIN {
        "(-9223372036854775807 - 1)".to_string()
    } else if n < 0 {
        format!("({n})")
    } else {
        n.to_string()
    }
}

/// A program that prints `call` once for every pair of `pairs`, `call`
/// being an expression of `a` and `b`.
fn program(pairs: &[(i64, i64)], call: &str) -> String {
    let xs: Vec<String> = pairs.iter().map(|&(a, _)| lit(a)).collect();
    let ys: Vec<String> = pairs.iter().map(|&(_, b)| lit(b)).collect();
    format!(
        "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let xs: List<Int> = [{}]\n    let ys: List<Int> = [{}]\n    for i in 0..xs.len() {{\n        let a = xs.at(i)\n        let b = ys.at(i)\n        let r = {call}\n        io.println(\"{{r}}\")?\n    }}\n    Ok(())\n}}\n",
        xs.join(", "),
        ys.join(", ")
    )
}

fn check(pairs: &[(i64, i64)], call: &str, f: impl Fn(i64, i64) -> i64) {
    let expected: String = pairs
        .iter()
        .map(|&(a, b)| format!("{}\n", f(a, b)))
        .collect();
    assert_runs(&program(pairs, call), &[], &expected);
}

fn one(call: &str) -> String {
    format!("use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let r = {call}\n    io.println(\"{{r}}\")?\n    Ok(())\n}}\n")
}

fn grid(non_negative: bool, nonzero_b: bool) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for &a in &EDGES {
        for &b in &EDGES {
            if (non_negative && (a < 0 || b < 0)) || (nonzero_b && b == 0) {
                continue;
            }
            out.push((a, b));
        }
    }
    out
}

fn euclid(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[test]
fn gcd_is_the_greatest_common_divisor_on_the_edges() {
    let mut pairs = grid(true, false);
    pairs.extend([
        (48, 18),
        (17, 5),
        (0, 0),
        (MAX, MAX - 1),
        (7_540_113_804_746_346_429, 4_660_046_610_375_530_309),
    ]);
    check(&pairs, "math.gcd(a, b)", euclid);
}

#[test]
fn isqrt_is_exact_for_every_small_value_and_around_each_square() {
    let n = 2000;
    let src = format!(
        "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    for n in 0..{n} {{\n        let r = math.isqrt(n)\n        io.println(\"{{r}}\")?\n    }}\n    Ok(())\n}}\n"
    );
    let expected: String = (0..n as i64)
        .map(|n| {
            format!(
                "{}\n",
                (0..)
                    .take_while(|r| r * r <= n)
                    .last()
                    .expect("0 is a root")
            )
        })
        .collect();
    assert_runs(&src, &[], &expected);
    let big: Vec<(i64, i64)> = [
        MAX,
        MAX - 1,
        3_037_000_499 * 3_037_000_499,
        3_037_000_499 * 3_037_000_499 - 1,
        3_037_000_499 * 3_037_000_499 + 1,
        4_611_686_018_427_387_904,
        4_611_686_018_427_387_903,
        4_611_686_014_132_420_609,
        1 << 40,
        (1 << 40) - 1,
    ]
    .iter()
    .map(|&n| (n, 0))
    .collect();
    check(&big, "math.isqrt(a)", |a, _| {
        // Newton from above, in integers.
        let mut r = a as i128;
        while r * r > a as i128 {
            r = (r + a as i128 / r) / 2;
        }
        while (r + 1) * (r + 1) <= a as i128 {
            r += 1;
        }
        r as i64
    });
}

#[test]
fn pow_is_exact_when_it_fits() {
    let mut pairs = Vec::new();
    for base in [-3, -2, -1, 0, 1, 2, 3, 10, 3_037_000_499, MAX, MIN] {
        for exp in [0, 1, 2, 3, 5, 40, 62, 63, 64] {
            let fits = (base as i128)
                .checked_pow(exp as u32)
                .is_some_and(|v| i64::try_from(v).is_ok());
            if fits {
                pairs.push((base, exp));
            }
        }
    }
    assert!(pairs.len() > 40);
    check(&pairs, "math.pow(a, b)", |a, b| a.pow(b as u32));
    // A result in range is not lost to a square no result needs.
    check(
        &[(MIN, 1), (2, 62), (-2, 63), (3, 39)],
        "math.pow(a, b)",
        |a, b| a.pow(b as u32),
    );
}

#[test]
fn pow_overflow_panics_with_p0001_and_never_wraps() {
    for call in [
        "math.pow(2, 63)",
        "math.pow(2, 64)",
        "math.pow(10, 19)",
        "math.pow(3, 40)",
        "math.pow(-2, 64)",
        "math.pow(9223372036854775807, 2)",
    ] {
        let src = one(call);
        let outcome = run_fake(&src, &[]);
        let panic = outcome
            .panic
            .unwrap_or_else(|| panic!("{call} should panic"));
        assert_eq!(panic.code, "P0001", "{call}");
        assert_eq!(outcome.stdout, "", "{call}");
    }
}

#[test]
fn floor_div_rounds_toward_negative_infinity() {
    let mut pairs = grid(false, true);
    pairs.retain(|&(a, b)| !(a == MIN && b == -1));
    pairs.extend([(7, 2), (-7, 2), (7, -2), (-7, -2), (6, 3), (-6, 3)]);
    check(&pairs, "math.floor_div(a, b)", |a, b| {
        let (a, b) = (a as i128, b as i128);
        let q = a / b;
        (if a % b != 0 && ((a < 0) != (b < 0)) {
            q - 1
        } else {
            q
        }) as i64
    });
}

#[test]
fn rem_euclid_is_never_negative() {
    let mut pairs = grid(false, true);
    pairs.retain(|&(a, b)| !(a == MIN && b == -1));
    pairs.extend([(7, 2), (-7, 2), (7, -2), (-7, -2), (-6, 3)]);
    check(&pairs, "math.rem_euclid(a, b)", |a, b| a.rem_euclid(b));
}

#[test]
fn a_broken_precondition_is_p0006_spanning_the_call() {
    for call in [
        "math.gcd(-1, 5)",
        "math.gcd(5, -1)",
        "math.isqrt(-1)",
        "math.pow(2, -1)",
        "math.floor_div(5, 0)",
        "math.rem_euclid(5, 0)",
    ] {
        assert_panics(&one(call), "P0006", call);
    }
}

#[test]
fn the_one_quotient_that_overflows_panics_with_p0001() {
    let call = "math.floor_div(-9223372036854775807 - 1, -1)";
    let outcome = run_fake(&one(call), &[]);
    assert_eq!(outcome.panic.expect("a panic").code, "P0001", "{call}");
}

#[test]
fn a_loop_in_a_helper_counts_steps() {
    let src = one("math.gcd(832040, 514229)");
    let run = |max_steps| {
        run_with_limits(
            &src,
            &mut FakeHost::new(),
            &[PROGRAM_PATH.to_string()],
            Limits {
                max_steps: Some(max_steps),
                ..Limits::NONE
            },
        )
    };
    assert_eq!(run(10_000).stdout, "1\n");
    let stopped = run(5);
    assert_eq!(stopped.panic.expect("the limit stops it").code, "P0038");
}
