//! The parity gaps the earlier native tickets left (ticket #140), run on
//! both backends through the differential harness (seam 1): range values,
//! function values made from prelude functions that may panic, function
//! values of many parameters, `dbg` of function values, tasks and scopes,
//! `Scope.spawn` and `Task.join` as values, and the call depth of contract
//! clauses and tasks at the `P0021` boundary.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

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
/// it printed `stdout` and `stderr` and exited 0.
fn assert_prints(name: &str, items: &str, body: &str, stdout: &str, stderr: &str) {
    let outcome = assert_same(&Case::file(name, &program(items, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.stderr, stderr, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

// ---- Range values ------------------------------------------------------------

#[test]
fn ranges_are_values_passed_returned_stored_and_iterated() {
    let items = r#"struct Span { r: Range<Int>, name: String }

fn upto(n: Int) -> Range<Int> {
    0..n
}

fn total(r: Range<Int>) -> Int {
    var t = 0
    for i in r {
        t = t + i
    }
    t
}

fn bytes(x: U8) -> Range<U8> {
    x..10
}"#;
    let body = r#"    let r = 0..3
    let s = upto(5)
    num(io, total(r))
    num(io, total(s))
    num(io, r.start)
    num(io, s.end)
    let sp = Span(r: 2..4, name: "two")
    num(io, total(sp.r))
    show(io, sp.name)
    var m = 1..2
    m.end = 6
    m.start = m.start + 1
    num(io, total(m))
    let k = Range(start: 7, end: 9)
    for i in k {
        num(io, i)
    }
    for b in bytes(8) {
        let _ = dbg(b)
    }
    let none = 5..2
    num(io, total(none))
    let f = upto
    num(io, total(f(4)))
    let rs = [0..1, 3..5]
    for x in rs {
        num(io, total(x))
    }
    let o = Some(1..3)
    match o {
        Some(x) => num(io, x.end)
        None => num(io, 0)
    }"#;
    assert_prints(
        "range_values",
        items,
        body,
        "3\n10\n0\n5\n5\ntwo\n14\n7\n8\n0\n6\n0\n7\n3\n",
        "[dbg] 8\n[dbg] 9\n",
    );
}

#[test]
fn dbg_shows_a_range_by_its_bounds() {
    let items = r#"struct Wrap { r: Range<I8> }

fn lo(x: I8) -> Range<I8> {
    x..3
}"#;
    let body = r#"    let r = 0..3
    let _ = dbg(r)
    let _ = dbg(Range(start: -2, end: 9))
    let _ = dbg(Wrap(r: lo(-4)))
    let _ = dbg([1..2, 5..5])
    let _ = dbg(Some(1..4))
    let _ = dbg(Range(start: "a", end: "b"))
    let _ = dbg(Range(start: [1], end: []))"#;
    assert_prints(
        "range_dbg",
        items,
        body,
        "",
        "[dbg] 0..3\n[dbg] -2..9\n[dbg] Wrap(-4..3)\n[dbg] [1..2, 5..5]\n\
         [dbg] Option.Some(1..4)\n[dbg] \"a\"..\"b\"\n[dbg] [1]..[]\n",
    );
}

#[test]
fn ranges_of_counted_bounds_are_released_and_copied_by_value() {
    // A range over text holds its bounds' references; a loop building and
    // dropping them must not grow, and writing a copy leaves the original.
    let body = r#"    var i = 0
    var keep = Range(start: "a", end: "b")
    while i < 200000 {
        let word = "w{i}"
        let r = Range(start: word, end: "{word}!")
        var c = r
        c.end = "changed"
        if i == 199999 {
            keep = r
            show(io, c.end)
        }
        i = i + 1
    }
    show(io, keep.start)
    show(io, keep.end)"#;
    assert_prints(
        "range_counted",
        "",
        body,
        "changed\nw199999\nw199999!\n",
        "",
    );
}

// ---- Many parameters -----------------------------------------------------------

#[test]
fn functions_closures_and_function_values_take_more_than_eight_arguments() {
    // Past the eighth, arguments travel through memory: recursion passes
    // them on rotated, a mutating method gives its receiver back, a
    // generic function returns its ninth, and `requires`/`ensures` read
    // them; closures and named functions as values take them too.
    let items = r#"struct Acc { total: Int }

impl Acc {
    fn add_all(var self, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int) {
        self.total = self.total + a + b + c + d + e + f + g + h + i + j
    }
}

fn weigh(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: String, k: Int) -> Int
    requires k > 0
    ensures |r| r > k
{
    a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + j.len() * 10 + 11 * k
}

fn deep(n: Int, a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int, j: Int) -> Int {
    if n == 0 {
        return a - b + c - d + e - f + g - h + i - j
    }
    let inner = deep(n - 1, j, a, b, c, d, e, f, g, h, i)
    inner + j
}

fn pick<T>(a: T, b: T, c: T, d: T, e: T, f: T, g: T, h: T, i: T, which: Int) -> T {
    if which == 9 { i } else { if which == 8 { h } else { a } }
}"#;
    let body = r#"    num(io, weigh(1, 2, 3, 4, 5, 6, 7, 8, 9, "ten", 11))
    num(io, deep(30, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    show(io, pick("a", "b", "c", "d", "e", "f", "g", "h", "i", 9))
    show(io, pick("a", "b", "c", "d", "e", "f", "g", "h", "i", 8))
    var acc = Acc(total: 0)
    acc.add_all(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
    num(io, acc.total)
    let w = weigh
    num(io, w(1, 1, 1, 1, 1, 1, 1, 1, 1, "x", 1))
    let base = 100
    let many = |a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: String| base + a + b + c + d + e + f + g + h + i.len()
    num(io, many(1, 2, 3, 4, 5, 6, 7, 8, "nine"))
    let fs = [many, |a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: String| a * h]
    for one in fs {
        num(io, one(2, 0, 0, 0, 0, 0, 0, 5, ""))
    }
    num(io, w(1, 2, 3, 4, 5, 6, 7, 8, 9, "ten", 0))"#;
    let outcome = assert_same(&Case::file("many_arguments", &program(items, body)));
    assert_eq!(
        outcome.stdout, "436\n160\ni\nh\n55\n66\n140\n107\n10\n",
        "{outcome:#?}"
    );
    assert!(
        outcome
            .stderr
            .contains("error[P0006]: precondition violated: `weigh` requires `k > 0`"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}

// ---- Prelude functions that may panic, as values ---------------------------------

/// A program whose `main` calls prelude functions that may panic through
/// function values, panicking the way its first argument says.
const PANICKING_VALUES: &str = r#"fn apply(f: fn(String, Int, Int) -> String, s: String) -> String {
    f(s, 1, 3)
}

fn main(io: Stdout, env: Env, args: Args) {
    let slice = String.slice
    let _ = dbg(apply(slice, "hello"))
    let rep = String.repeat
    let _ = dbg(rep("ab", 3))
    let secret = Env.secret
    let which = args.get(1).unwrap_or("none")
    if which == "slice" {
        let _ = dbg(apply(slice, "hé"))
    }
    if which == "repeat" {
        let _ = dbg(rep("ab", -2))
    }
    if which == "long" {
        let _ = dbg(rep("abcd", 100000000))
    }
    if which == "secret" {
        let _ = secret(env, "NOPE_NOT_SET")
    }
    if which == "todo" {
        let t: fn() -> Int = todo
        let _ = dbg(t())
    }
}
"#;

#[test]
fn a_prelude_function_value_panics_at_the_call_of_the_value() {
    let ok = "[dbg] \"el\"\n[dbg] \"ababab\"\n";
    let cases = [
        ("none", ""),
        (
            "slice",
            "main.osk:2:5: error[P0035]: cannot slice characters 1..3 of a text of 2 characters (bytes 71..81)\n",
        ),
        (
            "repeat",
            "main.osk:16:21: error[P0036]: cannot repeat a text -2 times (bytes 449..462)\n",
        ),
        (
            "long",
            "main.osk:19:21: error[P0037]: repeating a text of 4 bytes 100000000 times is too long (bytes 515..537)\n",
        ),
        (
            "secret",
            "main.osk:22:17: error[P0020]: the secret `NOPE_NOT_SET` is not set in the environment (bytes 588..615)\n",
        ),
        (
            "todo",
            "main.osk:26:21: error[P0003]: reached `todo()`: this code is not written yet (bytes 701..704)\n",
        ),
    ];
    for (which, report) in cases {
        let name = format!("panicking_value_{which}");
        let case = Case::file(&name, PANICKING_VALUES).with_args(&[which]);
        let outcome = assert_same(&case);
        assert_eq!(
            outcome.stderr,
            format!("{ok}{report}"),
            "{name}: {outcome:#?}"
        );
        let code = if report.is_empty() { 0 } else { 101 };
        assert_eq!(outcome.exit_code, Some(code), "{name}: {outcome:#?}");
    }
}

// ---- dbg of function values, tasks, scopes and capabilities -------------------------

#[test]
fn dbg_shows_function_values_tasks_scopes_and_capabilities() {
    let src = r#"struct Job { run: fn(Int) -> Int, label: String }

fn twice(x: Int) -> Int {
    x * 2
}

fn main(io: Stdout, err: Stderr, input: Stdin, env: Env, clock: Clock, random: Random, fs: Fs) {
    let total = scope |s| {
        let spawn: fn(Scope, cap fn() -> Int) -> Task<Int> = Scope.spawn
        let a = spawn(s, || 20)
        let b = s.spawn(|| 22)
        let _ = dbg(a)
        let _ = dbg(s)
        let _ = dbg([a, b])
        a.join() + b.join()
    }
    let _ = dbg(total)
    let c = |x: Int| x + total
    let _ = dbg(c)
    let _ = dbg(twice)
    let _ = dbg([c, twice, |y: Int| y])
    let _ = dbg(Job(run: c, label: "j"))
    let _ = dbg(Some(twice))
    let _ = dbg(io)
    let _ = dbg(err)
    let _ = dbg(input)
    let _ = dbg(env)
    let _ = dbg(clock)
    let _ = dbg(random)
    let _ = dbg(fs)
    let _ = dbg(fs.read_only())
    let _ = dbg(fs.write_only())
}
"#;
    let outcome = assert_same(&Case::file("dbg_values", src));
    assert_eq!(
        outcome.stderr,
        "[dbg] <task>\n[dbg] <scope>\n[dbg] [<task>, <task>]\n[dbg] 42\n[dbg] <closure>\n\
         [dbg] <fn>\n[dbg] [<closure>, <fn>, <closure>]\n[dbg] Job(<closure>, \"j\")\n\
         [dbg] Option.Some(<fn>)\n[dbg] <Stdout>\n[dbg] <Stderr>\n[dbg] <Stdin>\n[dbg] <Env>\n\
         [dbg] <Clock>\n[dbg] <Random>\n\
         [dbg] <Fs(FsCap { root: Anywhere, access: ReadWrite })>\n\
         [dbg] <Fs(FsCap { root: Anywhere, access: Read })>\n\
         [dbg] <Fs(FsCap { root: Anywhere, access: Write })>\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn dbg_shows_the_arguments() {
    // The executable's own path is its first argument natively (the
    // interpreter's is the program's), so only the rest are compared.
    let src = "fn main(args: Args) {\n    let _ = dbg(args)\n}\n";
    let case = Case::file("dbg_args", src).with_args(&["x", "y \"z\""]);
    let (interpreted, native) = differential::run(&case);
    for outcome in [&interpreted, &native] {
        assert!(
            outcome.stderr.starts_with("[dbg] Args[\"")
                && outcome.stderr.ends_with("\", \"x\", \"y \\\"z\\\"\"]\n"),
            "{outcome:#?}"
        );
        assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    }
}

#[test]
fn dbg_of_a_directory_capability_is_refused() {
    // Its interpreter text holds an OS handle: the native backend refuses
    // it rather than show another.
    let src = "fn main(fs: FsRead) {\n    let _ = dbg(fs.restrict_to(\".\"))\n}\n";
    let stderr = refusal("dbg_dir", src);
    assert!(
        stderr.contains("error[E0390]: `oskac build` cannot compile `dbg` of a `DirRead` yet"),
        "{stderr}"
    );
}

/// Build `src` as `main.osk`, which must be refused (exit 2): the stderr.
fn refusal(name: &str, src: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("main.osk"), src).expect("the source");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "main.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    stderr
}

// ---- Call depth at the P0021 boundary ----------------------------------------------

/// `program`'s outcome natively, given `depth` as its argument. A debug
/// build of the interpreter runs out of its own stack before the limit,
/// so these are checked against the native executable alone; each
/// expected outcome is a release interpreter's (`oskac run`), checked when
/// these tests were written.
fn at_depth(name: &str, program: &str, depth: u32) -> differential::Outcome {
    let case = Case::file(name, program).with_args(&[&depth.to_string()]);
    differential::run_native(&case)
}

/// Require `program` to print `deepest` given it, and to panic with
/// `P0021` at line and column `at`, bytes `bytes`, given one more.
fn assert_boundary(name: &str, program: &str, deepest: u32, (at, bytes): (&str, &str)) {
    let ok = at_depth(&format!("{name}_ok"), program, deepest);
    assert_eq!(ok.stdout, format!("{deepest}\n"), "{ok:#?}");
    assert_eq!(ok.exit_code, Some(0), "{ok:#?}");
    let over = at_depth(&format!("{name}_over"), program, deepest + 1);
    assert_eq!(
        over.stderr,
        format!(
            "main.osk:{at}: error[P0021]: calls nest too deep: is this recursion missing \
             its base case? (bytes {bytes})\n"
        ),
        "{over:#?}"
    );
    assert_eq!(over.exit_code, Some(101), "{over:#?}");
}

const DEPTH_MAIN: &str = r#"fn main(io: Stdout, args: Args) {
    let k = args.get(1).unwrap_or("0").parse_int().unwrap_or(0)
    let r = g(k)
    let _ = io.println("{r}")
}
"#;

#[test]
fn contract_clauses_count_calls_as_the_interpreter_does() {
    let limit = oska_native::MAX_CALL_DEPTH;
    // A `requires` clause's call nests one deeper than the call it checks.
    let requires = format!(
        "fn ok(n: Int) -> Bool {{\n    n >= 0\n}}\n\n\
         fn g(n: Int) -> Int\n    requires ok(n)\n{{\n    if n == 0 {{\n        return 0\n    }}\n    \
         g(n - 1) + 1\n}}\n\n{DEPTH_MAIN}"
    );
    assert_boundary("depth_requires", &requires, limit - 2, ("6:14", "71..76"));
    // So does an `ensures` clause's, checked as the deepest call returns.
    let ensures = format!(
        "fn small(n: Int) -> Bool {{\n    n < 1000000\n}}\n\n\
         fn g(n: Int) -> Int\n    ensures |r| small(r)\n{{\n    if n == 0 {{\n        return 0\n    }}\n    \
         g(n - 1) + 1\n}}\n\n{DEPTH_MAIN}"
    );
    assert_boundary("depth_ensures", &ensures, limit - 2, ("6:17", "82..90"));
}

#[test]
fn a_named_function_value_counts_its_call_once() {
    let limit = oska_native::MAX_CALL_DEPTH;
    let src = format!(
        "fn positive(n: Int) -> Bool {{\n    n >= 0\n}}\n\n\
         fn g(n: Int) -> Int\n    requires positive(n)\n{{\n    if n == 0 {{\n        return 0\n    }}\n    \
         let f = g\n    f(n - 1) + 1\n}}\n\n{DEPTH_MAIN}"
    );
    assert_boundary("depth_thunk", &src, limit - 2, ("6:14", "77..88"));
}

#[test]
fn tasks_recursing_through_join_reach_the_limit_without_overflowing() {
    // Each level is two counted calls, the function's and the task's, with
    // a runner's and the queue's frames between them on the stack.
    let limit = oska_native::MAX_CALL_DEPTH;
    let src = format!(
        "fn g(n: Int) -> Int {{\n    if n == 0 {{\n        return 0\n    }}\n    \
         let r = scope |s| {{\n        let t = s.spawn(|| g(n - 1))\n        t.join()\n    }}\n    \
         r + 1\n}}\n\n{DEPTH_MAIN}"
    );
    assert_boundary("depth_tasks", &src, limit / 2 - 1, ("6:28", "112..120"));
}
