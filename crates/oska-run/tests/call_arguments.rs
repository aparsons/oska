//! How the interpreter passes a call's arguments (ticket #505, spec
//! #521): each call evaluates its arguments into one `Vec`, which becomes
//! the callee's frame. The behaviour must not change: every kind of call
//! binds its arguments to the same slots, a `_` parameter takes none, and
//! a function's contracts see the arguments the body sees. Nor may what
//! `--max-memory` charges: a frame is not a value the program holds, and
//! a list or struct literal is still charged for exactly its items.

mod common;

use common::assert_runs;
use oska_run::{run_with_limits, FakeHost, Limits};

/// Every kind of call, each with a `_` parameter between two that bind:
/// a function, a method, a mutating method, a closure that captures, a
/// function value, a trait method, and a function with contracts.
const CALLS: &str = r#"struct Acc { total: Int }

trait Scaled {
    fn scaled(self, _: Int, by: Int) -> Int
}

impl Scaled for Acc {
    fn scaled(self, _: Int, by: Int) -> Int {
        self.total * by
    }
}

impl Acc {
    fn plus(self, a: Int, _: Int, b: Int) -> Int {
        self.total + a * 10 + b
    }

    fn add(var self, _: Int, n: Int) {
        let doubled = n * 2
        self.total = self.total + doubled
    }
}

fn pick(a: Int, _: Int, b: Int) -> Int {
    let sum = a * 100 + b
    let more = sum + 1
    more - 1
}

fn checked(a: Int, _: Int, b: Int) -> Int
    requires a < b
    ensures |r| r == a + b
{
    let r = a + b
    r
}

fn apply(f: fn(Int, Int) -> Int, x: Int) -> Int {
    f(x, 0)
}

fn first(a: Int, _: Int) -> Int {
    a
}

fn main(io: Stdout) {
    let base = 7
    let closure = |a: Int, _: Int, b: Int| a * 1000 + b * 10 + base
    var acc = Acc(total: 1)
    acc.add(99, 4)
    let p = pick(3, 99, 4)
    let q = acc.plus(2, 99, 5)
    let t = acc.total
    let c = closure(6, 99, 8)
    let v = apply(first, 11)
    let s = acc.scaled(99, 3)
    let k = checked(2, 99, 9)
    let _ = io.println("{p} {q} {t} {c} {v} {s} {k}")
}
"#;

#[test]
fn every_kind_of_call_binds_its_arguments_to_the_same_slots() {
    assert_runs(CALLS, &[], "304 34 9 6087 11 27 11\n");
}

/// Intrinsic calls in every position: nested in another's arguments, in
/// a function's arguments, in a closure `List.map` calls back into, on a
/// place (`push`), and reached through a trait (`Display` in an
/// interpolation, `From` behind `?`), so that one call's arguments are
/// still being gathered while another's run.
const INTRINSIC_CALLS: &str = r#"struct Celsius { degrees: Int }

impl Display for Celsius {
    fn to_string(self) -> String {
        let d = self.degrees
        "{d}C"
    }
}

struct ParseError { text: String }

struct AppError { message: String }

impl From<ParseError> for AppError {
    fn from(err: ParseError) -> AppError {
        let text = err.text
        AppError(message: "bad {text}")
    }
}

fn parse(text: String) -> Result<Int, ParseError> {
    match text.parse_int() {
        Some(n) => Ok(n)
        None => Err(ParseError(text: text))
    }
}

fn total(texts: List<String>) -> Result<Int, AppError> {
    var sum = 0
    for text in texts {
        sum = sum + parse(text.trim())?
    }
    Ok(sum)
}

fn shout(text: String, times: Int) -> String {
    text.to_lowercase().repeat(times.to_string().len() + times)
}

fn main(io: Stdout) {
    let words = "a,bb,ccc".split(",")
    let lens = words.map(|w| w.len() * words.len())
    var acc: List<Int> = List.new()
    acc.push(lens.get(0).unwrap_or(0) + "x,y".split(",").len())
    acc.push(shout("AB".slice(0, 1), lens.at(1)).len())
    let warm = Celsius(degrees: acc.at(0) + acc.at(1))
    let good = total([" 1", "2 ", " 3 "])
    let bad = total(["4", "five"])
    let n = match good {
        Ok(n) => n
        Err(_) => 0
    }
    let m = match bad {
        Ok(_) => "unexpected"
        Err(e) => e.message
    }
    let _ = io.println("{warm} {n} {m}")
}
"#;

