// SPDX-License-Identifier: Apache-2.0
//! Programs in Oska Core (ADR 0018, #425 to #431), continued from
//! `oska_core.rs`: closures, `Result` with `?`, `var`, assignment and
//! methods, mutating ones included, and `scope` with `spawn` and `join`.
//! Each program is a whole literal, so that `lean_reference.rs` also runs
//! them on the Lean reference and compares, and each repeats its preamble
//! instead of building it with `format!`.

mod common;

use common::{assert_panics, assert_runs, run_differential};
use oska_run::FakeHost;

// ---- Closures ----------------------------------------------------------

#[test]
fn closures_capture_the_values_they_read_and_are_called() {
    assert_runs(
        "enum Shape {
    Dot,
    Square(Int),
}

struct Pair { a: Int, b: Int }

fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn twice(f: fn(Int) -> Int, x: Int) -> Int {
    f(f(x))
}

fn adder(n: Int) -> fn(Int) -> Int {
    |x: Int| x + n
}

fn compose(f: fn(Int) -> Int, g: fn(Int) -> Int) -> fn(Int) -> Int {
    |x: Int| g(f(x))
}

fn pick(double: Bool) -> fn(Int) -> Int {
    if double {
        |x: Int| x * 2
    } else {
        |x: Int| x + 1
    }
}

fn main() {
    let step = 3
    let add = |x: Int| x + step
    check(add(1) == 4)
    check(twice(add, 10) == 16)
    check(twice(|x| x * 2, 5) == 20)
    let add_ten = adder(10)
    let add_one = adder(1)
    check(add_ten(2) == 12 && add_one(2) == 3)
    check(compose(add_ten, pick(true))(1) == 22)
    check(pick(false)(1) == 2)
    let unused = 100
    let nested = |a: Int, _: Bool| {
        let inner = |b: Int| a + b + step
        inner(a)
    }
    check(nested(4, true) == 11)
    let pair = Pair(a: 5, b: 7)
    let area = |s: Shape| match s {
        Shape.Dot => pair.a
        Shape.Square(n) if n > 0 => n * n + pair.b
        Shape.Square(_) => 0
    }
    check(area(Shape.Dot) == 5 && area(Shape.Square(3)) == 16 && area(Shape.Square(-1)) == 0)
    let constant = || unused + step
    check(constant() == 103)
}
",
        &[],
        "",
    );
}

#[test]
fn a_closure_that_captured_stdout_prints_when_it_is_called() {
    assert_runs(
        r#"fn apply(f: cap fn() -> Result<(), IoError>) -> Result<(), IoError> {
    f()
}

fn main(io: Stdout) {
    let greet = |line: Untrusted<String>| io.println(line)
    let _ = greet("first")
    let again = || greet("second")
    let _ = apply(again)
    let _ = apply(|| io.println("third"))
    let quiet = || 1 + 1
    if quiet() == 2 {
        let _ = io.println("done")
    }
}
"#,
        &[],
        "first\nsecond\nthird\ndone\n",
    );
}

#[test]
fn a_closure_that_captured_a_directory_capability_writes_beneath_its_root() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn save_all(save: cap fn(Untrusted<String>) -> Bool) -> Bool {
    save("a.txt") && save("../escape.txt")
}

fn main(io: Stdout, fs: FsWrite) {
    let root = fs.restrict_to(".")
    let _ = root.create_dir(Path.from("box"))
    let boxed = root.restrict_to(Path.from("box"))
    let save = |name: Untrusted<String>| boxed.write(Path.from(name), "saved").is_ok()
    if save_all(save) {
        let _ = io.println("all saved")
    } else {
        let _ = io.println("refused")
    }
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "refused\n");
    assert_eq!(host.file("box/a.txt"), Some("saved"));
    assert_eq!(host.file("escape.txt"), None);
}

#[test]
fn a_call_of_a_closure_runs_the_callee_then_its_arguments_left_to_right() {
    assert_panics(
        "fn overflow() -> Int {
    let big = 9223372036854775807
    big + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn main() {
    let pair = |a: Int, b: Int| a + b
    let _ = pair(overflow(), divide())
}
",
        "P0001",
        "big + 1",
    );
    assert_panics(
        "fn overflowing() -> fn(Int) -> Int {
    let big = 9223372036854775807
    let _ = big + 1
    |x: Int| x
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn main() {
    let _ = overflowing()(divide())
}
",
        "P0001",
        "big + 1",
    );
}

#[test]
fn a_closure_body_that_divides_by_zero_panics_with_p0004() {
    assert_panics(
        "fn main() {
    let n = 7
    let f = |x: Int| x / (x - n)
    let ok = f(8)
    let _ = f(7)
}
",
        "P0004",
        "x / (x - n)",
    );
}

