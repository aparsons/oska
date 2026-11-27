// SPDX-License-Identifier: Apache-2.0
//! `Float` conversions and text (spec #590, ticket #594): `to_int`,
//! `Int.to_float`, `to_string`, `fmt`, `parse_float`, `total_cmp` and
//! `approx_eq`, on the interpreter and, through the differential helpers,
//! on the native backend of the host.
//!
//! The expected text is worked out here from the rules in the prelude, with
//! Rust's `{:e}` for the shortest digits and `str::parse` for the correctly
//! rounded value: both are pure Rust, with no call into the C library, so
//! they are the same on musl and glibc. The native backend writes both in
//! Oska, so these tests are what holds the two apart.

mod common;

use common::{assert_panics, assert_runs};

const SIGN: u64 = 0x8000_0000_0000_0000;
const NAN: u64 = 0x7ff8_0000_0000_0000;

/// Patterns at the edges of text: zeros, subnormals, powers of ten around
/// the switch to an exponent, the largest values, and the values that are
/// the hardest to print or parse.
const EDGES: [u64; 24] = [
    0x0000_0000_0000_0000, // +0
    0x8000_0000_0000_0000, // -0
    0x0000_0000_0000_0001, // smallest subnormal
    0x000f_ffff_ffff_ffff, // largest subnormal
    0x0010_0000_0000_0000, // smallest normal
    0x3ff0_0000_0000_0000, // 1
    0xbff0_0000_0000_0000, // -1
    0x3ff8_0000_0000_0000, // 1.5
    0x3fb9_9999_9999_999a, // 0.1
    0x3fc9_9999_9999_999a, // 0.2
    0x3fd3_3333_3333_3333, // 0.3
    0x4008_0000_0000_0000, // 3
    0x4024_0000_0000_0000, // 10
    0x3ee4_f8b5_88e3_68f1, // 1e-5
    0x3eb0_c6f7_a0b5_ed8d, // 1e-6
    0x4341_c379_37e0_8000, // 1e16
    0x430c_6bf5_2634_0000, // 1e15
    0x4340_0000_0000_0000, // 2^53
    0x7fef_ffff_ffff_ffff, // largest finite
    0xffef_ffff_ffff_ffff,
    0x7ff0_0000_0000_0000, // +inf
    0xfff0_0000_0000_0000, // -inf
    0x7ff8_0000_0000_0000, // NaN
    0x4415_af1d_78b5_8c40, // 1e20
];

fn canon(x: f64) -> u64 {
    if x.is_nan() {
        NAN
    } else {
        x.to_bits()
    }
}

