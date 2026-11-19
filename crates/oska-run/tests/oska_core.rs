//! Programs in Oska Core (ADR 0018, #425, #426, #430): functions, `let`,
//! `if`, `Int` and `Bool`, structs, enums and `match`, and capabilities
//! with their effects, each a whole literal, so that `lean_reference.rs`
//! also runs them on the Lean reference and compares. A program without
//! capabilities shows what it computed by how it ends: `check` panics
//! with `P0004` on a false condition, and a run that returns cleanly
//! passed every check. One with capabilities prints, and writes files.
//!
//! Each program repeats its preamble (`fail`, `check`) instead of
//! building it with `format!`: `lean_reference.rs` collects only whole
//! string literals, so a program assembled at run time would not be
//! compared with the Lean reference.

mod common;

use common::{assert_panics, assert_runs, run_differential};
use oska_run::FakeHost;

#[test]
fn integer_arithmetic_rounds_toward_zero_and_compares() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn main() {
    check(2 + 3 * 4 == 14)
    check(10 - 4 - 3 == 3)
    check(-7 / 2 == -3)
    check(-7 % 2 == -1)
    check(7 % -2 == 1)
    check(-(5 - 8) == 3)
    let min = -9223372036854775807 - 1
    let minus_one = -1
    check(min < 0 && min % minus_one == 0)
    check(9223372036854775807 > 0)
    check(1 < 2 && 2 <= 2 && 3 > 2 && 3 >= 3 && 1 != 2)
    check(!(1 == 2) && true != false && true == true)
}
",
        &[],
        "",
    );
}

#[test]
fn logical_operators_evaluate_rhs_only_when_needed() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn boom() -> Bool {
    let zero = 0
    1 / zero == 0
}

fn main() {
    let a = false && boom()
    let b = true || boom()
    let c = false implies boom()
    check(!a && b && c)
    check(true implies true)
    check(!(true implies false))
    check(true && true && !(true && false))
    check(false || true)
}
",
        &[],
        "",
    );
}

#[test]
fn recursion_lets_blocks_and_ifs_compute_values() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn fact(n: Int) -> Int {
    if n <= 1 {
        1
    } else {
        n * fact(n - 1)
    }
}

fn fib(n: Int) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn sum_to(n: Int) -> Int {
    if n == 0 {
        0
    } else {
        n + sum_to(n - 1)
    }
}

fn even(n: Int) -> Bool {
    if n == 0 {
        true
    } else {
        odd(n - 1)
    }
}

fn odd(n: Int) -> Bool {
    if n == 0 {
        false
    } else {
        even(n - 1)
    }
}

fn second(_: Int, b: Int) -> Int {
    b
}

fn main() {
    check(fact(20) == 2432902008176640000)
    check(fib(15) == 610)
    check(sum_to(1000) == 500500)
    check(even(10) && odd(7) && !even(3))
    check(second(99, 7) == 7)
    let x = 1
    let x = x + 1
    let y = {
        let x = x * 10
        x + 1
    }
    check(x == 2 && y == 21)
    let z = if x > 1 {
        3
    } else {
        4
    }
    check(z == 3)
    if x == 2 {
        check(true)
    }
    let _ = fact(3)
}
",
        &[],
        "",
    );
}

#[test]
fn a_false_check_panics_with_p0004() {
    assert_panics(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn main() {
    check(1 + 1 == 3)
}
",
        "P0004",
        "1 / zero",
    );
}

#[test]
fn a_product_past_int_panics_with_p0001() {
    assert_panics(
        "fn fact(n: Int) -> Int {
    if n <= 1 {
        1
    } else {
        n * fact(n - 1)
    }
}

fn main() {
    let big = fact(21)
}
",
        "P0001",
        "n * fact(n - 1)",
    );
}

#[test]
fn the_smallest_int_divided_by_minus_one_panics_with_p0001() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let minus_one = -1
    let q = min / minus_one
}
",
        "P0001",
        "min / minus_one",
    );
}

#[test]
fn negating_the_smallest_int_panics_with_p0001() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let flipped = -min
}
",
        "P0001",
        "-min",
    );
}

