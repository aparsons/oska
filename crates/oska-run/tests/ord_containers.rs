// SPDX-License-Identifier: Apache-2.0
//! `Ord` for `Option<T: Ord>` (`None` before `Some`) and `List<T: Ord>`
//! (lexicographic, a prefix before a longer list) (spec #660, ticket #665,
//! ADR 0034). Every helper that runs a program also runs it natively on
//! Linux and compares (`common/mod.rs`), so each result holds on both
//! backends.

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
";

/// A line that prints how `a` orders against `b`.
fn line(a: &str, b: &str) -> String {
    format!("    io.println(show(({a}).cmp({b})))?\n")
}

/// Run the pairs and expect one of `<`, `=`, `>` for each.
fn check(items: &str, pairs: &[(&str, &str, &str)]) {
    let mut body = String::new();
    let mut expected = String::new();
    for (a, b, order) in pairs {
        body.push_str(&line(a, b));
        expected.push_str(order);
        expected.push('\n');
    }
    assert_runs(&program(&format!("{SHOW}{items}"), &body), &[], &expected);
}

#[test]
fn none_is_before_some() {
    check(
        "fn some(n: Int) -> Option<Int> { Some(n) }\nfn none() -> Option<Int> { None }\n",
        &[
            ("none()", "none()", "="),
            ("none()", "some(-5)", "<"),
            ("some(-5)", "none()", ">"),
            ("some(1)", "some(2)", "<"),
            ("some(2)", "some(1)", ">"),
            ("some(7)", "some(7)", "="),
        ],
    );
}

#[test]
fn an_option_of_text_and_of_an_option_orders_by_its_item() {
    check(
        "fn s(t: String) -> Option<String> { Some(t) }
fn nested(n: Int) -> Option<Option<Int>> { Some(Some(n)) }
fn inner_none() -> Option<Option<Int>> { Some(None) }
fn outer_none() -> Option<Option<Int>> { None }
",
        &[
            ("s(\"a\")", "s(\"ab\")", "<"),
            ("s(\"\")", "s(\"a\")", "<"),
            ("s(\"b\")", "s(\"a\")", ">"),
            ("outer_none()", "inner_none()", "<"),
            ("inner_none()", "nested(0)", "<"),
            ("nested(2)", "nested(1)", ">"),
            ("nested(1)", "nested(1)", "="),
        ],
    );
}

#[test]
fn lists_order_lexicographically_and_a_prefix_is_first() {
    check(
        "fn ints(a: List<Int>) -> List<Int> { a }\n",
        &[
            ("ints([])", "ints([])", "="),
            ("ints([])", "ints([0])", "<"),
            ("ints([0])", "ints([])", ">"),
            ("ints([1, 2])", "ints([1, 3])", "<"),
            ("ints([1, 3])", "ints([1, 2])", ">"),
            ("ints([1, 2])", "ints([1, 2, 0])", "<"),
            ("ints([1, 2, 0])", "ints([1, 2])", ">"),
            ("ints([2])", "ints([1, 9, 9])", ">"),
            ("ints([-1])", "ints([0])", "<"),
            ("ints([4, 5, 6])", "ints([4, 5, 6])", "="),
        ],
    );
}

#[test]
fn lists_of_text_lists_and_options_compare_item_by_item() {
    check(
        "fn words(a: List<String>) -> List<String> { a }
fn grid(a: List<List<Int>>) -> List<List<Int>> { a }
fn maybe(a: List<Option<Int>>) -> List<Option<Int>> { a }
",
        &[
            ("words([\"a\", \"b\"])", "words([\"a\", \"c\"])", "<"),
            ("words([\"b\"])", "words([\"a\", \"z\"])", ">"),
            ("words([\"ab\"])", "words([\"a\", \"b\"])", ">"),
            ("grid([[1], [2]])", "grid([[1], [2, 0]])", "<"),
            ("grid([[]])", "grid([])", ">"),
            ("grid([[1, 2]])", "grid([[1, 2]])", "="),
            ("maybe([None])", "maybe([Some(0)])", "<"),
            ("maybe([Some(1), None])", "maybe([Some(1), Some(0)])", "<"),
        ],
    );
}

