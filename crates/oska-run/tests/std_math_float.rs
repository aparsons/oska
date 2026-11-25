// SPDX-License-Identifier: Apache-2.0
//! The float functions of `std.math` (spec #590, ticket #596): `abs`,
//! `floor`, `ceil`, `round_half_even`, `hypot`, `powf`, `exp`, `ln`, `sin`,
//! `cos` and `atan2`. Each is run, on the interpreter and natively (through
//! the differential helpers, which fail on any difference), over the
//! vectors in `tests/float_reference/`, which `generate.py` writes from a
//! 400-bit computation (never the host's libm). A row holds the inputs, the
//! exact result as a pair of doubles `hi + lo`, a kind, and the bits the
//! function gave when the vector was written (the golden bits).
//!
//! Two checks per row. The golden bits must come back exactly, so a change
//! to any result is a visible change. And the result must be within the
//! stored bound of the exact value: kind `x` rows (zeros, infinities, NaN,
//! the exact functions) must equal the reference bits, kind `u` rows must
//! be within `BOUND` ulps of the exact value, an ulp being the gap between
//! the doubles around it.
//!
//! To write the golden bits after an intended change, run
//! `OSKA_BLESS_FLOAT=1 cargo test -p oska-run --test std_math_float`.

mod common;

use common::{assert_panics, run_fake};
use std::fmt::Write as _;

/// The bound on the error of each function, in ulps of the exact result:
/// the algorithm's own bound, stated in `std/math.osk` and the docs. The
/// exact functions (`abs`, `floor`, `ceil`, `round_half_even`) are 0.
const BOUNDS: [(&str, f64); 11] = [
    ("abs", 0.0),
    ("floor", 0.0),
    ("ceil", 0.0),
    ("round_half_even", 0.0),
    ("hypot", 1.0),
    ("powf", 1.0),
    ("exp", 1.0),
    ("ln", 1.0),
    ("sin", 1.0),
    ("cos", 1.0),
    ("atan2", 1.0),
];

struct Row {
    inputs: Vec<u64>,
    hi: u64,
    lo: u64,
    exact_bits: bool,
    pinned: Option<u64>,
}

fn path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/float_reference")
        .join(format!("{name}.txt"))
}

fn load(name: &str) -> (Vec<String>, Vec<Row>) {
    let text = std::fs::read_to_string(path(name)).expect("the vector file");
    let mut header = Vec::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            header.push(line.to_string());
            continue;
        }
        let fields: Vec<&str> = line.split(' ').collect();
        let n = fields.len() - 4;
        let hex = |s: &str| u64::from_str_radix(s, 16).expect("hex bits");
        rows.push(Row {
            inputs: fields[..n].iter().map(|s| hex(s)).collect(),
            hi: hex(fields[n]),
            lo: hex(fields[n + 1]),
            exact_bits: fields[n + 2] == "x",
            pinned: (fields[n + 3] != "-").then(|| hex(fields[n + 3])),
        });
    }
    (header, rows)
}

