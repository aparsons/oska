//! Native closures and function values (ticket #135), run on both
//! backends through the differential harness (seam 1): closures capturing
//! by value, `cap fn` values, named functions and methods used as values,
//! lists of mixed function values, `List.map`, and calls through function
//! values counting their depth (`P0021`) and checking `requires`. Every
//! target must compile these programs
//! (`differential::assert_compiled_same`, #162): a refusal fails on every
//! host, and each host builds them for the other targets too.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use differential::{assert_compiled_same, Case};

/// The helpers every program has. Interpolation takes only names, so a
/// program prints through these.
const HELPERS: &str = r#"fn show(io: Stdout, text: String) {
    let _ = io.println(text)
}

fn num(io: Stdout, n: Int) {
    let _ = io.println("{n}")
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
fn a_closure_captures_by_value() {
    let body = r#"    var n = 10
    var s = "before"
    let f = |x: Int| x + n
    let g = |t: String| "{s} {t}"
    n = 20
    s = "after"
    num(io, f(1))
    show(io, g("call"))
    num(io, n)
    show(io, s)"#;
    assert_prints("closure_by_value", "", body, "11\nbefore call\n20\nafter\n");
}

/// Run the program on both backends, require the same outcome, and that
/// it panicked (exit 101) with `stderr` after printing `stdout`.
fn assert_panics(name: &str, src: &str, stdout: &str, stderr: &str) {
    let outcome = assert_compiled_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.stderr, stderr, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
}

const SHAPES: &str = r#"struct Point { x: Int, y: Int }

impl Point {
    fn sum(self) -> Int {
        self.x + self.y
    }

    fn scaled(self, k: Int) -> Point {
        Point(x: self.x * k, y: self.y * k)
    }

    fn adder(self) -> fn(Int) -> Int {
        |n: Int| n + self.x + self.y
    }
}

fn double(n: Int) -> Int {
    n * 2
}

fn twice<T>(f: fn(T) -> T, x: T) -> T {
    f(f(x))
}

fn make_adder(k: Int) -> fn(Int) -> Int {
    |n: Int| n + k
}

fn compose(f: fn(Int) -> Int, g: fn(Int) -> Int) -> fn(Int) -> Int {
    |n: Int| g(f(n))
}"#;

/// A mutating method used as a value changes a copy of its receiver, and
/// a trait's method used as a value is the implementation for its first
/// parameter's type.
#[test]
fn mutating_and_trait_methods_are_values() {
    let items = r#"struct Counter { n: Int, label: String }

impl Counter {
    fn bump(var self) -> Int {
        self.n = self.n + 1
        self.label = "bumped"
        self.n
    }
}

trait Size {
    fn size(self) -> Int
}

impl Size for Counter {
    fn size(self) -> Int {
        self.n * 100
    }
}"#;
    let body = r#"    let c = Counter(n: 1, label: "c")
    let bump = Counter.bump
    num(io, bump(c))
    num(io, bump(c))
    let size = Size.size
    num(io, size(c))
    num(io, c.n)
    show(io, c.label)"#;
    assert_prints("method_values", items, body, "2\n2\n100\n1\nc\n");
}

#[test]
fn named_functions_and_methods_are_values() {
    let body = r#"    let f = double
    num(io, f(21))
    num(io, twice(double, 5))
    let excl = |t: String| "{t}!"
    show(io, twice(excl, "hey"))
    let p = Point(x: 3, y: 4)
    let sum = Point.sum
    num(io, sum(p))
    let scaled = Point.scaled
    let q = scaled(p, 10)
    num(io, q.sum())
    let add = p.adder()
    num(io, add(100))
    let len = String.len
    num(io, len("héllo"))"#;
    assert_prints(
        "named_values",
        SHAPES,
        body,
        "42\n20\nhey!!\n7\n70\n107\n5\n",
    );
}

#[test]
fn function_values_pass_to_and_return_from_functions() {
    let body = r#"    let add5 = make_adder(5)
    let both = compose(add5, double)
    num(io, both(1))
    let nested = compose(compose(double, add5), make_adder(-1))
    num(io, nested(10))
    var counter = 0
    while counter < 3 {
        let step = make_adder(counter)
        num(io, step(counter))
        counter = counter + 1
    }
    let outer = |a: Int| {
        let inner = |b: Int| a * b
        inner(a + 1)
    }
    num(io, outer(6))"#;
    assert_prints("passed_and_returned", SHAPES, body, "12\n24\n0\n2\n4\n42\n");
}

#[test]
fn lists_of_mixed_fn_and_cap_fn_values_run_in_order() {
    let items = r#"fn double(n: Int) -> Int {
    n * 2
}

fn run_all(fs: List<cap fn(Int) -> Int>, x: Int) -> Int {
    var total = 0
    for f in fs {
        total = total + f(x)
    }
    total
}"#;
    let body = r#"    let loud = |n: Int| {
        num(io, n)
        n + 1
    }
    let fs = [double, loud, |n: Int| n * n]
    num(io, run_all(fs, 7))
    var more = fs
    more.push(|n: Int| 0 - n)
    num(io, run_all(more, 2))
    num(io, fs.len())
    let g = fs.at(1)
    num(io, g(40))"#;
    assert_prints("mixed_list", items, body, "7\n71\n2\n9\n3\n40\n41\n");
}

