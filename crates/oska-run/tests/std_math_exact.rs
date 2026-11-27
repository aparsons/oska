// SPDX-License-Identifier: Apache-2.0
//! The exact tier of `std.math` (spec #590, ticket #606; ADR 0027): `hypot_exact`,
//! `exp_exact`, `ln_exact`, `sin_exact`, `cos_exact`, `atan2_exact` and
//! `powf_exact` return the double nearest the true value, a tie going to the
//! even one.
//!
//! The reference is the one the fast tier is held to: the vectors in
//! `tests/float_reference/`, which `generate.py` writes from a 400-bit
//! computation with mpmath, never the host's libm, and (for the hard
//! cases) `generate_exact.py`, from a 700-bit one. A row's `hi` column is the
//! correctly rounded result, and the exact function must return exactly
//! those bits: no tolerance, whatever the row (the special values of C99
//! Annex F included). Every call runs on the interpreter and natively, and
//! the two must agree (the differential helpers fail on any difference).
//!
//! The hard cases, `<name>_hard.txt`, are inputs whose result lies within
//! 2^-18 ulp of a point half way between two doubles, or exactly on one (a
//! tie), so the fast path of the function (a double-double evaluation, with a
//! margin of 2^-70 of the result) cannot decide the rounding and the slow
//! path (fixed point with 256 fractional bits, or exact integers) runs. A test
//! proves it with step counts: an easy input finishes within a small step
//! limit, and every hard case stops at that limit (`P0038`), because the slow
//! path is a long loop of Oska calls.
//!
//! The fast and exact tiers differ only in the last bit, and a test records
//! one input per function where they differ.

mod common;

use common::{run_fake, PROGRAM_PATH};
use oska_run::{run_with_limits, FakeHost, Limits};
use std::fmt::Write as _;

struct Row {
    inputs: Vec<u64>,
    hi: u64,
}

fn path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/float_reference")
        .join(format!("{name}.txt"))
}

fn load(file: &str) -> Vec<Row> {
    let text = std::fs::read_to_string(path(file)).expect("the vector file");
    let hex = |s: &str| u64::from_str_radix(s, 16).expect("hex bits");
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let n = fields.len() - 4;
            Row {
                inputs: fields[..n].iter().map(|s| hex(s)).collect(),
                hi: hex(fields[n]),
            }
        })
        .collect()
}

/// The ordinary vectors of `name` and then its hard cases.
fn all_rows(name: &str) -> Vec<Row> {
    let mut rows = load(name);
    rows.extend(load(&format!("{name}_hard")));
    rows
}

const HELPERS: &str = r#"
use std.math

fn hex(bits: U64) -> String {
    var text = ""
    for i in 0..16 {
        let shift = (60 - 4 * i).to_u64()
        let nibble = ((bits >> shift) & 15).to_int()
        let digit = "0123456789abcdef".slice(nibble, nibble + 1)
        text = "{text}{digit}"
    }
    text
}
"#;

/// A program that prints, in hex, the bits of each of `calls` (`math.<name>`
/// and so on, taking the inputs of a row) on one line for each row.
fn program(calls: &[String], rows: &[Row]) -> String {
    let arity = rows[0].inputs.len();
    let mut src = String::from(HELPERS);
    src.push_str("\nfn main(io: Stdout) -> Result<(), IoError> {\n");
    for k in 0..arity {
        let items: Vec<String> = rows.iter().map(|r| r.inputs[k].to_string()).collect();
        writeln!(src, "    let in{k}: List<U64> = [{}]", items.join(", "))
            .expect("write to a String");
    }
    src.push_str("    for i in 0..in0.len() {\n");
    let args: Vec<String> = (0..arity)
        .map(|k| format!("Float.from_bits(in{k}.at(i))"))
        .collect();
    let args = args.join(", ");
    for (n, call) in calls.iter().enumerate() {
        writeln!(src, "        let r{n} = math.{call}({args})").expect("write to a String");
    }
    let shown: Vec<String> = (0..calls.len())
        .map(|n| format!("hex(r{n}.to_bits())"))
        .collect();
    if calls.len() == 1 {
        writeln!(src, "        io.println({})?", shown[0]).expect("write to a String");
    } else {
        writeln!(src, "        let a = {}", shown[0]).expect("write to a String");
        writeln!(src, "        let b = {}", shown[1]).expect("write to a String");
        src.push_str("        io.println(\"{a} {b}\")?\n");
    }
    src.push_str("    }\n    Ok(())\n}\n");
    src
}