#[test]
fn a_remainder_by_zero_panics_with_p0004_before_any_overflow() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let zero = 0
    let r = min % zero
}
",
        "P0004",
        "min % zero",
    );
}

#[test]
fn operands_and_arguments_run_left_to_right() {
    assert_panics(
        "fn overflow() -> Int {
    let max = 9223372036854775807
    max + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn pair(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let first = pair(overflow(), divide())
}
",
        "P0001",
        "max + 1",
    );
    assert_panics(
        "fn overflow() -> Int {
    let max = 9223372036854775807
    max + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn main() {
    let first = divide() + overflow()
}
",
        "P0004",
        "1 / zero",
    );
}

// ---- Structs, enums and `match` (#426) -----------------------------------

#[test]
fn structs_are_built_and_their_fields_read() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

struct Point { x: Int, y: Int }

struct Segment { from: Point, to: Point, solid: Bool }

fn length2(s: Segment) -> Int {
    let dx = s.to.x - s.from.x
    let dy = s.to.y - s.from.y
    dx * dx + dy * dy
}

fn origin() -> Point {
    Point(x: 0, y: 0)
}

fn main() {
    let p = Point(x: 3, y: 4)
    check(p.x == 3 && p.y == 4)
    let s = Segment(from: origin(), to: p, solid: true)
    check(length2(s) == 25)
    check(s.solid && s.from.x == 0 && s.to.y == 4)
    let q = p
    check(q.x + q.y == 7)
}
",
        &[],
        "",
    );
}

#[test]
fn match_takes_the_first_arm_whose_pattern_and_guard_hold() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

enum Shape {
    Dot,
    Square(Int),
    Rect { w: Int, h: Int },
}

fn area(s: Shape) -> Int {
    match s {
        Shape.Dot => 0
        Shape.Square(n) => n * n
        Shape.Rect(w: w, h: h) => w * h
    }
}

fn describe(s: Shape) -> Int {
    match s {
        Shape.Square(n) if n > 100 => 3
        Shape.Square(n) if n > 10 => 2
        Shape.Square(_) => 1
        Shape.Rect(w: 0, h: _) => 0
        Shape.Rect(w: _, h: 0) => 0
        _ => -1
    }
}

fn main() {
    check(area(Shape.Dot) == 0)
    check(area(Shape.Square(5)) == 25)
    check(area(Shape.Rect(w: 2, h: 7)) == 14)
    check(describe(Shape.Square(500)) == 3)
    check(describe(Shape.Square(50)) == 2)
    check(describe(Shape.Square(5)) == 1)
    check(describe(Shape.Rect(w: 0, h: 9)) == 0)
    check(describe(Shape.Rect(w: 9, h: 0)) == 0)
    check(describe(Shape.Rect(w: 9, h: 9)) == -1)
    check(describe(Shape.Dot) == -1)
}
",
        &[],
        "",
    );
}

#[test]
fn match_on_literals_nested_patterns_and_bindings() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

struct Point { x: Int, y: Int }

enum Tagged {
    At(Point),
    Flag(Bool),
    Nothing,
}

fn sign(n: Int) -> Int {
    match n {
        0 => 0
        -1 => -1
        other => if other > 0 { 1 } else { -2 }
    }
}

fn flip(b: Bool) -> Bool {
    match b {
        true => false
        false => true
    }
}

fn on_axis(t: Tagged) -> Bool {
    match t {
        Tagged.At(Point(x: 0, y: _)) => true
        Tagged.At(Point(x: _, y: 0)) => true
        Tagged.At(p) => p.x == p.y
        Tagged.Flag(b) => b
        Tagged.Nothing => false
    }
}

fn unit() -> Int {
    match () {
        () => 7
    }
}

fn main() {
    check(sign(0) == 0 && sign(-1) == -1 && sign(9) == 1 && sign(-9) == -2)
    check(flip(false) && !flip(true))
    check(on_axis(Tagged.At(Point(x: 0, y: 3))))
    check(on_axis(Tagged.At(Point(x: 3, y: 0))))
    check(on_axis(Tagged.At(Point(x: 2, y: 2))))
    check(!on_axis(Tagged.At(Point(x: 1, y: 2))))
    check(on_axis(Tagged.Flag(true)) && !on_axis(Tagged.Flag(false)))
    check(!on_axis(Tagged.Nothing))
    check(unit() == 7)
}
",
        &[],
        "",
    );
}