/// The text of `x` by the rules of the prelude: the shortest digits that
/// parse back, in plain notation for a decimal exponent from -5 to 15, and
/// as `d.ddde<exponent>` otherwise.
fn text(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x.is_infinite() {
        return format!("{sign}inf");
    }
    if x == 0.0 {
        return format!("{sign}0.0");
    }
    let shortest = format!("{:e}", x.abs());
    let (mantissa, exponent) = shortest.split_once('e').expect("an exponent");
    let exponent: i32 = exponent.parse().expect("a number");
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let body = if (-5..16).contains(&exponent) {
        if exponent >= 0 {
            let point = exponent as usize + 1;
            let int_part = format!("{digits:0<point$}");
            let (int_part, rest) = int_part.split_at(point);
            let fraction = &digits[digits.len().min(point)..];
            let _ = rest;
            format!(
                "{int_part}.{}",
                if fraction.is_empty() { "0" } else { fraction }
            )
        } else {
            format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
        }
    } else if digits.len() == 1 {
        format!("{digits}e{exponent}")
    } else {
        format!("{}.{}e{exponent}", &digits[..1], &digits[1..])
    };
    format!("{sign}{body}")
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

    /// A finite pattern from a class that reaches the edges of rounding.
    fn float(&mut self) -> u64 {
        let sign = (self.next() & 1) << 63;
        let fraction = match self.next() % 4 {
            0 => 0,
            1 => (1 << 52) - 1,
            _ => self.next() & ((1 << 52) - 1),
        };
        let exponent = match self.next() % 5 {
            0 => self.next() % 2047,
            1 => 0,
            2 => 1 + self.next() % 3,
            3 => 1023 + self.next() % 80 - 40,
            _ => 2030 + self.next() % 17,
        };
        let bits = sign | (exponent << 52) | fraction;
        if exponent == 2047 {
            bits & !(1 << 52)
        } else {
            bits
        }
    }

    /// A decimal string, with the digits, point and exponent of a number
    /// text, around the edges of the double's range.
    fn decimal(&mut self) -> String {
        let count = 1 + self.next() % 25;
        let mut digits = String::new();
        for _ in 0..count {
            digits.push(char::from(b'0' + (self.next() % 10) as u8));
        }
        let mut out = String::new();
        if self.next() % 4 == 0 {
            out.push('-');
        }
        let point = self.next() % (count + 1);
        if point == 0 {
            out.push('0');
            out.push('.');
            out.push_str(&digits);
        } else if point == count {
            out.push_str(&digits);
        } else {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        }
        match self.next() % 4 {
            0 => {}
            1 => out.push_str(&format!("e{}", self.next() % 330)),
            2 => out.push_str(&format!("e-{}", self.next() % 340)),
            _ => out.push_str(&format!("E{}", self.next() % 40)),
        }
        out
    }
}

fn list_u64(name: &str, items: &[u64]) -> String {
    let items: Vec<String> = items.iter().map(u64::to_string).collect();
    format!("    let {name}: List<U64> = [{}]\n", items.join(", "))
}

fn list_str(name: &str, items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("    let {name}: List<String> = [{}]\n", items.join(", "))
}

fn main_of(body: &str) -> String {
    format!("fn hex(bits: U64) -> String {{\n    var text = \"\"\n    for i in 0..16 {{\n        let shift = (60 - 4 * i).to_u64()\n        let nibble = ((bits >> shift) & 15).to_int()\n        let digit = \"0123456789abcdef\".slice(nibble, nibble + 1)\n        text = \"{{text}}{{digit}}\"\n    }}\n    text\n}}\n\nfn shown(b: Bool) -> String {{\n    if b {{ \"true\" }} else {{ \"false\" }}\n}}\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}    Ok(())\n}}\n")
}

fn all_bits() -> Vec<u64> {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut bits = EDGES.to_vec();
    for _ in 0..400 {
        bits.push(rng.float());
    }
    bits
}

#[test]
fn to_string_is_the_shortest_text_that_parses_back() {
    let bits = all_bits();
    let program = main_of(&format!(
        "{}    for b in xs {{\n        let x = Float.from_bits(b)\n        io.println(x.to_string())?\n    }}\n",
        list_u64("xs", &bits)
    ));
    let expected: String = bits
        .iter()
        .map(|&b| format!("{}\n", text(f64::from_bits(canon(f64::from_bits(b))))))
        .collect();
    assert_runs(&program, &[], &expected);
}

#[test]
fn to_string_has_fixed_forms() {
    let program = main_of(
        "    let xs = [1.5, 100.0, 1e15, 1e16, 1.5e16, 0.00001, 1e-6, 1.5e-7, 123456.789, 5e-324, 1.7976931348623157e308, 0.1, 0.30000000000000004, -2.5, 0.0, -0.0]\n    for x in xs {\n        io.println(\"{x}\")?\n    }\n",
    );
    assert_runs(
        &program,
        &[],
        "1.5\n100.0\n1000000000000000.0\n1e16\n1.5e16\n0.00001\n1e-6\n1.5e-7\n123456.789\n5e-324\n1.7976931348623157e308\n0.1\n0.30000000000000004\n-2.5\n0.0\n-0.0\n",
    );
}

