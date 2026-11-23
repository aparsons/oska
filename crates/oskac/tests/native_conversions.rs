// SPDX-License-Identifier: Apache-2.0
//! Wrapping arithmetic and integer conversions (ticket #221), run on both
//! backends through the differential harness: `wrapping_add`,
//! `wrapping_sub` and `wrapping_mul` on `U8`, `U32` and `U64`, and
//! `to_<type>()` and `wrapping_to_<type>()` between `Int`, `U8`, `U16`,
//! `U32` and `U64`, at the edges (0, 1, each type's maximum and the
//! values just past it, -1 and `Int`'s minimum), against values computed
//! here, and `P0001` for each conversion that does not fit.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, in_parallel, Case};

/// An integer type with the conversions: its name, the suffix of the
/// method that converts to it, and its range.
#[derive(Clone, Copy)]
struct Ty {
    name: &'static str,
    suffix: &'static str,
    min: i128,
    max: i128,
}

const INT: Ty = Ty {
    name: "Int",
    suffix: "int",
    min: i64::MIN as i128,
    max: i64::MAX as i128,
};
const U8: Ty = unsigned("U8", "u8", 8);
const U16: Ty = unsigned("U16", "u16", 16);
const U32: Ty = unsigned("U32", "u32", 32);
const U64: Ty = unsigned("U64", "u64", 64);

const fn unsigned(name: &'static str, suffix: &'static str, bits: u32) -> Ty {
    Ty {
        name,
        suffix,
        min: 0,
        max: (1i128 << bits) - 1,
    }
}

const CONVERTIBLE: [Ty; 5] = [INT, U8, U16, U32, U64];

/// The types with wrapping arithmetic.
const WRAPPING: [Ty; 3] = [U8, U32, U64];

impl Ty {
    fn fits(self, n: i128) -> bool {
        (self.min..=self.max).contains(&n)
    }

    /// `n` modulo 2 to the type's width: its low bits.
    fn wrap(self, n: i128) -> i128 {
        n.rem_euclid(self.max + 1)
    }

    /// The type's edge values: 0, 1, -1, the maximum and minimum of every
    /// type, and one past each maximum, those that fit this type.
    fn edges(self) -> Vec<i128> {
        let mut values = vec![0, 1, -1, INT.min];
        for ty in CONVERTIBLE {
            values.push(ty.max);
            values.push(ty.max + 1);
            values.push(ty.max - 1);
        }
        values.sort_unstable();
        values.dedup();
        values.retain(|&n| self.fits(n));
        values
    }
}

/// An `Int`'s literal: `Int`'s minimum has no literal of its own.
fn literal(n: i128) -> String {
    if n == INT.min {
        format!("{} - 1", INT.min + 1)
    } else {
        n.to_string()
    }
}

/// A program whose `main` checks each `(expression, type, expected)` in
/// `checks`, printing `bad <expression>` for each that does not hold,
/// then `done`. `lets` binds the values the expressions use.
fn checking(lets: &[(String, Ty, i128)], checks: &[(String, Ty, i128)]) -> String {
    let mut body = String::new();
    for (name, ty, v) in lets {
        body.push_str(&format!("    let {name}: {} = {}\n", ty.name, literal(*v)));
    }
    for (expr, ty, expected) in checks {
        body.push_str(&format!(
            "    let want: {} = {}\n    \
             if {expr} != want {{\n        let _ = io.println(\"bad {expr}\")\n    }}\n",
            ty.name,
            literal(*expected)
        ));
    }
    format!("fn main(io: Stdout) {{\n{body}    let _ = io.println(\"done\")\n}}\n")
}

fn assert_all_hold(name: &str, src: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, "done\n", "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

#[test]
fn wrapping_arithmetic_keeps_the_low_bits_at_every_edge() {
    let mut all = Vec::new();
    for ty in WRAPPING {
        for op in ["add", "sub", "mul"] {
            all.push((ty, op));
        }
    }
    in_parallel(&all, |&(ty, op)| {
        let half = ty.max / 2;
        let bits = (ty.max + 1).trailing_zeros() as i128;
        let pattern = 0xA5C3_0F96_5A3C_F069i128 & ty.max;
        let values = [
            0,
            1,
            2,
            bits - 1,
            half,
            half + 1,
            ty.max - 1,
            ty.max,
            pattern,
        ];
        let lets: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}"), ty, v))
            .collect();
        let mut checks = Vec::new();
        for (i, &a) in values.iter().enumerate() {
            for (j, &b) in values.iter().enumerate() {
                // `u128` holds the exact sum and product of two `U64`s.
                let (a, b, m) = (a as u128, b as u128, ty.max as u128 + 1);
                let expected = match op {
                    "add" => (a + b) % m,
                    "sub" => (a + m - b) % m,
                    _ => a * b % m,
                };
                checks.push((format!("v{i}.wrapping_{op}(v{j})"), ty, expected as i128));
            }
        }
        // A literal argument takes the receiver's type.
        checks.push((
            format!("v0.wrapping_{op}(1)"),
            ty,
            ty.wrap(match op {
                "add" => 1,
                "sub" => -1,
                _ => 0,
            }),
        ));
        assert_all_hold(
            &format!("wrapping_{op}_{}", ty.name),
            &checking(&lets, &checks),
        );
    });
}

