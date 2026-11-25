// SPDX-License-Identifier: Apache-2.0
//! `Float` arithmetic (spec #590, ticket #593): `+ - * /`, `sqrt`, the
//! comparisons, negation and `math.fma`, against golden vectors worked out
//! with the host's IEEE 754 operations, on the interpreter and, through
//! the differential helpers, on the native backend of the host (x86_64 or
//! aarch64). The vectors cover subnormals, signed zero, infinity and NaN
//! for each operation. Every expected result is a bit pattern: a NaN is
//! the canonical NaN, `0x7ff8000000000000`, whatever the host made.

mod common;

use common::{assert_refused, assert_runs};

const NAN: u64 = 0x7ff8_0000_0000_0000;

/// Bit patterns that take each operation to its edges: signed zeros, the
/// smallest and largest subnormals, the smallest and largest normals,
/// numbers near one, infinities, and NaNs of both signs and a payload (a
/// NaN pattern becomes the canonical NaN on its way in).
const EDGES: [u64; 20] = [
    0x0000_0000_0000_0000, // +0
    0x8000_0000_0000_0000, // -0
    0x0000_0000_0000_0001, // smallest subnormal
    0x8000_0000_0000_0001,
    0x000f_ffff_ffff_ffff, // largest subnormal
    0x0010_0000_0000_0000, // smallest normal
    0x3ff0_0000_0000_0000, // 1
    0xbff0_0000_0000_0000, // -1
    0x3ff8_0000_0000_0000, // 1.5
    0x3fb9_9999_9999_999a, // 0.1
    0x4008_0000_0000_0000, // 3
    0x4340_0000_0000_0000, // 2^53
    0x7fef_ffff_ffff_ffff, // largest finite
    0xffef_ffff_ffff_ffff,
    0x7ff0_0000_0000_0000, // +inf
    0xfff0_0000_0000_0000, // -inf
    0x7ff8_0000_0000_0000, // canonical NaN
    0xfff8_0000_0000_0001, // a negative NaN with a payload
    0x7ff0_0000_0000_0001, // a signalling NaN
    0x3ca0_0000_0000_0000, // 2^-53
];

fn canon(x: f64) -> u64 {
    if x.is_nan() {
        NAN
    } else {
        x.to_bits()
    }
}

/// The edge pattern as a float the way the program makes it.
fn edge(bits: u64) -> f64 {
    f64::from_bits(canon(f64::from_bits(bits)))
}

fn hex(bits: u64) -> String {
    format!("{bits:016x}\n")
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

fn float(bits: U64) -> Float {
    Float.from_bits(bits)
}
"#;

fn list(name: &str, items: &[u64]) -> String {
    let items: Vec<String> = items.iter().map(u64::to_string).collect();
    format!("    let {name}: List<U64> = [{}]\n", items.join(", "))
}

/// A program that prints `body`'s result for each pair of edges, `a` and
/// `b` being the pair as floats.
fn pairs(body: &str) -> String {
    format!(
        "{HELPERS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{}    for i in 0..edges.len() {{\n        for j in 0..edges.len() {{\n            let a = float(edges.at(i))\n            let b = float(edges.at(j))\n{body}\n        }}\n    }}\n    Ok(())\n}}\n",
        list("edges", &EDGES)
    )
}

fn expected_pairs(f: impl Fn(f64, f64) -> String) -> String {
    let mut out = String::new();
    for &a in &EDGES {
        for &b in &EDGES {
            out.push_str(&f(edge(a), edge(b)));
        }
    }
    out
}

fn arithmetic(op: &str, f: fn(f64, f64) -> f64) {
    let body = format!(
        "            let r = a {op} b\n            let line = hex(r.to_bits())\n            io.println(line)?"
    );
    assert_runs(
        &pairs(&body),
        &[],
        &expected_pairs(|a, b| hex(canon(f(a, b)))),
    );
}

#[test]
fn addition_is_ieee_on_the_edges() {
    arithmetic("+", |a, b| a + b);
}

#[test]
fn subtraction_is_ieee_on_the_edges() {
    arithmetic("-", |a, b| a - b);
}

#[test]
fn multiplication_is_ieee_on_the_edges() {
    arithmetic("*", |a, b| a * b);
}

#[test]
fn division_is_ieee_on_the_edges() {
    arithmetic("/", |a, b| a / b);
}

#[test]
fn comparisons_follow_ieee_with_nan_unordered_and_zeros_equal() {
    let body = "            let bits = [a == b, a != b, a < b, a <= b, a > b, a >= b]\n            var line = \"\"\n            for bit in bits {\n                if bit { line = \"{line}1\" } else { line = \"{line}0\" }\n            }\n            io.println(line)?";
    assert_runs(
        &pairs(body),
        &[],
        &expected_pairs(|a, b| {
            let bits = [a == b, a != b, a < b, a <= b, a > b, a >= b];
            let line: String = bits.iter().map(|&x| if x { '1' } else { '0' }).collect();
            format!("{line}\n")
        }),
    );
}

#[test]
fn square_root_and_negation_are_ieee_on_the_edges() {
    let program = format!(
        "{HELPERS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{}    for i in 0..edges.len() {{\n        let a = float(edges.at(i))\n        let root = hex(a.sqrt().to_bits())\n        let negated = hex((-a).to_bits())\n        io.println(root)?\n        io.println(negated)?\n    }}\n    Ok(())\n}}\n",
        list("edges", &EDGES)
    );
    let mut expected = String::new();
    for &a in &EDGES {
        let a = edge(a);
        expected.push_str(&hex(canon(a.sqrt())));
        expected.push_str(&hex(canon(-a)));
    }
    assert_runs(&program, &[], &expected);
}

#[test]
fn literals_have_the_nearest_value() {
    let program = format!(
        "{HELPERS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let xs = [1.5, 2e-3, 0.1, 1_000.25, 1e308, 5e-324, 2.5e-324, 0.0, -0.0, -1.5e2, 7E2, 1.7976931348623157e308]\n    for x in xs {{\n        let line = hex(x.to_bits())\n        io.println(line)?\n    }}\n    Ok(())\n}}\n"
    );
    let want: [f64; 12] = [
        1.5,
        2e-3,
        0.1,
        1000.25,
        1e308,
        5e-324,
        2.5e-324,
        0.0,
        -0.0,
        -1.5e2,
        7e2,
        1.797_693_134_862_315_7e308,
    ];
    let expected: String = want.iter().map(|x| hex(x.to_bits())).collect();
    assert_runs(&program, &[], &expected);
}

/// A tiny xorshift generator, so the random vectors are the same on every
/// run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A pattern from a class chosen to reach the edges of rounding:
    /// subnormals, the smallest and largest exponents, and exponents near
    /// each other so that sums cancel.
    fn float(&mut self) -> u64 {
        let class = self.next() % 8;
        let sign = (self.next() & 1) << 63;
        let fraction = match self.next() % 5 {
            0 => 0,
            1 => (1 << 52) - 1,
            2 => 1 << 51,
            _ => self.next() & ((1 << 52) - 1),
        };
        let exponent = match class {
            0 => self.next() % 2047,
            1 => 0,
            2 => 1 + self.next() % 3,
            3 => 2040 + self.next() % 7,
            4 => 1023 + self.next() % 3 - 1,
            5 => 1023 + self.next() % 120 - 60,
            6 => 500 + self.next() % 20,
            _ => 1500 + self.next() % 20,
        };
        sign | (exponent << 52) | fraction
    }
}