#[test]
fn results_are_built_matched_and_passed_on_by_the_question_mark() {
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

struct AppError { code: Int }

impl From<Int> for AppError {
    fn from(code: Int) -> AppError { AppError(code: code + 100) }
}

fn half(n: Int) -> Result<Int, Int> {
    if n % 2 == 0 { Ok(n / 2) } else { Err(n) }
}

fn quarter(n: Int) -> Result<Int, Int> {
    let h = half(n)?
    half(h)
}

fn app_quarter(n: Int) -> Result<Int, AppError> {
    let h = half(n)?
    Ok(half(h)?)
}

fn code(r: Result<Int, AppError>) -> Int {
    match r {
        Ok(n) => n
        Err(AppError(code: c)) => 0 - c
    }
}

fn failed_with(r: Result<Int, Int>, e: Int) -> Bool {
    match r {
        Ok(_) => false
        Err(n) => n == e
    }
}

fn main() {
    check(quarter(12).unwrap_or(0) == 3)
    check(quarter(6).unwrap_or(0) == 0)
    check(quarter(7).is_ok() == false)
    check(failed_with(quarter(6), 3))
    check(failed_with(quarter(7), 7))
    check(code(app_quarter(12)) == 3)
    check(code(app_quarter(6)) == -103)
    check(code(app_quarter(5)) == -105)
    let nested: Result<Result<Int, Int>, Bool> = Ok(half(3))
    check(match nested {
        Ok(Ok(_)) => false
        Ok(Err(n)) => n == 3
        Err(_) => false
    })
}
",
        &[],
        "",
    );
}

#[test]
fn a_question_mark_in_a_closure_returns_from_the_closure() {
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

fn half(n: Int) -> Result<Int, Int> {
    if n % 2 == 0 { Ok(n / 2) } else { Err(n) }
}

fn apply(f: fn(Int) -> Result<Int, Int>, n: Int) -> Int {
    let r = f(n)
    match r {
        Ok(v) => v
        Err(e) => 1000 + e
    }
}

fn main() {
    let offset = 1
    let eighth = |n: Int| {
        let q = half(half(n)?)?
        let r = half(q)?
        Ok(r + offset)
    }
    check(apply(eighth, 16) == 3)
    check(apply(eighth, 12) == 1003)
    check(apply(eighth, 3) == 1003)
    check(eighth(8).unwrap_or(0) == 2)
}
",
        &[],
        "",
    );
}

#[test]
fn a_question_mark_converts_the_error_before_it_returns() {
    assert_panics(
        "struct Wrapped { code: Int }

impl From<Int> for Wrapped {
    fn from(code: Int) -> Wrapped {
        let zero = 0
        Wrapped(code: code / zero)
    }
}

fn half(n: Int) -> Result<Int, Int> {
    if n % 2 == 0 { Ok(n / 2) } else { Err(n) }
}

fn quarter(n: Int) -> Result<Int, Wrapped> {
    let h = half(n)?
    Ok(half(h)?)
}

fn main() {
    let fine = quarter(4)
    let _ = quarter(6)
}
",
        "P0004",
        "code / zero",
    );
}

#[test]
fn an_io_error_is_passed_on_by_the_question_mark() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn copy(dir: DirWrite, read: DirRead, from: Untrusted<String>, to: Untrusted<String>) -> Result<(), IoError> {
    let text = read.read_to_string(Path.from(from))?
    dir.write(Path.from(to), text)?
    dir.write(Path.from("done.txt"), "done")
}

fn report(io: Stdout, r: Result<(), IoError>) {
    match r {
        Ok(_) => {
            let _ = io.println("copied")
        }
        Err(_) => {
            let _ = io.println("refused")
        }
    }
}

fn main(io: Stdout, fs: Fs) {
    let root = fs.restrict_to(".")
    let dir = root.write_only()
    let read = root.read_only()
    let _ = dir.write(Path.from("a.txt"), "hello")
    report(io, copy(dir, read, "a.txt", "b.txt"))
    report(io, copy(dir, read, "missing.txt", "c.txt"))
    report(io, copy(dir.restrict_to(Path.from("box")), read, "a.txt", "d.txt"))
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "copied\nrefused\nrefused\n");
    assert_eq!(host.file("b.txt"), Some("hello"));
    assert_eq!(host.file("done.txt"), Some("done"));
    assert_eq!(host.file("c.txt"), None);
}

#[test]
fn a_var_is_assigned_and_its_fields_changed() {
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

struct Segment { from: Point, to: Point }

fn main() {
    var n = 1
    n = n + 1
    check(n == 2)
    var s = Segment(from: Point(x: 0, y: 0), to: Point(x: 3, y: 4))
    let before = s
    s.to.y = s.to.y * 10
    s.from = Point(x: 1, y: 1)
    check(s.to.y == 40 && s.from.x == 1 && s.to.x == 3)
    check(before.to.y == 4 && before.from.x == 0)
    var _ = n
    var m = n
    m = m + 5
    check(n == 2 && m == 7)
    var inner = 0
    let total = {
        var acc = 10
        acc = acc + n
        inner = acc
        acc
    }
    check(total == 12 && inner == 12)
}
",
        &[],
        "",
    );
}

