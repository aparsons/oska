// SPDX-License-Identifier: Apache-2.0
//! `Ord` and `Ordering` on the integers, `Bool` and `String` (spec #660,
//! ticket #664, ADR 0034): `<`, `<=`, `>` and `>=` on `Bool` and text, and
//! `cmp` on every `Ord` type, matched as an `Ordering`. Every helper that
//! runs a program also runs it natively on Linux and compares
//! (`common/mod.rs`), so each result holds on both backends.

mod common;

use common::{assert_refused, assert_runs};

/// A program of `items` and a `main` that prints each line of `body`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n")
}

/// Show an `Ordering` as `<`, `=` or `>`.
const SHOW: &str = "fn show(o: Ordering) -> String {
    match o {
        Ordering.Less => \"<\"
        Ordering.Equal => \"=\"
        Ordering.Greater => \">\"
    }
}
fn yes(b: Bool) -> String {
    if b { \"T\" } else { \"F\" }
}
";

/// The four operators on `a` and `b`, printed as `T` and `F`: `<`, `<=`,
/// `>`, `>=`.
fn operators(a: &str, b: &str) -> String {
    format!("    let ops = [yes({a} < {b}), yes({a} <= {b}), yes({a} > {b}), yes({a} >= {b})]\n    io.println(\"\".join(ops))?\n")
}

#[test]
fn text_orders_by_code_point() {
    // Pairs in order, so each `a < b`: a prefix before a longer text, the
    // empty text first, case by code point (`Z` before `a`), and non-ASCII
    // text after ASCII, with a longer code point after a shorter one.
    let pairs = [
        ("\"\"", "\"a\""),
        ("\"a\"", "\"ab\""),
        ("\"ab\"", "\"b\""),
        ("\"Z\"", "\"a\""),
        ("\"z\"", "\"\u{e9}\""),
        ("\"\u{e9}\"", "\"\u{4e2d}\""),
        ("\"\u{ffff}\"", "\"\u{1f600}\""),
        ("\"abcdefgh1\"", "\"abcdefgh2\""),
        ("\"abcdefghij\"", "\"abcdefghijk\""),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (a, b) in pairs {
        body.push_str(&operators(a, b));
        expected.push_str("TTFF\n");
        body.push_str(&operators(b, a));
        expected.push_str("FFTT\n");
    }
    body.push_str(&operators("\"same\"", "\"same\""));
    expected.push_str("FTFT\n");
    assert_runs(&program(SHOW, &body), &[], &expected);
}

#[test]
fn cmp_returns_an_ordering_that_matches() {
    let body = "    io.println(show(\"a\".cmp(\"b\")))?
    io.println(show(\"b\".cmp(\"a\")))?
    io.println(show(\"ab\".cmp(\"ab\")))?
    io.println(show(\"\".cmp(\"a\")))?
    io.println(show((3).cmp(5)))?
    io.println(show((5).cmp(3)))?
    io.println(show((5).cmp(5)))?
    io.println(show(false.cmp(true)))?
    io.println(show(true.cmp(false)))?
    io.println(show(true.cmp(true)))?";
    assert_runs(&program(SHOW, body), &[], "<\n>\n=\n<\n<\n>\n=\n<\n>\n=\n");
}

#[test]
fn every_integer_type_orders_by_value() {
    let body = "    let a: I8 = -128
    let b: I8 = 127
    let c: U8 = 255
    let d: U8 = 0
    let e: I16 = -1
    let f: U16 = 65535
    let g: I32 = -2147483648
    let h: U32 = 4294967295
    let i: I64 = -9223372036854775807
    let j: U64 = 18446744073709551615
    let k: U64 = 9223372036854775808
    io.println(show(a.cmp(b)))?
    io.println(show(c.cmp(d)))?
    io.println(show(e.cmp(-2)))?
    io.println(show(f.cmp(0)))?
    io.println(show(g.cmp(0)))?
    io.println(show(h.cmp(1)))?
    io.println(show(i.cmp(0)))?
    io.println(show(j.cmp(k)))?
    io.println(show(k.cmp(j)))?
    io.println(show(j.cmp(1)))?
    io.println(show(k.cmp(9223372036854775807)))?";
    assert_runs(
        &program(SHOW, body),
        &[],
        "<\n>\n>\n>\n<\n>\n<\n>\n<\n>\n>\n",
    );
}

#[test]
fn bool_orders_false_before_true() {
    let body = format!(
        "{}{}",
        operators("false", "true"),
        operators("true", "true")
    );
    assert_runs(&program(SHOW, &body), &[], "TTFF\nFTFT\n");
}

#[test]
fn a_comparison_works_in_a_condition_and_on_a_variable() {
    let body = "    let words = [\"pear\", \"apple\", \"fig\"]
    var first = words.at(0)
    for w in words {
        if w < first {
            first = w
        }
    }
    io.println(first)?
    var n = 0
    for w in words {
        if w >= \"fig\" && w <= \"pear\" {
            n = n + 1
        }
    }
    io.println(n.to_string())?";
    assert_runs(&program("", body), &[], "apple\n2\n");
}

#[test]
fn a_user_type_can_implement_ord_and_a_generic_function_can_use_it() {
    let items = "struct Version { major: Int, minor: Int }
impl Eq for Version {
    fn eq(self, other: Version) -> Bool {
        self.major == other.major && self.minor == other.minor
    }
}
impl Ord for Version {
    fn cmp(self, other: Version) -> Ordering {
        let by_major = self.major.cmp(other.major)
        match by_major {
            Ordering.Equal => self.minor.cmp(other.minor)
            _ => by_major
        }
    }
}
fn newer<T: Ord>(a: T, b: T) -> Bool {
    match a.cmp(b) {
        Ordering.Greater => true
        _ => false
    }
}
";
    let body = "    let a = Version(major: 1, minor: 9)
    let b = Version(major: 2, minor: 0)
    io.println(yes(newer(a, b)))?
    io.println(yes(newer(b, a)))?
    io.println(yes(newer(\"b\", \"a\")))?
    io.println(yes(newer(2, 3)))?";
    assert_runs(
        &program(&format!("{SHOW}{items}"), body),
        &[],
        "F\nT\nT\nF\n",
    );
}

#[test]
fn a_float_is_not_ord() {
    let src = program("", "    let x = (1.0).cmp(2.0)\n    io.println(\"no\")?");
    assert_refused(&src, "E0228");
    let src = program(
        "fn most<T: Ord>(a: T, b: T) -> T {\n    match a.cmp(b) {\n        Ordering.Less => b\n        _ => a\n    }\n}\n",
        "    let x = most(1.0, 2.0)\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0265");
}

#[test]
fn a_secret_is_not_ordered() {
    let src = program(
        "",
        "    let k: Secret<U32> = Secret.new(7)\n    let j: Secret<U32> = Secret.new(8)\n    let x = k < j\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0356");
}

#[test]
fn an_operator_on_a_user_type_is_refused_even_with_impl_ord() {
    // Operators on a user type with `impl Ord` are not defined yet: only
    // `cmp` is.
    let items = "struct Version { major: Int }
impl Eq for Version {
    fn eq(self, other: Version) -> Bool { self.major == other.major }
}
impl Ord for Version {
    fn cmp(self, other: Version) -> Ordering { self.major.cmp(other.major) }
}
";
    let src = program(
        items,
        "    let x = Version(major: 1) < Version(major: 2)\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0223");
}