/// The correctly rounded `a * b + c`, from the host's `mul_add`, with
/// one repair. musl's `fma` returns `+0` when a product that is not zero
/// underflows to zero and `c` is a zero, whatever the product's sign; the
/// exact result is not zero, so IEEE 754 gives a zero of the product's
/// sign (`-0` for a negative product), which glibc and the hardware give.
fn reference_fma(a: f64, b: f64, c: f64) -> u64 {
    let mut bits = canon(a.mul_add(b, c));
    let product_is_nonzero = a != 0.0 && b != 0.0 && a.is_finite() && b.is_finite();
    if product_is_nonzero && c == 0.0 && bits == 0 && (a.is_sign_negative() != b.is_sign_negative())
    {
        bits = 0x8000_0000_0000_0000;
    }
    bits
}

#[test]
fn fma_rounds_once_on_the_edges_and_on_random_vectors() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut triples: Vec<[u64; 3]> = Vec::new();
    for &a in &EDGES[..14] {
        for &b in &EDGES[..14] {
            for &c in &[
                EDGES[0], EDGES[1], EDGES[6], EDGES[12], EDGES[14], EDGES[16],
            ] {
                triples.push([a, b, c]);
            }
        }
    }
    for _ in 0..600 {
        triples.push([rng.float(), rng.float(), rng.float()]);
    }
    let flat: Vec<u64> = triples.iter().flatten().copied().collect();
    let program = format!(
        "{HELPERS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{}    for i in 0..(xs.len() / 3) {{\n        let a = float(xs.at(3 * i))\n        let b = float(xs.at(3 * i + 1))\n        let c = float(xs.at(3 * i + 2))\n        let line = hex(math.fma(a, b, c).to_bits())\n        io.println(line)?\n    }}\n    Ok(())\n}}\n",
        list("xs", &flat)
    );
    let expected: String = triples
        .iter()
        .map(|&[a, b, c]| hex(reference_fma(edge(a), edge(b), edge(c))))
        .collect();
    assert_runs(&program, &[], &expected);
}

#[test]
fn an_explicit_fma_differs_from_a_multiplication_and_an_addition() {
    // 0.1 * 10.0 is exactly 1 - 2^-54 rounded up to 1.0, so the two
    // operators give 0.0 and the fused operation gives 2^-54.
    let program = format!(
        "{HELPERS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n    let a = 0.1\n    let b = 10.0\n    let c = -1.0\n    let two = a * b + c\n    let one = math.fma(a, b, c)\n    let first = hex(two.to_bits())\n    let second = hex(one.to_bits())\n    io.println(first)?\n    io.println(second)?\n    Ok(())\n}}\n"
    );
    assert_runs(
        &program,
        &[],
        &format!("{}{}", hex(0), hex(0x3c90_0000_0000_0000)),
    );
}

#[test]
fn a_float_does_not_mix_with_an_int_or_take_a_remainder() {
    assert_refused(
        "fn main(io: Stdout) -> Result<(), IoError> {\n    let x = 1.5 + 1\n    Ok(())\n}\n",
        "E0237",
    );
    assert_refused(
        "fn main(io: Stdout) -> Result<(), IoError> {\n    let x = 1.5 % 2.0\n    Ok(())\n}\n",
        "E0223",
    );
    assert_refused(
        "fn main(io: Stdout) -> Result<(), IoError> {\n    let x = 1e999\n    Ok(())\n}\n",
        "E0222",
    );
}