#[test]
fn dbg_prints_the_text_of_a_float() {
    let outcome = common::run_fake(
        &main_of("    let x = dbg(0.1)\n    io.println(x.to_string())?\n"),
        &[],
    );
    assert_eq!(outcome.stdout, "0.1\n");
    assert_eq!(outcome.stderr, "[dbg] 0.1\n");
}

#[test]
fn parse_float_gives_back_what_to_string_printed() {
    let bits = all_bits();
    let program = main_of(&format!(
        "{}    for b in xs {{\n        let x = Float.from_bits(b)\n        if x == x && x - x == 0.0 {{\n            match x.to_string().parse_float() {{\n                Some(y) => {{\n                    let bits = y.to_bits()\n                    io.println(hex(bits))?\n                }}\n                None => io.println(\"none\")?\n            }}\n        }}\n    }}\n",
        list_u64("xs", &bits)
    ));
    let expected: String = bits
        .iter()
        .map(|&b| f64::from_bits(b))
        .filter(|x| x.is_finite())
        .map(|x| format!("{:016x}\n", x.to_bits()))
        .collect();
    assert_runs(&program, &[], &expected);
}

/// What `parse_float` gives for `s`: the bits, or `none`.
fn parsed(s: &str) -> String {
    match s.parse::<f64>() {
        Ok(x) if x.is_finite() => format!("{:016x}", x.to_bits()),
        _ => "none".to_string(),
    }
}

fn parse_program(strings: &[String], call: &str) -> String {
    main_of(&format!(
        "{}    for s in xs {{\n        match s.{call}() {{\n            Some(x) => {{\n                let bits = x.to_bits()\n                io.println(hex(bits))?\n            }}\n            None => io.println(\"none\")?\n        }}\n    }}\n",
        list_str("xs", strings)
    ))
}