#[test]
fn list_map_matches_the_interpreter() {
    let body = r#"    let xs = [1, 2, 3]
    let k = 10
    let ys = xs.map(|x: Int| x * k)
    num(io, ys.at(0) + ys.at(1) + ys.at(2))
    let words = xs.map(|x: Int| "w{x}")
    show(io, ", ".join(words))
    let lens = ["a", "bb", "ccc"].map(String.len)
    num(io, lens.at(2))
    let doubled = xs.map(double)
    num(io, doubled.at(2))
    let empty: List<Int> = []
    num(io, empty.map(double).len())
    let nested = [[1, 2], [3]].map(|row: List<Int>| row.map(|x: Int| x + 1))
    num(io, nested.at(0).at(1) + nested.at(1).at(0))
    let points = xs.map(|x: Int| Point(x: x, y: x))
    num(io, points.map(Point.sum).at(2))"#;
    assert_prints("list_map", SHAPES, body, "60\nw1, w2, w3\n3\n6\n0\n7\n6\n");
}

#[test]
fn closures_capture_structs_lists_and_text_and_free_them() {
    let items = r#"struct Tag { name: String, n: Int }

fn labeller(tag: Tag, xs: List<String>) -> fn(Int) -> String {
    |i: Int| {
        let item = xs.at(i)
        "{item}:{i}"
    }
}"#;
    let body = r#"    var xs = ["a", "b"]
    var tag = Tag(name: "t", n: 1)
    let f = |i: Int| {
        let item = xs.at(i)
        let name = tag.name
        "{name}{item}"
    }
    xs.push("c")
    tag.name = "changed"
    show(io, f(1))
    num(io, xs.len())
    var total = 0
    var round = 0
    while round < 20000 {
        let big = "x".repeat(100)
        let g = |n: Int| big.len() + n
        let h = g
        total = total + h(round) - round
        let l = labeller(tag, xs)
        let text = l(2)
        total = total + text.len()
        round = round + 1
    }
    num(io, total)"#;
    assert_prints("captures_freed", items, body, "tb\n3\n2060000\n");
}

#[test]
fn question_mark_and_return_inside_a_closure_leave_the_closure() {
    let body = r#"    let parse = |s: String| {
        let n = s.parse_int()?
        Some(n * 2)
    }
    let a = parse("21").unwrap_or(-1)
    let b = parse("x").unwrap_or(-1)
    num(io, a)
    num(io, b)
    let firsts = ["", "ab", "c"].map(|s: String| {
        if s.len() == 0 {
            return "-"
        }
        s.slice(0, 1)
    })
    show(io, "".join(firsts))"#;
    assert_prints("closure_exits", "", body, "42\n-1\n-ac\n");
}