/// The bits each of `calls` gives for each row, on both backends.
fn run_bits(calls: &[String], rows: &[Row]) -> Vec<Vec<u64>> {
    let outcome = run_fake(&program(calls, rows), &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{calls:?}: {outcome:#?}"
    );
    let got: Vec<Vec<u64>> = outcome
        .stdout
        .lines()
        .map(|l| {
            l.split(' ')
                .map(|h| u64::from_str_radix(h, 16).expect("hex"))
                .collect()
        })
        .collect();
    assert_eq!(got.len(), rows.len(), "{calls:?}: one line per row");
    got
}

fn shown(inputs: &[u64]) -> String {
    let parts: Vec<String> = inputs.iter().map(|i| format!("{i:016x}")).collect();
    parts.join(", ")
}

/// Runs every row of `name` through `math.<name>_exact` and requires the
/// reference bits.
fn check(name: &str) {
    let rows = all_rows(name);
    let got = run_bits(&[format!("{name}_exact")], &rows);
    for (n, (row, bits)) in rows.iter().zip(&got).enumerate() {
        assert_eq!(
            bits[0],
            row.hi,
            "{name}_exact({}) row {n}: want {:016x}, got {:016x}",
            shown(&row.inputs),
            row.hi,
            bits[0]
        );
    }
    println!("{name}_exact: {} rows, all correctly rounded", rows.len());
}

#[test]
fn hypot_exact_is_correctly_rounded() {
    check("hypot");
}

#[test]
fn exp_exact_is_correctly_rounded() {
    check("exp");
}

#[test]
fn ln_exact_is_correctly_rounded() {
    check("ln");
}

#[test]
fn sin_exact_is_correctly_rounded() {
    check("sin");
}

#[test]
fn cos_exact_is_correctly_rounded() {
    check("cos");
}

#[test]
fn atan2_exact_is_correctly_rounded() {
    check("atan2");
}

#[test]
fn powf_exact_is_correctly_rounded() {
    check("powf");
}

/// Whether the doubles `a` and `b` are the same, or neighbours.
fn within_one_place(a: u64, b: u64) -> bool {
    let (x, y) = (f64::from_bits(a), f64::from_bits(b));
    if x.is_nan() || y.is_nan() {
        return x.is_nan() && y.is_nan();
    }
    if x == y {
        return true;
    }
    if x.is_sign_negative() != y.is_sign_negative() {
        return false;
    }
    (a & !(1 << 63)).abs_diff(b & !(1 << 63)) <= 1
}

/// One input per function on which the fast tier is not the nearest double:
/// each is a row of the vectors (the fast function's golden bits, in the file
/// `std_math_float.rs` reads, are the fast tier's answer) and the exact tier
/// gives the reference bits, one place from the fast tier's.
const FAST_DIFFERS: [(&str, &[u64]); 7] = [
    ("hypot", &[0x479580cb395f3a68, 0x47599a31d22631d7]),
    // The fast `exp(1.0)` is 2.7182818284590455, one place off e.
    ("exp", &[0x3ff0000000000000]),
    ("ln", &[0x4008000000000000]),
    ("sin", &[0x40a1e5c2f50bf649]),
    ("cos", &[0x3fe921fb54442d17]),
    // `atan2(1.0, -5e-324)` is the double nearest pi / 2; the fast one is the next.
    ("atan2", &[0x3ff0000000000000, 0x8000000000000001]),
    // The square root of 2, to one place.
    ("powf", &[0x4000000000000000, 0x3fe0000000000000]),
];

#[test]
fn the_fast_and_exact_tiers_differ_only_in_the_last_place() {
    for (name, recorded) in FAST_DIFFERS {
        let rows = all_rows(name);
        let calls = [name.to_string(), format!("{name}_exact")];
        let got = run_bits(&calls, &rows);
        let mut differing = 0;
        for (row, bits) in rows.iter().zip(&got) {
            assert!(
                within_one_place(bits[0], bits[1]),
                "{name}({}): fast {:016x}, exact {:016x}",
                shown(&row.inputs),
                bits[0],
                bits[1]
            );
            if bits[0] != bits[1] {
                differing += 1;
            }
            if row.inputs == recorded {
                assert_ne!(
                    bits[0],
                    bits[1],
                    "{name}({}) is recorded as an input where the tiers differ",
                    shown(&row.inputs)
                );
                assert_eq!(bits[1], row.hi, "{name}: the exact tier is the reference");
            }
        }
        assert!(differing > 0, "{name}: the tiers differ on some row");
        assert!(
            rows.iter().any(|r| r.inputs == recorded),
            "{name}: the recorded input is in the vectors"
        );
        println!(
            "{name}: the tiers differ on {differing} of {} rows",
            rows.len()
        );
    }
}

/// Whether the program `src` stops at the step limit `limit` (`P0038`).
fn steps_stopped(src: &str, limit: u64) -> bool {
    let outcome = run_with_limits(
        src,
        &mut FakeHost::new(),
        &[PROGRAM_PATH.to_string()],
        Limits {
            max_steps: Some(limit),
            ..Limits::NONE
        },
    );
    match &outcome.panic {
        Some(p) => {
            assert_eq!(p.code, "P0038", "{outcome:#?}");
            true
        }
        None => {
            assert!(
                outcome.diagnostics.is_empty() && outcome.exit_code == 0,
                "{outcome:#?}"
            );
            false
        }
    }
}

/// The fewest steps under which `src` runs to its end: the smallest limit
/// that does not stop it, found by bisection.
fn steps_taken(src: &str) -> u64 {
    let (mut low, mut high) = (1u64, 1u64 << 18);
    assert!(
        !steps_stopped(src, high),
        "the call ends under {high} steps"
    );
    while low < high {
        let mid = (low + high) / 2;
        if steps_stopped(src, mid) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn one_call(name: &str, row: &Row) -> String {
    let args: Vec<String> = row
        .inputs
        .iter()
        .map(|i| format!("Float.from_bits({i})"))
        .collect();
    format!(
        "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let r = math.{name}_exact({})\n    io.println(\"{{r}}\")?\n    Ok(())\n}}\n",
        args.join(", ")
    )
}

/// The slow path is a long loop of Oska calls (fixed-point arithmetic, or
/// exact integers), so a call that takes it takes many more steps than the
/// fast path, which has no data-dependent loop: the fast path takes the
/// same number of steps for every input that reaches it.
#[test]
fn the_slow_path_runs_on_every_hard_case() {
    for name in ["hypot", "exp", "ln", "sin", "cos", "atan2", "powf"] {
        // Ordinary rows of the vectors: the fast path answers.
        let ordinary = load(name);
        let fast = ordinary[ordinary.len() - 3..]
            .iter()
            .map(|row| steps_taken(&one_call(name, row)))
            .max()
            .expect("rows");
        let hard = load(&format!("{name}_hard"));
        assert!(hard.len() >= 6, "{name}: hard cases");
        for row in &hard {
            assert!(
                steps_stopped(&one_call(name, row), fast),
                "{name}_exact({}) takes more than the fast path's {fast} steps",
                shown(&row.inputs)
            );
        }
        println!(
            "{name}_exact: the fast path takes at most {fast} steps; {} hard cases take more",
            hard.len()
        );
    }
}

fn run_ok(src: &str) -> String {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{outcome:#?}"
    );
    outcome.stdout
}

#[test]
fn sin_and_cos_exact_refuse_an_argument_past_the_reduction_limit_with_p0006() {
    for call in ["math.sin_exact(1647099.0)", "math.cos_exact(-1e22)"] {
        let src = format!(
            "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let r = {call}\n    io.println(\"{{r}}\")?\n    Ok(())\n}}\n"
        );
        common::assert_panics(&src, "P0006", call);
    }
}

#[test]
fn the_exact_functions_print_the_values_a_reader_expects() {
    let src = "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    let a = math.exp_exact(0.0)\n    let b = math.ln_exact(1.0)\n    let c = math.powf_exact(2.0, 10.0)\n    let d = math.hypot_exact(3.0, 4.0)\n    let e = math.sin_exact(0.0)\n    let f = math.cos_exact(0.0)\n    let g = math.atan2_exact(0.0, -1.0)\n    let h = math.powf_exact(-3.0, 3.0)\n    io.println(\"{a} {b} {c} {d} {e} {f} {g} {h}\")?\n    Ok(())\n}\n";
    assert_eq!(
        run_ok(src),
        "1.0 0.0 1024.0 5.0 0.0 1.0 3.141592653589793 -27.0\n"
    );
}