fn save(name: &str, header: &[String], rows: &[Row], got: &[u64]) {
    let mut out = String::new();
    for line in header {
        writeln!(out, "{line}").unwrap();
    }
    for (row, bits) in rows.iter().zip(got) {
        for i in &row.inputs {
            write!(out, "{i:016x} ").unwrap();
        }
        let kind = if row.exact_bits { 'x' } else { 'u' };
        writeln!(out, "{:016x} {:016x} {kind} {bits:016x}", row.hi, row.lo).unwrap();
    }
    std::fs::write(path(name), out).expect("write the vectors");
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

/// A program that prints, in hex, the bits of `math.<name>` for each row.
fn program(name: &str, rows: &[Row]) -> String {
    let arity = rows[0].inputs.len();
    let mut src = String::from(HELPERS);
    src.push_str("\nfn main(io: Stdout) -> Result<(), IoError> {\n");
    for k in 0..arity {
        let items: Vec<String> = rows.iter().map(|r| r.inputs[k].to_string()).collect();
        writeln!(src, "    let in{k}: List<U64> = [{}]", items.join(", ")).unwrap();
    }
    writeln!(src, "    for i in 0..in0.len() {{").unwrap();
    let args: Vec<String> = (0..arity)
        .map(|k| format!("Float.from_bits(in{k}.at(i))"))
        .collect();
    writeln!(src, "        let r = math.{name}({})", args.join(", ")).unwrap();
    src.push_str(
        "        let line = hex(r.to_bits())\n        io.println(line)?\n    }\n    Ok(())\n}\n",
    );
    src
}

fn run_bits(name: &str, rows: &[Row]) -> Vec<u64> {
    let outcome = run_fake(&program(name, rows), &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{name}: {outcome:#?}"
    );
    let got: Vec<u64> = outcome
        .stdout
        .lines()
        .map(|l| u64::from_str_radix(l, 16).expect("hex"))
        .collect();
    assert_eq!(got.len(), rows.len(), "{name}: one line per row");
    got
}

/// The gap between the doubles around the finite `x`, as a power of two.
fn ulp(x: f64) -> f64 {
    let bits = x.abs().to_bits();
    if bits >> 52 == 0 {
        return f64::from_bits(1);
    }
    f64::from_bits(bits + 1) - f64::from_bits(bits)
}

/// The error of `got` against the exact `hi + lo`, in ulps. Infinite when a
/// finite result is not finite, or the reverse.
fn error_in_ulps(got: u64, row: &Row) -> f64 {
    let (g, hi, lo) = (
        f64::from_bits(got),
        f64::from_bits(row.hi),
        f64::from_bits(row.lo),
    );
    if g.is_nan() || hi.is_nan() {
        return if g.is_nan() && hi.is_nan() {
            0.0
        } else {
            f64::INFINITY
        };
    }
    if g.is_infinite() || hi.is_infinite() {
        return if g == hi { 0.0 } else { f64::INFINITY };
    }
    ((g - hi) - lo).abs() / ulp(hi)
}

/// Runs the whole vector file of `name`; returns the largest error found,
/// in ulps.
fn check(name: &str) -> f64 {
    let bound = BOUNDS
        .iter()
        .find(|(n, _)| *n == name)
        .expect("a stated bound")
        .1;
    let (header, rows) = load(name);
    let got = run_bits(name, &rows);
    if std::env::var_os("OSKA_BLESS_FLOAT").is_some() {
        save(name, &header, &rows, &got);
    }
    let mut worst: f64 = 0.0;
    for ((row, &bits), n) in rows.iter().zip(&got).zip(0..) {
        let shown: Vec<String> = row.inputs.iter().map(|i| format!("{i:016x}")).collect();
        let at = format!("{name}({}) row {n}", shown.join(", "));
        if std::env::var_os("OSKA_BLESS_FLOAT").is_none() {
            assert_eq!(
                Some(bits),
                row.pinned,
                "{at}: the bits changed from the golden ones"
            );
        }
        if row.exact_bits || bound == 0.0 {
            assert_eq!(
                bits, row.hi,
                "{at}: exact bits {:016x}, got {bits:016x}",
                row.hi
            );
        } else {
            let error = error_in_ulps(bits, row);
            assert!(
                error <= bound,
                "{at}: {error} ulps from the exact value, bound {bound}: got {bits:016x}, exact {:016x}",
                row.hi
            );
            worst = worst.max(error);
        }
    }
    println!(
        "{name}: worst error {worst:.4} ulps over {} rows",
        rows.len()
    );
    worst
}

#[test]
fn abs_clears_the_sign_and_is_exact() {
    check("abs");
}

#[test]
fn floor_is_exact() {
    check("floor");
}

#[test]
fn ceil_is_exact() {
    check("ceil");
}

#[test]
fn round_half_even_is_exact() {
    check("round_half_even");
}

#[test]
fn hypot_stays_within_its_bound() {
    check("hypot");
}

#[test]
fn powf_stays_within_its_bound() {
    check("powf");
}

#[test]
fn exp_stays_within_its_bound() {
    check("exp");
}

#[test]
fn ln_stays_within_its_bound() {
    check("ln");
}

#[test]
fn sin_stays_within_its_bound() {
    check("sin");
}

#[test]
fn cos_stays_within_its_bound() {
    check("cos");
}

#[test]
fn atan2_stays_within_its_bound() {
    check("atan2");
}

fn one(call: &str) -> String {
    format!("use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let r = {call}\n    io.println(\"{{r}}\")?\n    Ok(())\n}}\n")
}

#[test]
fn sin_and_cos_refuse_an_argument_past_the_reduction_limit_with_p0006() {
    for call in [
        "math.sin(1647099.0)",
        "math.cos(-1647099.0)",
        "math.sin(1e22)",
        "math.cos(1.7976931348623157e308)",
    ] {
        assert_panics(&one(call), "P0006", call);
    }
}

#[test]
fn the_functions_print_the_values_a_reader_expects() {
    let src = "use std.math\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    let a = math.exp(0.0)\n    let b = math.ln(1.0)\n    let c = math.powf(2.0, 10.0)\n    let d = math.hypot(3.0, 4.0)\n    let e = math.floor(-2.5)\n    let f = math.ceil(-0.5)\n    let g = math.round_half_even(2.5)\n    let h = math.abs(-0.0)\n    let i = math.sin(0.0)\n    let j = math.cos(0.0)\n    let k = math.atan2(0.0, -1.0)\n    io.println(\"{a} {b} {c} {d} {e} {f} {g} {h} {i} {j} {k}\")?\n    Ok(())\n}\n";
    common::assert_runs(
        src,
        &[],
        "1.0 0.0 1024.0 5.0 -3.0 -0.0 2.0 0.0 0.0 1.0 3.141592653589793\n",
    );
}