#[test]
fn unsigned_and_bool_items_order_by_their_own_type() {
    check(
        "fn big(a: List<U64>) -> List<U64> { a }
fn flags(a: List<Bool>) -> List<Bool> { a }
fn byte(a: Option<U8>) -> Option<U8> { a }
",
        &[
            (
                "big([9223372036854775808])",
                "big([9223372036854775807])",
                ">",
            ),
            ("big([18446744073709551615])", "big([0])", ">"),
            ("flags([false, true])", "flags([false, false])", ">"),
            ("flags([true])", "flags([true, false])", "<"),
            ("byte(Some(255))", "byte(Some(0))", ">"),
        ],
    );
}

#[test]
fn a_user_type_with_ord_orders_inside_a_list_and_an_option() {
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
fn v(major: Int, minor: Int) -> Version { Version(major: major, minor: minor) }
fn versions(a: List<Version>) -> List<Version> { a }
fn maybe(a: Option<Version>) -> Option<Version> { a }
";
    check(
        items,
        &[
            ("versions([v(1, 9)])", "versions([v(2, 0)])", "<"),
            ("versions([v(1, 9), v(0, 0)])", "versions([v(1, 9)])", ">"),
            ("versions([v(1, 2)])", "versions([v(1, 2)])", "="),
            ("maybe(Some(v(1, 9)))", "maybe(Some(v(1, 10)))", "<"),
            ("maybe(None)", "maybe(Some(v(0, 0)))", "<"),
        ],
    );
}

