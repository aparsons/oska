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

/// Whether `KEEPS` runs to its end under a memory limit of `limit` bytes.
fn keeps_fits_in(limit: u64) -> bool {
    let mut host = FakeHost::new();
    let limits = Limits {
        max_memory: Some(limit),
        ..Limits::NONE
    };
    let outcome = run_with_limits(KEEPS, &mut host, &["prog.osk".to_string()], limits);
    assert!(
        outcome.diagnostics.is_empty(),
        "KEEPS should check: {:#?}",
        outcome.diagnostics
    );
    match &outcome.panic {
        None => true,
        Some(panic) if panic.code == "P0040" => false,
        Some(other) => panic!("KEEPS should run or stop with P0040, not {other:#?}"),
    }
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
