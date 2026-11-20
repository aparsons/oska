//! Bitwise operators, shifts and rotations on `U8`, `U32` and `U64`
//! (ticket #220), run on both backends through the differential harness:
//! every operator and rotation on each type at the edges (0, 1, the
//! maximum, the width less 1, and a mixed bit pattern), against values
//! computed here, and `P0008` for shift and rotation amounts of the
//! width, past it, and below 0.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, in_parallel, Case};

/// Each type with the bitwise operators: its name and width.
const TYPES: [(&str, u32); 3] = [("U8", 8), ("U32", 32), ("U64", 64)];

/// Every bit of a `bits`-wide integer set.
fn mask(bits: u32) -> u128 {
    (1u128 << bits) - 1
}

/// The edge values of a `bits`-wide type: 0, 1, the maximum, the width
/// less 1, and a pattern of mixed bits.
fn edges(bits: u32) -> Vec<u128> {
    let pattern = 0xA5C3_0F96_5A3C_F069u128 & mask(bits);
    vec![0, 1, mask(bits), u128::from(bits - 1), pattern]
}

fn rotl(x: u128, n: u32, bits: u32) -> u128 {
    ((x << n) | (x >> ((bits - n) % bits))) & mask(bits)
}

fn rotr(x: u128, n: u32, bits: u32) -> u128 {
    ((x >> n) | (x << ((bits - n) % bits))) & mask(bits)
}