#[test]
fn parse_float_is_correctly_rounded() {
    let mut strings: Vec<String> = [
        "0",
        "-0",
        "0.0",
        "1",
        "1.5",
        "-1.5",
        "0.1",
        "1e0",
        "1E5",
        "1e-5",
        "123456789012345678",
        // 2^53 + 1: halfway between two doubles, so the even one.
        "9007199254740993",
        "9007199254740993.0000000000000000000000000000001",
        "9007199254740995",
        // The largest double, the first value that rounds up to infinity
        // (rejected), and the one just under it.
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "1.7976931348623159e308",
        // The smallest subnormal, half of it (rounds to even, zero), just
        // above half of it, and the largest subnormal.
        "5e-324",
        "2.4703282292062327e-324",
        "2.4703282292062328e-324",
        "2.2250738585072009e-308",
        "2.2250738585072014e-308",
        "1e-400",
        "0.000000000000000000000000000000000000000000000000000000000000001",
        "1e23",
        "8.41e21",
        "5.0e-324",
        "00012.5",
        "1.0e+5",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..300 {
        strings.push(rng.decimal());
    }
    // A long run of digits: only the sticky bit past the 800th matters.
    strings.push(format!("0.{}1", "0".repeat(60)));
    strings.push(format!("1{}", "0".repeat(300)));
    let expected: String = strings.iter().map(|s| format!("{}\n", parsed(s))).collect();
    assert_runs(&parse_program(&strings, "parse_float"), &[], &expected);
}

#[test]
fn parse_float_refuses_what_it_does_not_take_by_default() {
    // Attack input: a special value, a sign the text did not need, an
    // overflow and an over-long text must be an error, never a number.
    let long = "9".repeat(10_000);
    let strings: Vec<String> = [
        "NaN", "nan", "inf", "-inf", "Infinity", "infinity", "+1", "+0", "1e999", "-1e999", "+inf",
        "", " ", "1 ", " 1", "1.", ".5", "-", "-.5", "1e", "1e+", "e5", "0x10", "1_000", "1,5",
        "١", "--1", "1.5.5", "1e5.5", "0b1", "1d", "1f", "١٢",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .chain([long.clone(), format!("0.{long}"), format!("1e{long}")])
    .collect();
    let expected = "none\n".repeat(strings.len());
    assert_runs(&parse_program(&strings, "parse_float"), &[], &expected);
}

#[test]
fn parse_float_lenient_takes_the_forms_the_default_refuses() {
    let strings: Vec<String> = [
        "NaN",
        "nan",
        "inf",
        "-inf",
        "Infinity",
        "-infinity",
        "+1",
        "+1.5e2",
        "+inf",
        "1e999",
        "-1e999",
        "1.5",
        "",
        "+",
        "infx",
        "--1",
        "+-1",
        "nan1",
        "1_0",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let line = |bits: u64| format!("{bits:016x}\n");
    let none = "none\n";
    let expected = [
        line(NAN),
        line(NAN),
        line(0x7ff0_0000_0000_0000),
        line(0xfff0_0000_0000_0000),
        line(0x7ff0_0000_0000_0000),
        line(0xfff0_0000_0000_0000),
        line(1.0f64.to_bits()),
        line(150.0f64.to_bits()),
        line(0x7ff0_0000_0000_0000),
        line(0x7ff0_0000_0000_0000),
        line(0xfff0_0000_0000_0000),
        line(1.5f64.to_bits()),
        none.to_string(),
        none.to_string(),
        none.to_string(),
        none.to_string(),
        none.to_string(),
        none.to_string(),
        none.to_string(),
    ]
    .concat();
    assert_runs(
        &parse_program(&strings, "parse_float_lenient"),
        &[],
        &expected,
    );
}

#[test]
fn parse_float_limits_its_length_unless_lenient() {
    let at_limit = format!("1{}", "0".repeat(399));
    let over = format!("1{}", "0".repeat(400));
    let program = main_of(&format!(
        "    let a = \"{at_limit}\"\n    let b = \"{over}\"\n    let first = a.parse_float().is_some()\n    let second = b.parse_float().is_some()\n    let third = b.parse_float_lenient().is_some()\n    let a1 = shown(first)\n    let a2 = shown(second)\n    let a3 = shown(third)\n    io.println(\"{{a1}} {{a2}} {{a3}}\")?\n"
    ));
    // 1e399 is out of range, so the shorter one is refused as well, but
    // for its size: a number of the same length that fits is read.
    let fits = format!("0.{}1", "0".repeat(397));
    let fits_over = format!("0.{}1", "0".repeat(398));
    let program2 = main_of(&format!(
        "    let a = \"{fits}\"\n    let b = \"{fits_over}\"\n    let first = a.parse_float().is_some()\n    let second = b.parse_float().is_some()\n    let third = b.parse_float_lenient().is_some()\n    let a1 = shown(first)\n    let a2 = shown(second)\n    let a3 = shown(third)\n    io.println(\"{{a1}} {{a2}} {{a3}}\")?\n"
    ));
    assert_runs(&program, &[], "false false true\n");
    assert_runs(&program2, &[], "true false true\n");
}

#[test]
fn fmt_prints_fixed_decimals_rounding_exact_ties_to_even() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut cases: Vec<(u64, i64)> = Vec::new();
    for &b in &EDGES[..21] {
        for d in [0, 1, 2, 5] {
            cases.push((b, d));
        }
    }
    for &x in &[
        0.125f64, 0.375, 2.5, 3.5, 0.5, 1.5, 0.045, 1.005, 2.675, 1e21, 123.456,
    ] {
        for d in [0, 1, 2, 3] {
            cases.push((x.to_bits(), d));
        }
    }
    for _ in 0..200 {
        let d = (rng.next() % 30) as i64;
        let b = rng.float();
        // Keep the text short: a large exponent makes a very long line.
        if f64::from_bits(b).abs() < 1e40 {
            cases.push((b, d));
        }
    }
    cases.push((5e-324f64.to_bits(), 340));
    cases.push((0.1f64.to_bits(), 340));
    let xs: Vec<u64> = cases.iter().map(|c| c.0).collect();
    let ds: Vec<String> = cases.iter().map(|c| c.1.to_string()).collect();
    let program = main_of(&format!(
        "{}    let ds: List<Int> = [{}]\n    for i in 0..xs.len() {{\n        let x = Float.from_bits(xs.at(i))\n        io.println(x.fmt(ds.at(i)))?\n    }}\n",
        list_u64("xs", &xs),
        ds.join(", ")
    ));
    let expected: String = cases
        .iter()
        .map(|&(b, d)| {
            let x = f64::from_bits(canon(f64::from_bits(b)));
            let line = if x.is_nan() {
                "NaN".to_string()
            } else if x.is_infinite() {
                text(x)
            } else {
                format!("{x:.*}", d as usize)
            };
            format!("{line}\n")
        })
        .collect();
    assert_runs(&program, &[], &expected);
}

#[test]
fn fmt_digits_out_of_range_panics() {
    for digits in ["-1", "341"] {
        let src = main_of(&format!(
            "    let x = 1.5\n    io.println(x.fmt({digits}))?\n"
        ));
        assert_panics(&src, "P0050", &format!("x.fmt({digits})"));
    }
}

#[test]
fn to_int_truncates_toward_zero() {
    let xs: [f64; 14] = [
        0.0,
        -0.0,
        0.9,
        -0.9,
        1.5,
        -1.5,
        2.5,
        123456789.99,
        4503599627370497.0,
        9007199254740992.0,
        -9007199254740992.0,
        9223372036854774784.0,
        -9223372036854775808.0,
        1e18,
    ];
    let bits: Vec<u64> = xs.iter().map(|x| x.to_bits()).collect();
    let program = main_of(&format!(
        "{}    for b in xs {{\n        let n = Float.from_bits(b).to_int()\n        io.println(\"{{n}}\")?\n    }}\n",
        list_u64("xs", &bits)
    ));
    let expected: String = xs.iter().map(|&x| format!("{}\n", x as i64)).collect();
    assert_runs(&program, &[], &expected);
}

#[test]
fn to_int_panics_on_nan_infinity_and_out_of_range() {
    let cases: [(u64, &str); 6] = [
        (NAN, "NaN"),
        (0x7ff0_0000_0000_0000, "inf"),
        (0xfff0_0000_0000_0000, "-inf"),
        (9223372036854775808.0f64.to_bits(), "9.223372036854776e18"),
        (1e19f64.to_bits(), "1e19"),
        (
            (-9223372036854777856.0f64).to_bits(),
            "-9.223372036854778e18",
        ),
    ];
    for (bits, shown) in cases {
        let src = main_of(&format!(
            "    let x = Float.from_bits({bits})\n    let n = x.to_int()\n    io.println(\"{{n}}\")?\n"
        ));
        assert_panics(&src, "P0046", "x.to_int()");
        let outcome = common::run_fake(&src, &[]);
        let message = outcome.panic.expect("a panic").message;
        assert!(
            message.contains(shown),
            "the message names the value {shown}: {message}"
        );
    }
}

#[test]
fn int_to_float_is_exact_up_to_two_to_the_53() {
    let program = main_of(
        "    let xs = [0, 1, -1, 123456789, 9007199254740991, 9007199254740992, -9007199254740992]\n    for n in xs {\n        io.println(n.to_float().to_string())?\n    }\n",
    );
    assert_runs(
        &program,
        &[],
        "0.0\n1.0\n-1.0\n123456789.0\n9007199254740991.0\n9007199254740992.0\n-9007199254740992.0\n",
    );
}

#[test]
fn int_to_float_refuses_what_it_would_round() {
    for n in [
        "9007199254740993",
        "-9007199254740993",
        "9223372036854775807",
    ] {
        let src = main_of(&format!(
            "    let n = {n}\n    let x = n.to_float()\n    io.println(x.to_string())?\n"
        ));
        assert_panics(&src, "P0049", "n.to_float()");
    }
    let src = main_of(
        "    let n = -9223372036854775807 - 1\n    let x = n.to_float()\n    io.println(x.to_string())?\n",
    );
    assert_panics(&src, "P0049", "n.to_float()");
}

#[test]
fn int_to_float_rounded_rounds_to_nearest_even() {
    let mut rng = Rng(0xdead_beef_1234_5678);
    let mut ns: Vec<i64> = vec![
        0,
        1,
        -1,
        9007199254740993,
        9007199254740995,
        -9007199254740993,
        9007199254740994,
        i64::MAX,
        i64::MIN,
        i64::MAX - 1023,
        i64::MAX - 1024,
        i64::MAX - 1025,
        1 << 62,
        (1 << 62) + 1,
        (1 << 62) + 512,
        (1 << 62) + 513,
        (1 << 62) + 1536,
    ];
    for _ in 0..300 {
        let shift = rng.next() % 64;
        ns.push((rng.next() >> shift) as i64 * if rng.next() & 1 == 0 { 1 } else { -1 });
    }
    let list: Vec<String> = ns.iter().map(|n| n.to_string()).collect();
    let program = main_of(&format!(
        "    let ns: List<Int> = [{}]\n    for n in ns {{\n        let x = n.to_float_rounded()\n        let bits = x.to_bits()\n        io.println(hex(bits))?\n    }}\n",
        list.join(", ").replace("-9223372036854775808", "-9223372036854775807 - 1")
    ));
    let expected: String = ns
        .iter()
        .map(|&n| format!("{:016x}\n", (n as f64).to_bits()))
        .collect();
    assert_runs(&program, &[], &expected);
}

#[test]
fn total_cmp_is_a_total_order() {
    let order: Vec<u64> = vec![
        0xfff0_0000_0000_0000, // -inf
        0xffef_ffff_ffff_ffff,
        0xbff0_0000_0000_0000, // -1
        0x8000_0000_0000_0001,
        SIGN, // -0
        0,    // +0
        1,
        0x3ff0_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000, // +inf
        NAN,
    ];
    let program = main_of(&format!(
        "{}    for i in 0..xs.len() {{\n        for j in 0..xs.len() {{\n            let a = Float.from_bits(xs.at(i))\n            let b = Float.from_bits(xs.at(j))\n            let order = a.total_cmp(b)\n            io.println(\"{{order}}\")?\n        }}\n    }}\n",
        list_u64("xs", &order)
    ));
    let mut expected = String::new();
    for i in 0..order.len() {
        for j in 0..order.len() {
            expected.push_str(&format!("{}\n", (i as i64 - j as i64).signum()));
        }
    }
    assert_runs(&program, &[], &expected);
}

#[test]
fn approx_eq_compares_within_a_relative_and_an_absolute_tolerance() {
    let program = main_of(
        "    let nan = Float.from_bits(9221120237041090560)\n    let inf = 1.0 / 0.0\n    let cases = [\n        (1.0).approx_eq(1.0, 0.0, 0.0),\n        (0.1 + 0.2).approx_eq(0.3, 1e-9, 0.0),\n        (0.1 + 0.2).approx_eq(0.3, 0.0, 0.0),\n        (1e10).approx_eq(1e10 + 1.0, 1e-9, 0.0),\n        (1e10).approx_eq(1e10 + 100.0, 1e-9, 0.0),\n        (0.0).approx_eq(1e-12, 0.0, 1e-9),\n        (0.0).approx_eq(1e-3, 1e-9, 1e-9),\n        nan.approx_eq(nan, 1.0, 1.0),\n        inf.approx_eq(inf, 0.0, 0.0),\n        inf.approx_eq(-inf, 1.0, 1.0),\n        inf.approx_eq(1e308, 1.0, 0.0),\n        (1.0).approx_eq(2.0, nan, 0.0),\n        (-0.0).approx_eq(0.0, 0.0, 0.0),\n    ]\n    for c in cases {\n        let line = shown(c)\n        io.println(line)?\n    }\n",
    );
    assert_runs(
        &program,
        &[],
        "true\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\nfalse\ntrue\nfalse\nfalse\nfalse\ntrue\n",
    );
}