#[test]
fn a_generic_function_over_ord_takes_lists_and_options() {
    let items = "fn later<T: Ord>(a: T, b: T) -> Bool {
    match a.cmp(b) {
        Ordering.Greater => true
        _ => false
    }
}
fn yes(b: Bool) -> String {
    if b { \"T\" } else { \"F\" }
}
";
    let body = "    io.println(yes(later([1, 2], [1])))?
    io.println(yes(later([1], [1, 2])))?
    io.println(yes(later(Some(\"b\"), Some(\"a\"))))?
    io.println(yes(later(Some(1), None)))?
    io.println(yes(later([[1]], [[1], []])))?";
    assert_runs(&program(items, body), &[], "T\nF\nT\nT\nF\n");
}

#[test]
fn a_sort_key_is_a_list_and_a_tuple_style_pair_is_an_option() {
    // The shape a later `sorted` uses: pick the least list by `cmp`.
    let body = "    let rows = [[3, 1], [2, 9, 9], [2, 9], [], [3]]
    var least = rows.at(0)
    for row in rows {
        match row.cmp(least) {
            Ordering.Less => {
                least = row
            }
            _ => {}
        }
    }
    let width = least.len()
    io.println(\"{width}\")?
    var best: Option<Int> = None
    for n in [4, 8, 2, 6] {
        match Some(n).cmp(best) {
            Ordering.Greater => {
                best = Some(n)
            }
            _ => {}
        }
    }
    io.println(best.unwrap_or(-1).to_string())?";
    assert_runs(&program("", body), &[], "0\n8\n");
}

#[test]
fn a_float_item_is_not_ord() {
    let src = program(
        "",
        "    let x = Some(1.0).cmp(Some(2.0))\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0265");
    let src = program(
        "fn most<T: Ord>(a: T, b: T) -> T {\n    match a.cmp(b) {\n        Ordering.Less => b\n        _ => a\n    }\n}\n",
        "    let x = most([1.0], [2.0])\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0265");
}

#[test]
fn an_item_with_no_ordering_is_refused() {
    let src = program(
        "struct Point { x: Int }\nimpl Eq for Point {\n    fn eq(self, other: Point) -> Bool { self.x == other.x }\n}\n",
        "    let x = [Point(x: 1)].cmp([Point(x: 2)])\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0265");
}

const OPS: &str = "fn yes(b: Bool) -> String {
    if b { \"T\" } else { \"F\" }
}
";

/// Run the pairs, each expecting `TTFF` (`a < b`), `FFTT` (`b < a`) or
/// `FTFT` (equal),.
fn check_operators(items: &str, pairs: &[(&str, &str, &str)]) {
    let mut body = String::new();
    let mut expected = String::new();
    for (a, b, order) in pairs {
        body.push_str(&format!(
            "    io.println(\"\".join([yes({a} < {b}), yes({a} <= {b}), yes({a} > {b}), yes({a} >= {b})]))?\n"
        ));
        let shown = match *order {
            "<" => "TTFF",
            ">" => "FFTT",
            _ => "FTFT",
        };
        expected.push_str(&format!("{shown}\n"));
    }
    assert_runs(&program(&format!("{OPS}{items}"), &body), &[], &expected);
}

#[test]
fn the_operators_order_options_and_lists_through_cmp() {
    check_operators(
        "fn some(n: Int) -> Option<Int> { Some(n) }\nfn none() -> Option<Int> { None }\nfn xs(a: List<Int>) -> List<Int> { a }\nfn ws(a: List<String>) -> List<String> { a }\nfn grid(a: List<List<Int>>) -> List<List<Int>> { a }\n",
        &[
            ("none()", "some(1)", "<"),
            ("some(2)", "some(1)", ">"),
            ("some(7)", "some(7)", "="),
            ("none()", "none()", "="),
            ("xs([1])", "xs([2])", "<"),
            ("xs([])", "xs([0])", "<"),
            ("xs([1, 2])", "xs([1])", ">"),
            ("xs([1, 2])", "xs([1, 2])", "="),
            ("ws([\"b\"])", "ws([\"a\", \"z\"])", ">"),
            ("grid([[1], [2]])", "grid([[1], [3]])", "<"),
        ],
    );
}

#[test]
fn the_operators_order_containers_of_a_user_type_with_ord() {
    let items = "struct V { n: Int }
impl Eq for V {
    fn eq(self, other: V) -> Bool { self.n == other.n }
}
impl Ord for V {
    fn cmp(self, other: V) -> Ordering { other.n.cmp(self.n) }
}
fn vs(a: List<V>) -> List<V> { a }
fn maybe(a: Option<V>) -> Option<V> { a }
";
    // `V` orders backwards, so the container does too.
    check_operators(
        items,
        &[
            ("vs([V(n: 2)])", "vs([V(n: 1)])", "<"),
            ("maybe(Some(V(n: 9)))", "maybe(Some(V(n: 3)))", "<"),
            ("maybe(None)", "maybe(Some(V(n: 3)))", "<"),
        ],
    );
}

#[test]
fn the_operators_work_in_a_generic_function_over_ord() {
    let items = "fn lower<T: Ord>(a: List<T>, b: List<T>) -> Bool { a < b }
fn opt<T: Ord>(a: Option<T>, b: Option<T>) -> Bool { a >= b }
";
    let body = "    io.println(yes(lower([1, 2], [1, 3])))?
    io.println(yes(lower([\"b\"], [\"a\"])))?
    io.println(yes(opt(Some(1), None)))?
    io.println(yes(opt(None, Some(\"x\"))))?";
    assert_runs(
        &program(&format!("{OPS}{items}"), body),
        &[],
        "T\nF\nT\nF\n",
    );
}

#[test]
fn an_operator_on_containers_without_ord_items_is_refused() {
    let src = program("", "    let x = [1.0] < [2.0]\n    io.println(\"no\")?");
    assert_refused(&src, "E0265");
    let src = program(
        "struct Point { x: Int }\nimpl Eq for Point {\n    fn eq(self, other: Point) -> Bool { self.x == other.x }\n}\n",
        "    let x = Some(Point(x: 1)) >= None\n    io.println(\"no\")?",
    );
    assert_refused(&src, "E0265");
}
