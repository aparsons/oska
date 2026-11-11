//! Native traits and `?` (ticket #133), run on both backends through the
//! differential harness (seam 1): user traits with default and
//! overridden methods, calls through trait bounds in generic code, the
//! prelude's `Eq`, `Display`, `Hash` and `From`, `==`/`!=` on every `Eq`
//! type, and `?` on `Result` and `Option`, with and without a `From`
//! conversion.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// A program of `items` whose `main` runs `body`, with `io: Stdout`,
/// returning `Result<(), IoError>` so that the body can use `?`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n")
}

/// Run `src` on both backends, require the same outcome, and that it
/// printed `stdout` and exited 0.
fn assert_prints(name: &str, src: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// `oskac build` refuses `src` with `E0390` naming `what`, and exits 2.
fn assert_refused(name: &str, src: &str, what: &str) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("native_traits_refusals")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("main.osk"), src).expect("the source");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "main.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{name}: {stderr}");
    assert!(stderr.contains("error[E0390]"), "{name}: {stderr}");
    assert!(stderr.contains(what), "{name}: {stderr}");
}

const SHAPES: &str = "trait Shape {
    fn area(self) -> Int
    fn name(self) -> String {
        \"shape\"
    }
    fn describe(self) -> Untrusted<String> {
        let n = self.name()
        let a = self.area()
        \"{n} of area {a}\"
    }
}

struct Square { side: Int }

struct Rect { w: Int, h: Int, label: String }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
}

impl Shape for Rect {
    fn area(self) -> Int {
        self.w * self.h
    }
    fn name(self) -> String {
        let label = self.label
        \"rect {label}\"
    }
}";