#[test]
fn intrinsic_calls_see_their_own_arguments_however_they_nest() {
    assert_runs(INTRINSIC_CALLS, &[], "12C 6 bad five\n");
}

/// Makes lists and texts through intrinsics, a mutating one included,
/// and keeps them all, so the meter's sweep frees nothing.
const KEEPS_INTRINSICS: &str = r#"fn main() {
    let text = "a,bb,ccc"
    let comma = ","
    let words = text.split(comma)
    let measure = |w: String| [w.len(), w.byte_len()]
    let lens = words.map(measure)
    var grown: List<Int> = List.new()
    grown.push(words.len())
    grown.push(lens.len())
    let dash = "-"
    let joined = dash.join(words)
    let kept = "kept"
    let all = [words, [kept]]
}
"#;

/// The bytes `KEEPS_INTRINSICS` holds at its end, as the meter counted
/// them before ticket #505 changed how intrinsics get their arguments.
const KEEPS_INTRINSICS_BYTES: u64 = 2764;

#[test]
fn max_memory_charges_the_values_intrinsics_make_exactly_as_before() {
    let fits = |limit| fits_in(KEEPS_INTRINSICS, limit);
    assert!(
        fits(KEEPS_INTRINSICS_BYTES),
        "KEEPS_INTRINSICS should fit in {KEEPS_INTRINSICS_BYTES} bytes"
    );
    assert!(
        !fits(KEEPS_INTRINSICS_BYTES - 1),
        "KEEPS_INTRINSICS should not fit in {} bytes",
        KEEPS_INTRINSICS_BYTES - 1
    );
}

/// Makes lists and structs through every kind of call and keeps them all,
/// the arguments' lists included, so nothing it charges is freed before
/// it ends: the meter's sweep frees nothing, and the smallest limit it
/// fits in is exactly what it was charged. (With garbage to sweep, that
/// limit depends on what a sweep finds dead, which differed between this
/// machine and CI's x86_64 runners.)
const KEEPS: &str = r#"struct Pair { left: List<Int>, right: List<Int> }

impl Pair {
    fn swapped(self, _: Int) -> Pair {
        Pair(left: self.right, right: self.left)
    }

    fn grow(var self, n: Int) {
        self.left.push(n)
    }
}

fn pair(left: List<Int>, _: Int, right: List<Int>) -> Pair {
    Pair(left: left, right: right)
}

fn longer(items: List<Int>, extra: Int) -> List<Int>
    requires extra > 0
    ensures |r| r.len() > items.len()
{
    var more = items
    more.push(extra)
    more
}

fn main() {
    let a = pair([1, 2, 3], 0, [4, 5])
    let six = [6, 7]
    let b = longer(six, 8)
    let ten = [10, 11, 12, 13]
    let make = |xs: List<Int>, _: Int| [xs.len(), 9]
    let c = make(ten, 0)
    var d = a.swapped(0)
    d.grow(14)
    let e = [a, d]
    let f = [b, c, six, ten]
}
"#;

/// Whether `src` runs to its end under a memory limit of `limit` bytes.
fn fits_in(src: &str, limit: u64) -> bool {
    let mut host = FakeHost::new();
    let limits = Limits {
        max_memory: Some(limit),
        ..Limits::NONE
    };
    let outcome = run_with_limits(src, &mut host, &["prog.osk".to_string()], limits);
    assert!(
        outcome.diagnostics.is_empty(),
        "{src} should check: {:#?}",
        outcome.diagnostics
    );
    match &outcome.panic {
        None => true,
        Some(panic) if panic.code == "P0040" => false,
        Some(other) => panic!("{src} should run or stop with P0040, not {other:#?}"),
    }
}

/// Whether `KEEPS` runs to its end under a memory limit of `limit` bytes.
fn keeps_fits_in(limit: u64) -> bool {
    fits_in(KEEPS, limit)
}

/// The bytes `KEEPS` holds at its end, as the meter counted them before
/// ticket #505 changed how calls pass their arguments, on a 64-bit host.
const KEEPS_BYTES: u64 = 3200;

#[test]
fn max_memory_charges_the_values_calls_make_exactly_as_before() {
    assert!(
        keeps_fits_in(KEEPS_BYTES),
        "KEEPS should fit in {KEEPS_BYTES} bytes"
    );
    assert!(
        !keeps_fits_in(KEEPS_BYTES - 1),
        "KEEPS should not fit in {} bytes",
        KEEPS_BYTES - 1
    );
}
