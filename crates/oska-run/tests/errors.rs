//! Errors and generics at runtime (#63): `Option` and `Result`, `?` with
//! `From` conversions, `main` returning `Err`, trait calls, and generic
//! code dispatching on each value's runtime type tag (ADR 0007).

mod common;

use common::{assert_exits, assert_refused, assert_runs, run_fake, run_on};
use oska_run::FakeHost;

// ---- `?` and `main` returning `Err` ----------------------------------------

const HALF: &str = "fn half(n: Int) -> Result<Int, String> {
    if n % 2 == 0 { Ok(n / 2) } else { Err(\"{n} is odd\") }
}
";

#[test]
fn question_mark_unwraps_ok_and_main_returning_err_prints_it_and_exits_1() {
    let src = format!(
        "{HALF}
fn main(io: Stdout) -> Result<(), String> {{
    let a = half(8)?
    let _ = io.println(\"half of 8 is {{a}}\")
    let b = half(3)?
    let _ = io.println(\"unreachable {{b}}\")
    Ok(())
}}
"
    );
    assert_exits(&src, 1, "3 is odd\n");
    assert_eq!(run_fake(&src, &[]).stdout, "half of 8 is 4\n");
}

const APP_ERROR: &str = "struct ParseError { text: String }

struct RangeError { n: Int }

enum AppError {
    Parse(String),
    Range(Int),
}

impl From<ParseError> for AppError {
    fn from(err: ParseError) -> AppError { AppError.Parse(err.text) }
}

impl From<RangeError> for AppError {
    fn from(err: RangeError) -> AppError { AppError.Range(err.n) }
}

impl Display for AppError {
    fn to_string(self) -> String {
        match self {
            AppError.Parse(text) => \"not a number: {text}\"
            AppError.Range(n) => \"out of range: {n}\"
        }
    }
}

fn parse(text: String) -> Result<Int, ParseError> {
    match text.parse_int() {
        Some(n) => Ok(n)
        None => Err(ParseError(text: text))
    }
}

fn small(n: Int) -> Result<Int, RangeError> {
    if n < 10 { Ok(n) } else { Err(RangeError(n: n)) }
}

fn read(text: String) -> Result<Int, AppError> {
    let n = parse(text)?
    let m = small(n)?
    Ok(m)
}
";

/// A `main` that reads `text` through [`APP_ERROR`]'s `read`.
fn read_main(text: &str) -> String {
    format!(
        "{APP_ERROR}
fn main(io: Stdout) -> Result<(), AppError> {{
    let n = read(\"{text}\")?
    let _ = io.println(\"read {{n}}\")
    Ok(())
}}
"
    )
}

#[test]
fn question_mark_converts_each_error_through_the_from_impl_for_its_type() {
    assert_runs(&read_main("7"), &[], "read 7\n");
    assert_exits(&read_main("x"), 1, "not a number: x\n");
    assert_exits(&read_main("12"), 1, "out of range: 12\n");
}

#[test]
fn question_mark_on_an_option_returns_none_from_the_function_or_closure() {
    assert_runs(
        &program(
            "fn sum_first_two(xs: List<Int>) -> Option<Int> {
    let a = xs.get(0)?
    let b = xs.get(1)?
    Some(a + b)
}",
            "    let pick = |xs: List<Int>| Some(xs.get(2)? * 10)
    let a = sum_first_two([3, 4]).unwrap_or(-1)
    let b = sum_first_two([3]).unwrap_or(-1)
    let c = pick([1, 2, 3]).unwrap_or(-1)
    let d = pick([1]).unwrap_or(-1)
    io.println(\"{a} {b} {c} {d}\")",
        ),
        &[],
        "7 -1 30 -1\n",
    );
}

// ---- Trait calls and generic dispatch --------------------------------------

const POINT: &str = "struct Point { x: Int, y: Int }

impl Display for Point {
    fn to_string(self) -> String {
        let x = self.x
        let y = self.y
        \"({x}, {y})\"
    }
}

impl Eq for Point {
    fn eq(self, other: Point) -> Bool { self.x == other.x }
}
";

/// A program of `items` and a `main` that runs `body`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

#[test]
fn generic_code_displays_each_value_through_the_impl_for_its_type() {
    let items = format!(
        "{POINT}
fn show<T: Display>(label: String, value: T) -> Untrusted<String> {{
    let text = value.to_string()
    \"{{label}} {{value}} {{text}}\"
}}"
    );
    assert_runs(
        &program(
            &items,
            "    let _ = io.println(show(\"point\", Point(x: 1, y: 2)))
    let _ = io.println(show(\"int\", 7))
    io.println(show(\"text\", \"hi\"))",
        ),
        &[],
        "point (1, 2) (1, 2)\nint 7 7\ntext hi hi\n",
    );
}

#[test]
fn generic_code_compares_values_through_the_eq_impl_for_their_type() {
    let items = format!(
        "{POINT}
fn same<T: Eq>(a: T, b: T) -> String {{
    if a == b && a.eq(b) {{ \"same\" }} else if a != b {{ \"different\" }} else {{ \"mixed\" }}
}}"
    );
    // `Point`'s own `Eq` compares only `x`.
    assert_runs(
        &program(
            &items,
            "    let _ = io.println(same(Point(x: 1, y: 2), Point(x: 1, y: 9)))
    let _ = io.println(same(Point(x: 1, y: 2), Point(x: 3, y: 2)))
    let _ = io.println(same(4, 4))
    io.println(same(\"a\", \"b\"))",
        ),
        &[],
        "same\ndifferent\nsame\ndifferent\n",
    );
}

#[test]
fn options_and_lists_compare_their_items_through_eq_for_the_item_type() {
    // `Point`'s own `Eq` compares only `x`.
    assert_runs(
        &program(
            POINT,
            "    let a = Some(Point(x: 1, y: 2))
    let b = Some(Point(x: 1, y: 5))
    let none: Option<Point> = None
    let _ = io.println(if a == b { \"equal\" } else { \"unequal\" })
    let _ = io.println(if a == none { \"equal\" } else { \"unequal\" })
    let _ = io.println(if [a, b] == [b, a] { \"equal\" } else { \"unequal\" })
    io.println(if [1, 2] != [1, 2, 3] { \"unequal\" } else { \"equal\" })",
        ),
        &[],
        "equal\nunequal\nequal\nunequal\n",
    );
}

#[test]
fn an_io_error_displays_as_its_message() {
    assert_exits(
        "fn main(io: Stdout) -> Result<(), IoError> {
    let e = IoError(message: \"disk full\")
    let _ = io.println(\"failed: {e}\")
    Err(e)
}
",
        1,
        "disk full\n",
    );
    assert_runs(
        "fn main(io: Stdout) -> Result<(), IoError> {
    let e = IoError(message: \"disk full\")
    let text = e.to_string()
    io.println(\"failed: {e} ({text})\")
}
",
        &[],
        "failed: disk full (disk full)\n",
    );
}

const SHAPES: &str = "trait Shape {
    fn area(self) -> Int
    fn describe(self) -> String {
        let a = self.area()
        \"a shape of area {a}\"
    }
}

struct Square { side: Int }

struct Rect { w: Int, h: Int }

impl Shape for Square {
    fn area(self) -> Int { self.side * self.side }
}

impl Shape for Rect {
    fn area(self) -> Int { self.w * self.h }
    fn describe(self) -> String { \"a rectangle\" }
}

fn report<T: Shape>(shape: T) -> Untrusted<String> {
    let a = shape.area()
    let d = shape.describe()
    \"{d}, {a}\"
}
";

#[test]
fn a_user_trait_dispatches_to_the_impl_for_each_type_with_its_default_methods() {
    assert_runs(
        &program(
            SHAPES,
            "    let _ = io.println(report(Square(side: 3)))
    let _ = io.println(report(Rect(w: 2, h: 5)))
    let sq = Square(side: 2)
    let a = sq.area()
    let d = sq.describe()
    io.println(\"{d} ({a})\")",
        ),
        &[],
        "a shape of area 9, 9\na rectangle, 10\na shape of area 4 (4)\n",
    );
}

#[test]
fn generic_dispatch_on_a_sized_integer_type_with_its_own_impl_is_refused() {
    // Every integer carries the tag `Int` at runtime, so the impl for
    // `U8` could not be told from the one for `Int`.
    let impls = "trait Name {
    fn name(self) -> String
}

impl Name for Int {
    fn name(self) -> String { \"int\" }
}

impl Name for U8 {
    fn name(self) -> String { \"u8\" }
}
";
    assert_refused(
        &program(
            &format!("{impls}\nfn show<T: Name>(x: T) -> Untrusted<String> {{ x.name() }}"),
            "    let b: U8 = 3\n    io.println(show(b))",
        ),
        "E0380",
    );
    // Called on a known type, the impl is chosen before the program runs.
    assert_runs(
        &program(impls, "    let b: U8 = 3\n    io.println(b.name())"),
        &[],
        "u8\n",
    );
}

// ---- Functions as values ---------------------------------------------------

#[test]
fn a_named_function_is_a_value_that_can_be_called_and_passed() {
    assert_runs(
        &program(
            "fn double(n: Int) -> Int { n * 2 }

struct Counter { n: Int }

impl Counter {
    fn make(n: Int) -> Counter { Counter(n: n) }
}

fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }",
            "    let f = double
    let a = f(4)
    let b = apply(double, 5)
    let xs = [1, 2, 3].map(double)
    let total = xs.at(0) + xs.at(1) + xs.at(2)
    let make = Counter.make
    let c = make(7).n
    io.println(\"{a} {b} {total} {c}\")",
        ),
        &[],
        "8 10 12 7\n",
    );
}

// ---- Option and Result -----------------------------------------------------

#[test]
fn ok_or_turns_an_option_into_a_result() {
    assert_runs(
        &program(
            "fn first(xs: List<Int>) -> Result<Int, String> {
    xs.get(0).ok_or(\"empty\")
}

fn show(r: Result<Int, String>) -> String {
    match r {
        Ok(n) => \"ok {n}\"
        Err(e) => \"err {e}\"
    }
}",
            "    let _ = io.println(show(first([4, 5])))
    io.println(show(first(List.new())))",
        ),
        &[],
        "ok 4\nerr empty\n",
    );
}

// ---- The capabilities example ----------------------------------------------

const CAPABILITIES: &str = include_str!("../../../examples/capabilities.osk");

#[test]
fn the_capabilities_example_counts_the_lines_of_a_file_in_the_fake_filesystem() {
    let host = FakeHost::new().with_file("notes.txt", "one\ntwo\nthree\n");
    let outcome = run_on(host, CAPABILITIES, &["notes.txt"]);
    assert_eq!(
        (outcome.stdout.as_str(), outcome.exit_code),
        ("lines: 3\n", 0),
        "{outcome:#?}"
    );
}

#[test]
fn the_capabilities_example_reports_a_missing_argument_or_file_and_exits_1() {
    let outcome = run_on(FakeHost::new(), CAPABILITIES, &[]);
    assert_eq!(
        (outcome.stderr.as_str(), outcome.exit_code),
        ("usage: capabilities <file>\n", 1),
        "{outcome:#?}"
    );
    let outcome = run_on(FakeHost::new(), CAPABILITIES, &["missing.txt"]);
    assert_eq!(
        (outcome.stderr.as_str(), outcome.exit_code),
        ("cannot read the file\n", 1),
        "{outcome:#?}"
    );
}