#[test]
fn user_traits_dispatch_default_and_overridden_methods() {
    let src = program(
        SHAPES,
        "    io.println(Square(side: 3).describe())?
    let r = Rect(w: 2, h: 5, label: \"r1\")
    io.println(r.describe())?
    io.println(r.name())?
    io.println(Square(side: 1).name())?",
    );
    assert_prints(
        "trait_defaults",
        &src,
        "shape of area 9\nrect r1 of area 10\nrect r1\nshape\n",
    );
}

#[test]
fn generic_functions_call_the_impl_for_each_type() {
    let items = format!(
        "{SHAPES}

struct Point {{ x: Int, y: Int }}

impl Display for Point {{
    fn to_string(self) -> String {{
        let x = self.x
        let y = self.y
        \"({{x}}, {{y}})\"
    }}
}}

impl Eq for Point {{
    fn eq(self, other: Point) -> Bool {{ self.x == other.x }}
}}

impl Hash for Point {{
    fn hash(self) -> Int {{ self.x * 31 + self.y }}
}}

struct Boxed<T> {{ value: T }}

impl<T: Display> Display for Boxed<T> {{
    fn to_string(self) -> String {{
        let inner = self.value.to_string().assume_trusted()
        \"[{{inner}}]\"
    }}
}}

impl<T> Shape for Boxed<T> {{
    fn area(self) -> Int {{ 1 }}
}}

fn describe_all<S: Shape>(a: S, b: S) -> Untrusted<String> {{
    let first = a.describe()
    let second = b.describe()
    \"{{first}}; {{second}}\"
}}

fn show<T: Display>(label: String, value: T) -> Untrusted<String> {{
    let text = value.to_string()
    \"{{label}} {{value}} {{text}}\"
}}

fn same<T: Eq>(a: T, b: T) -> String {{
    if a == b && a.eq(b) {{ \"same\" }} else if a != b {{ \"different\" }} else {{ \"mixed\" }}
}}

fn hashed<T: Hash>(value: T) -> Int {{
    value.hash() + 1
}}

fn twice<T: Display>(value: T) -> Untrusted<String> {{
    show(\"twice\", Boxed(value: value))
}}"
    );
    let src = program(
        &items,
        "    io.println(describe_all(Square(side: 2), Square(side: 4)))?
    io.println(describe_all(Rect(w: 1, h: 2, label: \"a\"), Rect(w: 3, h: 3, label: \"b\")))?
    io.println(Boxed(value: 5).describe())?
    io.println(show(\"point\", Point(x: 1, y: 2)))?
    io.println(show(\"int\", 7))?
    io.println(show(\"text\", \"hi\"))?
    io.println(twice(Point(x: 3, y: 4)))?
    io.println(twice(\"t\"))?
    io.println(same(Point(x: 1, y: 2), Point(x: 1, y: 9)))?
    io.println(same(Point(x: 1, y: 2), Point(x: 3, y: 2)))?
    io.println(same(4, 4))?
    io.println(same(true, false))?
    io.println(same(\"a\", \"b\"))?
    io.println(same(\"a\", \"a\"))?
    io.println(same((), ()))?
    io.println(same(Some(\"x\"), Some(\"x\")))?
    let h = hashed(Point(x: 2, y: 3))
    io.println(\"hash {h}\")?",
    );
    assert_prints(
        "trait_bounds",
        &src,
        "shape of area 4; shape of area 16\n\
         rect a of area 2; rect b of area 9\n\
         shape of area 1\n\
         point (1, 2) (1, 2)\n\
         int 7 7\n\
         text hi hi\n\
         twice [(3, 4)] [(3, 4)]\n\
         twice [t] [t]\n\
         same\ndifferent\nsame\ndifferent\ndifferent\nsame\nsame\nsame\n\
         hash 66\n",
    );
}

#[test]
fn equality_matches_on_every_eq_type() {
    let src = program(
        "struct Point { x: Int, y: Int }

impl Eq for Point {
    fn eq(self, other: Point) -> Bool { self.x == other.x }
}

enum Color {
    Red,
    Green,
}

impl Eq for Color {
    fn eq(self, other: Color) -> Bool {
        match self {
            Color.Red => match other {
                Color.Red => true
                Color.Green => false
            }
            Color.Green => match other {
                Color.Red => false
                Color.Green => true
            }
        }
    }
}

fn word(io: Stdout, b: Bool) -> Result<(), IoError> {
    if b { io.println(\"T\") } else { io.println(\"F\") }
}

fn some(n: Int) -> Option<Int> {
    if n > 100 { None } else { Some(n) }
}

fn label(n: Int) -> Option<String> {
    Some(\"n{n}\")
}

fn parsed(text: String) -> Result<Int, IoError> {
    match text.parse_int() {
        Some(n) => Ok(n)
        None => Err(IoError(message: \"bad {text}\"))
    }
}",
        "    let a = some(1)
    let none: Option<Int> = None
    word(io, a == some(1))?
    word(io, a != some(2))?
    word(io, a == none)?
    word(io, none == some(500))?
    word(io, label(3) == label(3))?
    word(io, label(3) == label(4))?
    word(io, label(3).eq(label(3)))?
    let p = Some(Point(x: 1, y: 2))
    word(io, p == Some(Point(x: 1, y: 7)))?
    word(io, p != Some(Point(x: 2, y: 2)))?
    let nested = Some(Some(\"deep\"))
    word(io, nested == Some(Some(\"deep\")))?
    word(io, nested == Some(None))?
    word(io, parsed(\"4\") == Ok(4))?
    word(io, parsed(\"x\") == parsed(\"x\"))?
    word(io, parsed(\"x\") == parsed(\"y\"))?
    word(io, parsed(\"4\") == parsed(\"x\"))?
    let e = IoError(message: \"m\")
    word(io, e == IoError(message: \"m\"))?
    word(io, e != IoError(message: \"n\"))?
    word(io, Path.new(\"a/b\") == Path.new(\"a/b\"))?
    word(io, Path.new(\"a\") != Path.new(\"b\"))?
    let r: Result<Color, String> = Ok(Color.Red)
    word(io, r == Ok(Color.Red))?
    word(io, r == Ok(Color.Green))?
    word(io, r == Err(\"x\"))?
    word(io, () == ())?
    word(io, 3 == 3)?
    word(io, \"a\" != \"a\")?",
    );
    assert_prints(
        "equality",
        &src,
        "TTFTTFTTTTFTTFFTTTTTFFTTF"
            .chars()
            .map(|c| format!("{c}\n"))
            .collect::<String>()
            .as_str(),
    );
}

#[test]
fn comparing_lists_is_refused_until_lists_exist() {
    assert_refused(
        "list_eq",
        "fn main(io: Stdout) {\n    let same = [1] == [1]\n}\n",
        "cannot compile",
    );
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

#[test]
fn question_mark_converts_each_error_through_its_from_impl() {
    for (text, stdout, stderr, code) in [
        ("7", "read 7\n", "", 0),
        ("x", "", "not a number: x\n", 1),
        ("12", "", "out of range: 12\n", 1),
    ] {
        let src = format!(
            "{APP_ERROR}
fn main(io: Stdout) -> Result<(), AppError> {{
    let n = read(\"{text}\")?
    let _ = io.println(\"read {{n}}\")
    Ok(())
}}
"
        );
        let outcome = assert_same(&Case::file(&format!("from_{text}"), &src));
        assert_eq!(outcome.stdout, stdout, "{text}: {outcome:#?}");
        assert_eq!(outcome.stderr, stderr, "{text}: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(code), "{text}: {outcome:#?}");
    }
}

#[test]
fn question_mark_returns_early_releasing_what_it_abandons() {
    let src = program(
        "struct Counter { count: Int, log: String }

impl Counter {
    fn step(var self, text: String) -> Option<Int> {
        let n = text.parse_int()?
        self.count = self.count + n
        let log = self.log
        self.log = \"{log}+{text}\"
        Some(self.count)
    }
}

fn first_digit(text: String) -> Option<Int> {
    let head = text.slice(0, 1)
    head.parse_int()
}

fn join(a: String, b: String, c: String) -> String {
    \"{a}{b}{c}\"
}

fn digits(text: String) -> Option<String> {
    let label = \"digits of {text}\"
    let joined = join(\"<{label}>\", first_digit(text)?.to_string(), \"!\")
    Some(joined)
}

fn total(texts: Option<String>, more: Option<String>) -> Option<Int> {
    var sum = 0
    for i in 0..3 {
        let here = \"round {i}\"
        let t = match texts {
            Some(t) => {
                let held = \"held {t}\"
                first_digit(held.slice(5, 6))?
            }
            None => more?.len()
        }
        sum = sum + t + here.len()
    }
    Some(sum)
}

fn checked(text: String) -> Result<String, String> {
    if text == \"\" {
        Err(\"empty\")
    } else {
        Ok(\"<{text}>\")
    }
}

fn both(a: String, b: String) -> Result<String, String> {
    let x = checked(a)?
    let y = checked(b)?
    Ok(\"{x}{y}\")
}

fn shown(r: Result<String, String>) -> String {
    match r {
        Ok(t) => \"ok {t}\"
        Err(e) => \"err {e}\"
    }
}

fn number(o: Option<Int>) -> String {
    match o {
        Some(n) => \"some {n}\"
        None => \"none\"
    }
}

fn text(o: Option<String>) -> String {
    match o {
        Some(t) => \"some {t}\"
        None => \"none\"
    }
}",
        "    var c = Counter(count: 0, log: \"\")
    io.println(number(c.step(\"4\")))?
    io.println(number(c.step(\"x\")))?
    io.println(number(c.step(\"5\")))?
    io.println(c.log)?
    io.println(text(digits(\"42\")))?
    io.println(text(digits(\"z2\")))?
    io.println(number(total(Some(\"9\"), None)))?
    io.println(number(total(Some(\"q\"), None)))?
    io.println(number(total(None, Some(\"abc\"))))?
    io.println(number(total(None, None)))?
    io.println(shown(both(\"a\", \"b\")))?
    io.println(shown(both(\"\", \"b\")))?
    io.println(shown(both(\"a\", \"\")))?
    var i = 0
    while i < 20000 {
        let _ = digits(\"x{i}\")
        let _ = total(Some(\"q\"), None)
        let _ = both(\"a{i}\", \"\")
        i = i + 1
    }
    io.println(\"done\")?",
    );
    assert_prints(
        "question_mark",
        &src,
        "some 4\nnone\nsome 9\n+4+5\nsome <digits of 42>4!\nnone\n\
         some 48\nnone\nsome 30\nnone\nok <a><b>\nerr empty\nerr empty\ndone\n",
    );
}

#[test]
fn question_mark_on_a_failed_main_reports_its_error() {
    let src = "fn get(n: Int) -> Result<Int, IoError> {
    if n > 1 { Err(IoError(message: \"too big {n}\")) } else { Ok(n) }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let a = get(1)?
    io.println(\"got {a}\")?
    let b = get(5)?
    io.println(\"got {b}\")?
    Ok(())
}
";
    let outcome = assert_same(&Case::file("failed_main", src));
    assert_eq!(outcome.stdout, "got 1\n", "{outcome:#?}");
    assert_eq!(outcome.stderr, "too big 5\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(1), "{outcome:#?}");
}

#[test]
fn mutating_trait_methods_and_generic_from_impls_work_through_bounds() {
    let src = program(
        "trait Tally {
    fn add(var self, n: Int)
    fn total(self) -> Int
    fn add_twice(var self, n: Int) {
        self.add(n)
        self.add(n)
    }
}

struct Sum { n: Int, label: String }

impl Tally for Sum {
    fn add(var self, n: Int) {
        self.n = self.n + n
    }
    fn total(self) -> Int { self.n }
}

fn tally_all<T: Tally>(start: T) -> Int {
    var t = start
    t.add(1)
    t.add_twice(10)
    t.total()
}

struct Fail<T> { code: T }

struct Report { text: String }

impl<T: Display> From<Fail<T>> for Report {
    fn from(f: Fail<T>) -> Report {
        let code = f.code.to_string().assume_trusted()
        Report(text: \"failed with {code}\")
    }
}

fn check<T>(ok: Bool, code: T) -> Result<Int, Fail<T>> {
    if ok { Ok(1) } else { Err(Fail(code: code)) }
}

fn run(a: Bool, b: Bool) -> Result<Int, Report> {
    let x = check(a, 7)?
    let y = check(b, \"text\")?
    Ok(x + y)
}

fn shown(r: Result<Int, Report>) -> String {
    match r {
        Ok(n) => \"ok {n}\"
        Err(report) => report.text
    }
}",
        "    let s = Sum(n: 0, label: \"s\")
    let t = tally_all(s)
    io.println(\"{t} {s.n}\")?
    io.println(shown(run(true, true)))?
    io.println(shown(run(false, true)))?
    io.println(shown(run(true, false)))?",
    );
    assert_prints(
        "trait_mutating_from",
        &src,
        "21 0\nok 2\nfailed with 7\nfailed with text\n",
    );
}

#[test]
fn todo_panics_with_p0003_where_it_is_reached() {
    let src = "fn later(n: Int) -> String {
    if n < 2 { \"early\" } else { todo() }
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(later(1))?
    io.println(later(2))?
    Ok(())
}
";
    let outcome = assert_same(&Case::file("todo", src));
    assert_eq!(outcome.stdout, "early\n", "{outcome:#?}");
    assert!(outcome.stderr.contains("error[P0003]"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}