#[test]
fn wrapping_arithmetic_wraps_where_plain_arithmetic_panics() {
    let src = "fn main(io: Stdout) {\n    \
               let top: U8 = 255\n    \
               let big: U64 = 18446744073709551615\n    \
               let zero: U32 = 0\n    \
               let _ = io.println(top.wrapping_add(1).to_int().to_string())\n    \
               let _ = io.println(top.wrapping_mul(top).to_int().to_string())\n    \
               let _ = io.println(big.wrapping_mul(big).to_int().to_string())\n    \
               let _ = io.println(zero.wrapping_sub(1).to_int().to_string())\n    \
               let _ = io.println((top + 1).to_int().to_string())\n}\n";
    let outcome = assert_same(&Case::file("wrapping_versus_plain", src));
    assert_eq!(outcome.stdout, "0\n1\n1\n4294967295\n", "{outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains("error[P0001]: `255 + 1` overflows `U8`"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}

#[test]
fn a_conversion_keeps_every_value_that_fits_its_target() {
    in_parallel(&CONVERTIBLE, |&from| {
        let values = from.edges();
        let lets: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}"), from, v))
            .collect();
        let mut checks = Vec::new();
        for to in CONVERTIBLE {
            if to.name == from.name {
                continue;
            }
            for (i, &v) in values.iter().enumerate() {
                if to.fits(v) {
                    checks.push((format!("v{i}.to_{}()", to.suffix), to, v));
                }
            }
        }
        assert_all_hold(&format!("to_from_{}", from.name), &checking(&lets, &checks));
    });
}

#[test]
fn a_wrapping_conversion_keeps_the_low_bits() {
    let narrowing = [
        (U16, U8),
        (U32, U8),
        (U32, U16),
        (U64, U8),
        (U64, U16),
        (U64, U32),
    ];
    in_parallel(&narrowing, |&(from, to)| {
        let values = from.edges();
        let mut lets: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}"), from, v))
            .collect();
        let pattern = 0xA5C3_0F96_5A3C_F069i128 & from.max;
        lets.push(("p".to_string(), from, pattern));
        let mut checks: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}.wrapping_to_{}()", to.suffix), to, to.wrap(v)))
            .collect();
        checks.push((
            format!("p.wrapping_to_{}()", to.suffix),
            to,
            to.wrap(pattern),
        ));
        assert_all_hold(
            &format!("wrapping_to_{}_{}", from.name, to.name),
            &checking(&lets, &checks),
        );
    });
}

#[test]
fn a_conversion_that_does_not_fit_panics_with_p0001() {
    let mut all = Vec::new();
    for from in CONVERTIBLE {
        for to in CONVERTIBLE {
            if to.name == from.name {
                continue;
            }
            for v in from.edges() {
                // The nearest values on either side of the target's range.
                let nearest = v == to.max + 1 || v == -1 || (v == from.max && v > to.max);
                if !to.fits(v) && nearest {
                    all.push((from, to, v));
                }
            }
        }
    }
    assert!(all.len() >= 12, "{} cases", all.len());
    in_parallel(&all, |&(from, to, v)| {
        let src = format!(
            "fn main(io: Stdout) {{\n    let x: {} = {}\n    \
             let _ = io.println(\"before\")\n    let r = x.to_{}()\n    \
             let _ = io.println(\"after\")\n}}\n",
            from.name,
            literal(v),
            to.suffix
        );
        let name = format!("p0001_{}_to_{}_{v}", from.name, to.name);
        let outcome = assert_same(&Case::file(&name, &src));
        let message = format!(
            "error[P0001]: `to_{}()` of {v} overflows `{}`, which ranges from {} to {}",
            to.suffix, to.name, to.min, to.max
        );
        assert_eq!(outcome.stdout, "before\n", "{name}: {outcome:#?}");
        assert!(
            outcome.stderr.contains(&message),
            "{name}: {outcome:#?}, expected {message}"
        );
        assert_eq!(outcome.exit_code, Some(101), "{name}: {outcome:#?}");
    });
}

#[test]
fn conversions_and_wrapping_methods_work_as_function_values_and_in_closures() {
    let src = "fn apply(f: fn(Int) -> U8, n: Int) -> U8 {\n    f(n)\n}\n\n\
               fn main(io: Stdout) {\n    \
               let narrow = Int.to_u8\n    \
               let low = |v: U32| v.wrapping_to_u8().to_int()\n    \
               let add = U8.wrapping_add\n    \
               let _ = io.println(apply(narrow, 200).to_int().to_string())\n    \
               let _ = io.println(low(4294967041).to_string())\n    \
               let _ = io.println(add(apply(narrow, 250), 10).to_int().to_string())\n    \
               let _ = apply(narrow, 256)\n}\n";
    let outcome = assert_same(&Case::file("conversion_values", src));
    assert_eq!(outcome.stdout, "200\n1\n4\n", "{outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains("error[P0001]: `to_u8()` of 256 overflows `U8`, which ranges from 0 to 255"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}