#[test]
fn a_recursive_enum_is_built_and_folded() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

enum Ints {
    Empty,
    Cons(Int, Ints),
}

fn range(from: Int, to: Int) -> Ints {
    if from >= to {
        Ints.Empty
    } else {
        Ints.Cons(from, range(from + 1, to))
    }
}

fn sum(xs: Ints) -> Int {
    match xs {
        Ints.Empty => 0
        Ints.Cons(x, rest) => x + sum(rest)
    }
}

fn evens(xs: Ints) -> Int {
    match xs {
        Ints.Empty => 0
        Ints.Cons(x, rest) if x % 2 == 0 => 1 + evens(rest)
        Ints.Cons(_, rest) => evens(rest)
    }
}

fn main() {
    let xs = range(0, 100)
    check(sum(xs) == 4950)
    check(evens(xs) == 50)
    check(sum(Ints.Empty) == 0)
}
",
        &[],
        "",
    );
}

#[test]
fn a_guard_that_panics_panics_before_any_later_arm() {
    assert_panics(
        "fn main() {
    let zero = 0
    let n = match 5 {
        x if x / zero == 0 => 1
        _ => 2
    }
}
",
        "P0004",
        "x / zero",
    );
}

/// The checker takes a struct's labels only in declaration order
/// (`E0225`), so the order written is the declaration order, and the
/// fields are built in it, left to right.
#[test]
fn fields_are_built_left_to_right_in_declaration_order() {
    assert_panics(
        "struct Pair { a: Int, b: Int }

fn overflow() -> Int {
    let max = 9223372036854775807
    max + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn main() {
    let p = Pair(a: divide(), b: overflow())
}
",
        "P0004",
        "1 / zero",
    );
}

#[test]
fn an_arm_body_that_overflows_panics_with_p0001() {
    assert_panics(
        "enum Op {
    Double(Int),
    Keep(Int),
}

fn apply(op: Op) -> Int {
    match op {
        Op.Double(n) => n * 2
        Op.Keep(n) => n
    }
}

fn main() {
    let big = apply(Op.Keep(9223372036854775807))
    let bigger = apply(Op.Double(big))
}
",
        "P0001",
        "n * 2",
    );
}

// ---- Capabilities ------------------------------------------------------

#[test]
fn main_prints_through_the_stdout_it_receives() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn say(io: Stdout, line: Untrusted<String>) {
    let _ = io.println(line)
}

fn main(io: Stdout) {
    say(io, "hello")
    let _ = io.println("a \"quoted\" line\twith a tab and a \\")
    let _ = io.println("")
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(
        outcome.stdout,
        "hello\na \"quoted\" line\twith a tab and a \\\n\n"
    );
    let world = host.world();
    assert!(
        world.files.is_empty(),
        "printing writes no file: {world:#?}"
    );
    assert!(
        world.dirs.is_empty(),
        "printing makes no directory: {world:#?}"
    );
}

#[test]
fn a_directory_capability_writes_reads_and_creates_directories_beneath_its_root() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn save(dir: DirWrite, name: Untrusted<String>, text: Untrusted<String>) -> Bool {
    dir.write(Path.from(name), text).is_ok()
}

fn main(io: Stdout, fs: Fs) {
    let root = fs.restrict_to(".")
    let out = root.write_only()
    let made = out.create_dir(Path.from("box"))
    let again = out.create_dir(Path.new("box"))
    let first = save(out, "box/a.txt", "first")
    let second = save(out, "box/a.txt", "second")
    let top = save(out.restrict_to(Path.from("box")), "../top.txt", "top")
    let deep = save(out, "box/missing/a.txt", "deep")
    let ok = made.is_ok() && again.is_ok() && first && second && !top && !deep
    if ok {
        let _ = io.println("saved")
    }
    let read = root.read_only()
    let _ = io.println(read.read_to_string(Path.from("box/a.txt")).unwrap_or("missing"))
    let _ = io.println(read.read_to_string(Path.from("box/b.txt")).unwrap_or("missing"))
    let _ = io.println(read.read_to_string(Path.from("box")).unwrap_or("not a file"))
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "saved\nsecond\nmissing\nnot a file\n");
    assert_eq!(host.file("box/a.txt"), Some("second"));
    assert_eq!(host.file("top.txt"), None);
}

