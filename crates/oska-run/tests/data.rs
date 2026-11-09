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

// ---- Mutating methods and value semantics -----------------------------------

#[test]
fn push_updates_the_var_it_is_called_on() {
    assert_runs(
        &main_with(
            "    var xs: List<Int> = List.new()
    for i in 0..3 {
        xs.push(i * 10)
    }
    let n = xs.len()
    let last = xs.at(2)
    io.println(\"{n} {last}\")",
        ),
        &[],
        "3 20\n",
    );
}

#[test]
fn pushing_onto_a_copy_never_changes_the_original() {
    assert_runs(
        &main_with(
            "    var xs = [1]
    let before = xs
    xs.push(2)
    var after = xs
    after.push(3)
    let a = before.len()
    let b = xs.len()
    let c = after.len()
    io.println(\"{a} {b} {c}\")",
        ),
        &[],
        "1 2 3\n",
    );
}

const ACCOUNT: &str = "struct Account { balance: Int, history: List<Int> }

impl Account {
    fn open() -> Account {
        Account(balance: 0, history: List.new())
    }

    fn deposit(var self, amount: Int) -> Int {
        self.balance = self.balance + amount
        self.history.push(amount)
        self.balance
    }

    fn balance(self) -> Int {
        self.balance
    }
}

struct Bank { main: Account, spare: Account }";

#[test]
fn a_mutating_method_writes_its_receiver_back_to_the_var() {
    assert_runs(
        &program(
            ACCOUNT,
            "    var a = Account.open()
    let first = a.deposit(5)
    let second = a.deposit(7)
    let total = a.balance()
    let n = a.history.len()
    io.println(\"{first} {second} {total} {n}\")",
        ),
        &[],
        "5 12 12 2\n",
    );
}

#[test]
fn a_mutating_method_on_a_field_writes_back_through_the_struct() {
    assert_runs(
        &program(
            ACCOUNT,
            "    var bank = Bank(main: Account.open(), spare: Account.open())
    let _ = bank.main.deposit(3)
    bank.spare.balance = 40
    let m = bank.main.balance()
    let s = bank.spare.balance()
    io.println(\"{m} {s}\")",
        ),
        &[],
        "3 40\n",
    );
}

#[test]
fn mutating_a_copy_of_a_struct_never_changes_the_original() {
    assert_runs(
        &program(
            ACCOUNT,
            "    var a = Account.open()
    let _ = a.deposit(1)
    let saved = a
    var bank = Bank(main: a, spare: a)
    let _ = a.deposit(10)
    let _ = bank.main.deposit(100)
    bank.spare.balance = 1000
    let s = saved.balance()
    let n = saved.history.len()
    let x = a.balance()
    let m = bank.main.balance()
    let p = bank.spare.balance()
    io.println(\"{s} {n} {x} {m} {p}\")",
        ),
        &[],
        "1 1 11 101 1000\n",
    );
}

// ---- Closures --------------------------------------------------------------

#[test]
fn a_closure_captures_values_when_it_is_made_and_can_be_called() {
    assert_runs(
        &program(
            "fn twice(f: fn(Int) -> Int, x: Int) -> Int {
    f(f(x))
}",
            "    var step = 3
    let add = |x: Int| x + step
    step = 100
    let a = add(1)
    let b = twice(add, 10)
    let c = twice(|x| x * 2, 5)
    io.println(\"{a} {b} {c}\")",
        ),
        &[],
        "4 16 20\n",
    );
}

#[test]
fn a_captured_list_keeps_its_value_when_the_var_is_mutated_later() {
    assert_runs(
        &main_with(
            "    var xs = [1]
    let count = || xs.len()
    xs.push(2)
    let captured = count()
    let now = xs.len()
    io.println(\"{captured} {now}\")",
        ),
        &[],
        "1 2\n",
    );
}

#[test]
fn list_map_calls_the_closure_on_each_item() {
    assert_runs(
        &main_with(
            "    let offset = 100
    let ys = [1, 2, 3].map(|x| x + offset)
    let a = ys.at(0)
    let c = ys.at(2)
    let n = ys.len()
    io.println(\"{a} {c} {n}\")",
        ),
        &[],
        "101 103 3\n",
    );
}