#[test]
fn a_mutating_method_changes_its_receiver_and_no_copy_of_it() {
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

struct Counter { n: Int }

struct Pair { left: Counter, right: Counter }

impl Counter {
    fn new() -> Counter { Counter(n: 0) }

    fn get(self) -> Int { self.n }

    fn bump(var self) {
        self.n = self.n + 1
    }

    fn add(var self, by: Int) -> Int {
        var step = by
        self.n = self.n + step
        step = step + 1
        self.n
    }

    fn bump_twice(var self) {
        self.bump()
        self.bump()
    }
}

fn bumped(c: Counter) -> Counter {
    var copy = c
    copy.bump()
    copy
}

fn main() {
    var a = Counter.new()
    let b = a
    a.bump()
    check(a.get() == 1 && b.get() == 0)
    check(a.add(5) == 6 && a.n == 6)
    var p = Pair(left: a, right: b)
    p.left.bump_twice()
    p.right.bump()
    check(p.left.n == 8 && p.right.n == 1)
    check(a.n == 6 && b.n == 0)
    let c = bumped(a)
    check(c.n == 7 && a.n == 6)
    let seen = |x: Int| x + b.n
    a.bump()
    check(seen(0) == 0 && a.n == 7)
    Counter(n: 5).bump()
    let t = Counter(n: 9)
    check(t.get() == 9)
}
",
        &[],
        "",
    );
}

#[test]
fn a_question_mark_in_a_mutating_method_still_writes_its_receiver_back() {
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

struct Account { balance: Int, tries: Int }

impl Account {
    fn take(var self, amount: Int) -> Result<Int, Int> {
        self.tries = self.tries + 1
        let left = self.check_left(amount)?
        self.balance = left
        Ok(left)
    }

    fn check_left(self, amount: Int) -> Result<Int, Int> {
        if amount <= self.balance { Ok(self.balance - amount) } else { Err(amount) }
    }
}

fn main() {
    var a = Account(balance: 10, tries: 0)
    check(a.take(4).unwrap_or(-1) == 6)
    check(!a.take(100).is_ok())
    check(a.balance == 6 && a.tries == 2)
}
",
        &[],
        "",
    );
}

#[test]
fn a_panic_in_a_mutating_method_ends_the_program() {
    assert_panics(
        "struct Counter { n: Int }

impl Counter {
    fn divide(var self, by: Int) {
        self.n = self.n / by
    }
}

fn main() {
    var c = Counter(n: 6)
    c.divide(2)
    c.divide(0)
}
",
        "P0004",
        "self.n / by",
    );
}

#[test]
fn tasks_run_in_spawn_order_when_joined_and_when_the_block_ends() {
    assert_runs(
        r#"fn main(io: Stdout) {
    scope |s| {
        let a = s.spawn(|| io.println("a"))
        let b = s.spawn(|| io.println("b"))
        let c = s.spawn(|| io.println("c"))
        let _ = io.println("block")
        let _ = b.join()
        let _ = io.println("joined b")
        let _ = a.join()
        let _ = io.println("joined a")
        let _ = s.spawn(|| io.println("d"))
        let _ = io.println("end")
    }
    let _ = io.println("after")
}
"#,
        &[],
        "block\na\nb\njoined b\njoined a\nend\nc\nd\nafter\n",
    );
}

#[test]
fn join_gives_each_task_its_result() {
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

fn square(n: Int) -> Int {
    n * n
}

fn main() {
    let base = 10
    let total = scope |s| {
        let a = s.spawn(|| square(3))
        let b: Task<Int> = s.spawn(|| base + 1)
        let first = b.join()
        let again = b.join()
        first * 100 + again + a.join()
    }
    check(total == 1100 + 11 + 9)
    let none = scope |s| {
        let _ = s.spawn(|| square(2))
        7
    }
    check(none == 7)
}
",
        &[],
        "",
    );
}

#[test]
fn a_capability_passed_into_a_task_performs_its_effect_there() {
    let mut host = FakeHost::new();
    let outcome = run_differential(
        &mut host,
        r#"fn greet(io: Stdout, name: String) -> Int {
    let _ = io.println(name)
    1
}

fn main(io: Stdout, fs: Fs) {
    let dir = fs.restrict_to(".").write_only()
    let count = scope |s| {
        let a = s.spawn(|| greet(io, "task"))
        let w = s.spawn(|| dir.write(Path.from("out.txt"), "from a task"))
        let _ = w.join()
        a.join() + 1
    }
    let _ = io.println("done")
}
"#,
        &[],
    );
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "task\ndone\n");
    assert_eq!(host.file("out.txt"), Some("from a task"));
}

#[test]
fn a_panic_in_a_task_ends_the_program() {
    assert_panics(
        "fn half(n: Int) -> Int {
    n / 2
}

fn main() {
    let total = scope |s| {
        let a = s.spawn(|| 1)
        let b = s.spawn(|| 10 / half(1))
        a.join() + b.join()
    }
}
",
        "P0004",
        "10 / half(1)",
    );
}

#[test]
fn a_panic_in_the_block_skips_the_tasks_still_queued() {
    assert_panics(
        r#"fn half(n: Int) -> Int {
    n / 2
}

fn main(io: Stdout) {
    scope |s| {
        let _ = s.spawn(|| io.println("never"))
        let _ = io.println("before")
        let _ = 10 / half(1)
    }
}
"#,
        "P0004",
        "10 / half(1)",
    );
}
