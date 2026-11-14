//! Calls that pass more arguments than a target passes in registers
//! (eight on aarch64, six on x86_64; ticket #162), run on both backends
//! through the differential harness (seam 1): direct calls, calls
//! through function values and closures, trait methods through a bound,
//! mutating methods, contracts and `?`. Every target must compile these
//! programs (`differential::assert_compiled_same`): a refusal fails on
//! every host, and each host builds them for the other targets too.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use differential::{assert_compiled_same, Case};

/// The helper every program has. Interpolation takes only names, so a
/// program prints through it.
const HELPERS: &str = r#"fn num(io: Stdout, n: Int) {
    let _ = io.println("{n}")
}

/// Ten arguments, each weighted by its place, so that any two swapped
/// or lost change the sum.
fn weigh(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int) -> Int {
    a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j
}
"#;

/// A program with `items` whose `main` runs `body` with `io: Stdout`.
fn program(items: &str, body: &str) -> String {
    format!("{HELPERS}\n{items}\n\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run the program on both backends, require the same outcome, and that
/// it printed `stdout` and exited 0.
fn assert_prints(name: &str, items: &str, body: &str, stdout: &str) {
    let outcome = assert_compiled_same(&Case::file(name, &program(items, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

#[test]
fn ten_arguments_reach_a_function_called_directly_and_through_a_value() {
    let body = "    num(io, weigh(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    let f = weigh
    num(io, f(10, 9, 8, 7, 6, 5, 4, 3, 2, 1))";
    assert_prints("wide_value", "", body, "385\n220\n");
}

#[test]
fn an_argument_that_is_itself_a_wide_call_keeps_both_calls_arguments_apart() {
    let items = "fn rotate(n: Int, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int {
    if n == 0 {
        weigh(a, b, c, d, e, f, g, h, i, 0)
    } else {
        rotate(n - 1, b, c, d, e, f, g, h, i, a)
    }
}";
    let body = "    num(io, weigh(1, 2, 3, 4, 5, 6, 7, weigh(1, 1, 1, 1, 1, 1, 1, 1, 1, 1), 9, 10))
    num(io, rotate(1, 1, 2, 3, 4, 5, 6, 7, 8, 9))
    num(io, rotate(9, 1, 2, 3, 4, 5, 6, 7, 8, 9))";
    assert_prints("wide_nested", items, body, "761\n249\n285\n");
}

#[test]
fn closures_and_function_values_of_every_width_get_their_arguments() {
    let items =
        "fn apply(f: fn(Int, Int, Int, Int, Int, Int, Int, Int, Int, Int) -> Int, n: Int) -> Int {
    f(n, n, n, n, n, n, n, n, n, n)
}

fn six(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int) -> Int {
    weigh(a, b, c, d, e, f, 0, 0, 0, 0)
}";
    let body = "    let base = 1000
    let shifted = |a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int| base + weigh(a, b, c, d, e, f, g, h, i, j)
    num(io, shifted(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    num(io, apply(weigh, 1))
    num(io, apply(shifted, 2))
    let s = six
    num(io, s(1, 2, 3, 4, 5, 6))
    let five = |a: Int, b: Int, c: Int, d: Int, e: Int| base + six(a, b, c, d, e, 1)
    num(io, five(1, 1, 1, 1, 1))
    let seven = |a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int| six(a, b, c, d, e, f) * g + base
    num(io, seven(1, 2, 3, 4, 5, 6, 2))";
    assert_prints(
        "wide_closures",
        items,
        body,
        "1385\n55\n1110\n91\n1021\n1182\n",
    );
}

#[test]
fn trait_and_mutating_methods_take_a_receiver_and_nine_arguments() {
    let items = "trait Mix {
    fn mix(self, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int
}

struct Base { n: Int }

impl Mix for Base {
    fn mix(self, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int {
        self.n + weigh(a, b, c, d, e, f, g, h, i, 0)
    }
}

fn mixed<T: Mix>(x: T) -> Int {
    x.mix(1, 2, 3, 4, 5, 6, 7, 8, 9)
}

struct Tally { total: Int, calls: Int }

impl Tally {
    fn add(var self, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int {
        self.total = self.total + weigh(a, b, c, d, e, f, g, h, i, 0)
        self.calls = self.calls + 1
        self.calls
    }
}";
    let body = "    num(io, mixed(Base(n: 5)))
    var t = Tally(total: 0, calls: 0)
    num(io, t.add(1, 2, 3, 4, 5, 6, 7, 8, 9))
    num(io, t.add(9, 8, 7, 6, 5, 4, 3, 2, 1))
    num(io, t.total)";
    assert_prints("wide_methods", items, body, "290\n1\n2\n450\n");
}

#[test]
fn question_mark_leaves_a_function_of_ten_parameters() {
    let items = "fn first(a: String, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int) -> Option<Int> {
    let n = a.parse_int()?
    Some(weigh(n, b, c, d, e, f, g, h, i, j))
}

fn shown(io: Stdout, o: Option<Int>) {
    match o {
        Some(n) => num(io, n)
        None => num(io, -1)
    }
}";
    let body = "    shown(io, first(\"1\", 2, 3, 4, 5, 6, 7, 8, 9, 10))
    shown(io, first(\"x\", 2, 3, 4, 5, 6, 7, 8, 9, 10))";
    assert_prints("wide_question_mark", items, body, "385\n-1\n");
}

/// A function of ten parameters whose contract reads its last one and
/// its result.
const SPREAD: &str = "fn spread(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int) -> Int
    requires j > 0
    ensures |r| r < 500
{
    weigh(a, b, c, d, e, f, g, h, i, j)
}";

/// Run `body` after [`SPREAD`] on both backends, require the same
/// outcome, and that it printed `stdout`, then panicked with `code` and
/// `message`, exit 101.
fn assert_spread_panics(name: &str, body: &str, stdout: &str, code: &str, message: &str) {
    let outcome = assert_compiled_same(&Case::file(name, &program(SPREAD, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains(&format!("error[{code}]: {message}")),
        "case `{name}`: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
}

#[test]
fn contracts_see_the_tenth_parameter_called_directly_and_through_a_value() {
    let requires = "precondition violated: `spread` requires `j > 0`";
    assert_spread_panics(
        "wide_requires",
        "    num(io, spread(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    num(io, spread(1, 2, 3, 4, 5, 6, 7, 8, 9, 0))",
        "385\n",
        "P0006",
        requires,
    );
    assert_spread_panics(
        "wide_requires_value",
        "    let f = spread
    num(io, f(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    num(io, f(1, 2, 3, 4, 5, 6, 7, 8, 9, 0))",
        "385\n",
        "P0006",
        requires,
    );
    assert_spread_panics(
        "wide_ensures",
        "    let f = spread
    num(io, f(10, 10, 10, 10, 10, 10, 10, 10, 10, 10))",
        "",
        "P0007",
        "postcondition violated: `spread` ensures `r < 500`",
    );
}