#[test]
fn a_closure_reads_the_receiver_of_the_method_it_is_written_in() {
    let items = r#"struct Counter { n: Int, label: String }

impl Counter {
    fn bump(var self) {
        let before = || self.n
        self.n = self.n + 10
        let after = || self.n
        self.n = before() + after()
    }

    fn render(self) -> List<String> {
        [1, 2].map(|i: Int| {
            let label = self.label
            let n = self.n
            "{label}{n}.{i}"
        })
    }
}"#;
    let body = r#"    var c = Counter(n: 1, label: "c")
    c.bump()
    num(io, c.n)
    show(io, " ".join(c.render()))"#;
    assert_prints("closure_self", items, body, "12\nc12.1 c12.2\n");
}

#[test]
fn a_closure_calls_through_a_trait_bound() {
    let items = r#"trait Size {
    fn size(self) -> Int
}

struct Box { w: Int }

impl Size for Box {
    fn size(self) -> Int {
        self.w * 10
    }
}

impl Size for String {
    fn size(self) -> Int {
        self.len()
    }
}

fn sizes<T: Size>(xs: List<T>, extra: Int) -> List<Int> {
    xs.map(|x: T| x.size() + extra)
}

fn total(xs: List<Int>) -> Int {
    var t = 0
    for x in xs {
        t = t + x
    }
    t
}"#;
    let body = r#"    num(io, total(sizes([Box(w: 1), Box(w: 2)], 1)))
    num(io, total(sizes(["ab", "cde"], 0)))"#;
    assert_prints("trait_bound", items, body, "32\n5\n");
}

#[test]
fn runaway_recursion_through_a_function_value_panics_with_p0021() {
    let items = "fn down(n: Int) -> Int {\n    let f = down\n    \
                 if n == 0 { 0 } else { 1 + f(n - 1) }\n}";
    let deep = program(items, "    num(io, down(2000))");
    let outcome = assert_compiled_same(&Case::file("value_recursion", &deep));
    assert_eq!(outcome.stdout, "2000\n", "{outcome:#?}");
    let runaway = program(items, "    num(io, down(1000000))");
    assert_panics(
        "value_runaway",
        &runaway,
        "",
        "main.osk:11:32: error[P0021]: calls nest too deep: is this recursion missing its \
         base case? (bytes 203..211)\n",
    );
}

#[test]
fn a_panic_inside_a_closure_names_the_closures_code() {
    let body = "    let xs = [1, 2]\n    let ys = xs.map(|i: Int| xs.at(i))\n    num(io, ys.len())";
    assert_panics(
        "closure_panic",
        &program("", body),
        "",
        "main.osk:13:30: error[P0002]: index 2 is out of bounds for a list of length 2 \
         (bytes 203..211)\n",
    );
}

/// A call through a function value counts one level, as the interpreter's
/// `call_value` does: checked against the native executable alone, as the
/// debug interpreter runs out of its own stack first (`native_ints.rs`).
#[test]
fn a_call_through_a_function_value_counts_exactly_one_level() {
    let limit = oska_native::MAX_CALL_DEPTH;
    let items = "fn down(n: Int) -> Int {\n    let f = down\n    \
                 if n == 0 { 0 } else { 1 + f(n - 1) }\n}";
    // `down(n)` nests `n + 1` calls under `main`.
    let body = |n: u32| format!("    num(io, down({n}))");
    let deepest = differential::run_native(&Case::file(
        "value_depth_ok",
        &program(items, &body(limit - 1)),
    ));
    assert_eq!(deepest.stdout, format!("{}\n", limit - 1), "{deepest:#?}");
    let too_deep = differential::run_native(&Case::file(
        "value_depth_over",
        &program(items, &body(limit)),
    ));
    assert!(too_deep.stderr.contains("error[P0021]"), "{too_deep:#?}");
    assert_eq!(too_deep.exit_code, Some(101));
}