#[test]
fn a_confined_capability_refuses_a_path_outside_its_root() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn report(io: Stdout, ok: Bool, what: Untrusted<String>) {
    if ok {
        let _ = io.println(what)
    } else {
        let _ = io.println("refused")
    }
}

fn main(io: Stdout, write: FsWrite) {
    let root = write.restrict_to(".")
    let _ = root.create_dir(Path.from("box"))
    let boxed = root.restrict_to(Path.from("box"))
    report(io, boxed.write(Path.from("inside.txt"), "kept").is_ok(), "inside")
    report(io, boxed.write(Path.from("../escape.txt"), "leaked").is_ok(), "escaped")
    report(io, boxed.write(Path.from("/escape.txt"), "leaked").is_ok(), "escaped")
    report(io, boxed.write(Path.from("sub/../../escape.txt"), "leaked").is_ok(), "escaped")
    report(io, boxed.create_dir(Path.from("../out")).is_ok(), "escaped")
    let above = boxed.restrict_to(Path.from(".."))
    report(io, above.write(Path.from("escape.txt"), "leaked").is_ok(), "escaped")
    let same = boxed.restrict_to(Path.from("."))
    report(io, same.write(Path.from("again.txt"), "kept").is_ok(), "inside again")
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(
        outcome.stdout,
        "inside\nrefused\nrefused\nrefused\nrefused\nrefused\ninside again\n"
    );
    assert_eq!(host.file("box/inside.txt"), Some("kept"));
    assert_eq!(host.file("box/again.txt"), Some("kept"));
    assert_eq!(host.file("escape.txt"), None);
}

#[test]
fn a_narrowed_read_capability_reads_only_beneath_its_root() {
    assert_runs(
        r#"fn main(io: Stdout, fs: Fs) {
    let root = fs.restrict_to(".")
    let _ = root.write_only().write(Path.from("secret.txt"), "secret")
    let _ = root.write_only().create_dir(Path.from("notes"))
    let _ = root.write_only().write(Path.from("notes/readme.txt"), "readme")
    let notes = root.read_only().restrict_to(Path.from("notes"))
    let _ = io.println(notes.read_to_string(Path.from("readme.txt")).unwrap_or("refused"))
    let _ = io.println(notes.read_to_string(Path.from("../secret.txt")).unwrap_or("refused"))
    let _ = io.println(notes.read_to_string(Path.from("sub/../readme.txt")).unwrap_or("refused"))
    let _ = io.println(notes.read_to_string(Path.from("")).unwrap_or("refused"))
    let missing = fs.read_only().restrict_to("nowhere")
    let _ = io.println(missing.read_to_string(Path.from("x")).unwrap_or("refused"))
    let file = fs.restrict_to("secret.txt")
    let _ = io.println(file.read_only().read_to_string(Path.from("x")).unwrap_or("refused"))
}
"#,
        &[],
        "readme\nrefused\nreadme\nrefused\nrefused\nrefused\n",
    );
}

#[test]
fn files_written_before_a_panic_are_kept() {
    let mut host = FakeHost::new();
    let src = r#"fn main(io: Stdout, fs: FsWrite) {
    let out = fs.restrict_to(".")
    let _ = out.write(Path.from("before.txt"), "written")
    let _ = io.println("before")
    let zero = 0
    let _ = 1 / zero
    let _ = io.println("after")
}
"#;
    let outcome = run_differential(&mut host, src, &[]);
    let Some(panic) = &outcome.panic else {
        panic!("{src:?} should panic with P0004: {outcome:#?}");
    };
    assert_eq!(panic.code, "P0004", "{panic:#?}");
    assert_eq!(&src[panic.span.start..panic.span.end], "1 / zero");
    assert_eq!(outcome.exit_code, 101, "a panic exits with code 101");
    assert_eq!(outcome.stdout, "before\n");
    assert_eq!(host.file("before.txt"), Some("written"));
}
