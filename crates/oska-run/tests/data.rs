//! Data at runtime (#62): structs, enums, `match`, `List`, closures,
//! strings, and mutating methods under value semantics (ADR 0001).

mod common;

use common::{assert_panics, assert_runs};

/// A `main` that runs `body` with `io: Stdout` in scope.
fn main_with(body: &str) -> String {
    format!("fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// A program of `items` and a `main` that runs `body`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\n{}", main_with(body))
}

// ---- Structs ---------------------------------------------------------------

#[test]
fn a_struct_is_constructed_and_its_fields_read() {
    assert_runs(
        &program(
            "struct Point { x: Int, y: Int }",
            "    let p = Point(x: 3, y: 4)\n    let sum = p.x + p.y\n    io.println(\"{sum} {p.y}\")",
        ),
        &[],
        "7 4\n",
    );
}

// ---- Enums and match -------------------------------------------------------

const SHAPE: &str = "enum Shape {
    Circle { radius: Int },
    Square(Int),
    Empty,
}

fn describe(s: Shape) -> String {
    match s {
        Shape.Circle(radius: r) if r > 100 => \"big circle\"
        Shape.Circle(radius: _) => \"circle\"
        Shape.Square(n) => \"square of {n}\"
        Shape.Empty => \"nothing\"
    }
}";

#[test]
fn match_picks_the_first_arm_whose_pattern_and_guard_match() {
    assert_runs(
        &program(
            SHAPE,
            "    let _ = io.println(describe(Shape.Circle(radius: 500)))
    let _ = io.println(describe(Shape.Circle(radius: 5)))
    let _ = io.println(describe(Shape.Square(2)))
    io.println(describe(Shape.Empty))",
        ),
        &[],
        "big circle\ncircle\nsquare of 2\nnothing\n",
    );
}

#[test]
fn match_handles_literal_struct_nested_and_binding_patterns() {
    assert_runs(
        &program(
            "struct Point { x: Int, y: Int }

fn sign(n: Int) -> String {
    match n {
        0 => \"zero\"
        -1 => \"minus one\"
        other => \"other {other}\"
    }
}

fn word(s: String) -> Int {
    match s {
        \"one\" => 1
        _ => 0
    }
}

fn flag(b: Bool) -> Int {
    match b {
        true => 1
        false => 0
    }
}

fn inner(o: Option<Option<Int>>) -> Int {
    match o {
        Some(Some(n)) => n
        Some(None) => -1
        None => -2
    }
}

fn on_axis(p: Point) -> Bool {
    match p {
        Point(x: 0, y: _) => true
        Point(x: _, y: 0) => true
        _ => false
    }
}",
            "    let a = sign(0)
    let b = sign(-1)
    let c = sign(7)
    let d = word(\"one\") + word(\"two\") + flag(true) + flag(false)
    let e = inner(Some(Some(5))) + inner(Some(None)) + inner(None)
    let f = on_axis(Point(x: 0, y: 3)) && on_axis(Point(x: 3, y: 0)) && !on_axis(Point(x: 1, y: 1))
    let unit = match () {
        () => \"unit\"
    }
    if f {
        let _ = io.println(\"axes ok\")
    }
    io.println(\"{a}, {b}, {c}, {d}, {e}, {unit}\")",
        ),
        &[],
        "axes ok\nzero, minus one, other 7, 2, 2, unit\n",
    );
}

// ---- Lists -----------------------------------------------------------------

#[test]
fn a_list_literal_has_a_length_and_items_by_index() {
    assert_runs(
        &program(
            "fn show(o: Option<Int>) -> String {
    match o {
        Some(n) => \"{n}\"
        None => \"none\"
    }
}",
            "    let xs = [10, 20, 30]
    let n = xs.len()
    let first = show(xs.get(0))
    let past = show(xs.get(3))
    let below = show(xs.get(-1))
    var total = 0
    for x in xs {
        total = total + x
    }
    io.println(\"{n} {first} {past} {below} {total}\")",
        ),
        &[],
        "3 10 none none 60\n",
    );
}

#[test]
fn indexing_a_list_reads_the_item_at_that_index() {
    assert_runs(
        &main_with("    let xs = [\"a\", \"b\"]\n    let b = xs.at(1)\n    io.println(b)"),
        &[],
        "b\n",
    );
}

#[test]
fn indexing_past_the_end_of_a_list_panics_with_p0002() {
    assert_panics(
        &main_with("    let xs = [1, 2]\n    let n = xs.at(2)\n    io.println(\"{n}\")"),
        "P0002",
        "xs.at(2)",
    );
    assert_panics(
        &main_with("    let xs = [1, 2]\n    let n = xs.at(-1)\n    io.println(\"{n}\")"),
        "P0002",
        "xs.at(-1)",
    );
}