/// A program whose `main` checks each `(expression, expected)` in
/// `checks`, over the values `values` bound as `v0`, `v1`, ... of type
/// `ty`, printing `bad <expression>` for each that does not hold, then
/// `done`.
fn checking(ty: &str, values: &[u128], checks: &[(String, u128)]) -> String {
    let mut body = String::new();
    for (i, v) in values.iter().enumerate() {
        body.push_str(&format!("    let v{i}: {ty} = {v}\n"));
    }
    for (expr, expected) in checks {
        body.push_str(&format!(
            "    let want: {ty} = {expected}\n    \
             if ({expr}) != want {{\n        let _ = io.println(\"bad {expr}\")\n    }}\n"
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
fn and_or_xor_and_not_compute_what_the_interpreter_does_at_every_edge() {
    in_parallel(&TYPES, |&(ty, bits)| {
        let values = edges(bits);
        let mut checks = Vec::new();
        for (i, a) in values.iter().enumerate() {
            checks.push((format!("~v{i}"), !a & mask(bits)));
            for (j, b) in values.iter().enumerate() {
                checks.push((format!("v{i} & v{j}"), a & b));
                checks.push((format!("v{i} | v{j}"), a | b));
                checks.push((format!("v{i} ^ v{j}"), a ^ b));
            }
        }
        // A literal operand takes the other's type, either side.
        checks.push(("1 | v0".to_string(), 1));
        checks.push(("v2 & 1".to_string(), 1));
        checks.push(("~v0 ^ v2".to_string(), 0));
        checks.push(("v4 ^ v4 ^ v4".to_string(), values[4]));
        assert_all_hold(&format!("bitwise_{ty}"), &checking(ty, &values, &checks));
    });
}

#[test]
fn shifts_compute_what_the_interpreter_does_at_every_edge() {
    in_parallel(&TYPES, |&(ty, bits)| {
        let values = edges(bits);
        let mut checks = Vec::new();
        for (i, a) in values.iter().enumerate() {
            for n in [0, 1, bits / 2, bits - 1] {
                checks.push((format!("v{i} << {n}"), (a << n) & mask(bits)));
                checks.push((format!("v{i} >> {n}"), a >> n));
            }
            // An amount held in a value, not a literal.
            checks.push((format!("v{i} << v3"), (a << (bits - 1)) & mask(bits)));
            checks.push((format!("v{i} >> v3"), a >> (bits - 1)));
            checks.push((format!("v{i} << v1 << v1"), (a << 2) & mask(bits)));
        }
        assert_all_hold(&format!("shifts_{ty}"), &checking(ty, &values, &checks));
    });
}

#[test]
fn rotations_compute_what_the_interpreter_does_at_every_edge() {
    in_parallel(&TYPES, |&(ty, bits)| {
        let values = edges(bits);
        let mut checks = Vec::new();
        for (i, a) in values.iter().enumerate() {
            for n in [0, 1, 3, bits / 2, bits - 1] {
                checks.push((format!("v{i}.rotl({n})"), rotl(*a, n, bits)));
                checks.push((format!("v{i}.rotr({n})"), rotr(*a, n, bits)));
            }
            checks.push((format!("v{i}.rotl(5).rotr(5)"), *a));
        }
        assert_all_hold(&format!("rotations_{ty}"), &checking(ty, &values, &checks));
    });
}

#[test]
fn rotations_work_as_function_values_and_in_closures() {
    let src = "fn apply(f: fn(U32, Int) -> U32, x: U32, n: Int) -> U32 {\n    f(x, n)\n}\n\n\
               fn main(io: Stdout) {\n    \
               let left = U32.rotl\n    \
               let x: U32 = 2147483649\n    \
               if apply(left, x, 1) == 3 {\n        let _ = io.println(\"value\")\n    }\n    \
               let right = |v: U32| v.rotr(1)\n    \
               if right(3) == 2147483649 {\n        let _ = io.println(\"closure\")\n    }\n    \
               let flip = |v: U8| ~v & 15\n    \
               if flip(5) == 10 {\n        let _ = io.println(\"not\")\n    }\n    \
               let _ = apply(left, x, 32)\n}\n";
    let outcome = assert_same(&Case::file("rotation_values", src));
    assert_eq!(outcome.stdout, "value\nclosure\nnot\n", "{outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains("error[P0008]: `rotl` by 32: the amount must be from 0 to 31 for `U32`"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}

#[test]
fn a_shift_or_rotation_amount_out_of_range_panics_with_p0008() {
    let mut all = Vec::new();
    for (ty, bits) in TYPES {
        let max = mask(bits);
        let top = bits - 1;
        for (what, expr, shown) in [
            (
                "shl_width",
                format!("x << {bits}"),
                format!("`<<` by {bits}"),
            ),
            (
                "shr_width",
                format!("x >> {bits}"),
                format!("`>>` by {bits}"),
            ),
            ("shl_max", format!("x << {max}"), format!("`<<` by {max}")),
            ("shr_held", "x >> big".to_string(), format!("`>>` by {max}")),
            (
                "rotl_width",
                format!("x.rotl({bits})"),
                format!("`rotl` by {bits}"),
            ),
            (
                "rotr_past",
                format!("x.rotr({})", bits + 1),
                format!("`rotr` by {}", bits + 1),
            ),
            (
                "rotl_negative",
                "x.rotl(-1)".to_string(),
                "`rotl` by -1".to_string(),
            ),
            (
                "rotr_most_negative",
                "x.rotr(-9223372036854775807 - 1)".to_string(),
                "`rotr` by -9223372036854775808".to_string(),
            ),
        ] {
            let message =
                format!("error[P0008]: {shown}: the amount must be from 0 to {top} for `{ty}`");
            all.push((ty, what, expr, message, max));
        }
    }
    in_parallel(&all, |(ty, what, expr, message, max)| {
        let src = format!(
            "fn main(io: Stdout) {{\n    let x: {ty} = 1\n    let big: {ty} = {max}\n    \
             let _ = io.println(\"before\")\n    let r = {expr}\n    \
             let _ = io.println(\"after\")\n}}\n"
        );
        let outcome = assert_same(&Case::file(&format!("p0008_{ty}_{what}"), &src));
        assert_eq!(outcome.stdout, "before\n", "`{ty}` {what}: {outcome:#?}");
        assert!(
            outcome.stderr.contains(message.as_str()),
            "`{ty}` {what}: {outcome:#?}, expected {message}"
        );
        assert_eq!(outcome.exit_code, Some(101), "`{ty}` {what}: {outcome:#?}");
    });
}
