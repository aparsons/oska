// SPDX-License-Identifier: Apache-2.0
//! `Deque` (#804, ADR 0032): a prelude value written in Oska. Each helper
//! that runs a program also runs it natively on Linux and compares.

mod common;

use common::{assert_runs, run_on, PROGRAM_PATH};
use oska_run::{run_with_limits, FakeHost, Limits};

#[test]
fn a_deque_works_as_a_queue_and_a_stack() {
    let src = r#"fn yes(b: Bool) -> String {
    if b { "true" } else { "false" }
}

fn text(items: List<Int>) -> String {
    var out = "["
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out}, "
        }
        let x = items.at(i)
        out = "{out}{x}"
        i = i + 1
    }
    "{out}]"
}

fn show(io: Stdout, d: Deque<Int>) -> Result<(), IoError> {
    let items = text(d.to_list())
    io.println(items)
}

fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<Int> = Deque.new()
    d.push_back(1)
    d.push_back(2)
    d.push_front(0)
    d.push_front(-1)
    show(io, d)?
    let n = d.len()
    let g = d.get(1).unwrap_or(99)
    let out = d.get(4).unwrap_or(99)
    let neg = d.get(-1).unwrap_or(99)
    io.println("{n} {g} {out} {neg}")?
    let f = d.pop_front().unwrap_or(99)
    let b = d.pop_back().unwrap_or(99)
    io.println("{f} {b}")?
    show(io, d)?
    let made: Deque<Int> = Deque.of([5, 6, 7])
    show(io, made)?
    let first = made.get(0).unwrap_or(99)
    io.println("{first}")
}
"#;
    assert_runs(
        src,
        &[],
        "[-1, 0, 1, 2]\n4 0 99 99\n-1 2\n[0, 1]\n[5, 6, 7]\n5\n",
    );
}

#[test]
fn a_pop_on_an_empty_deque_is_none() {
    let src = r#"fn yes(b: Bool) -> String {
    if b { "true" } else { "false" }
}

fn text(items: List<Int>) -> String {
    var out = "["
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out}, "
        }
        let x = items.at(i)
        out = "{out}{x}"
        i = i + 1
    }
    "{out}]"
}

fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<String> = Deque.new()
    let a = yes(d.pop_front().is_some())
    let b = yes(d.pop_back().is_some())
    let e = yes(d.is_empty())
    d.push_back("x")
    let c = d.pop_back().unwrap_or("none")
    let f = yes(d.pop_front().is_some())
    io.println("{a} {b} {e} {c} {f}")
}
"#;
    assert_runs(src, &[], "false false true x false\n");
}

#[test]
fn a_copy_passed_to_a_function_is_unchanged() {
    let src = r#"fn yes(b: Bool) -> String {
    if b { "true" } else { "false" }
}

fn text(items: List<Int>) -> String {
    var out = "["
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out}, "
        }
        let x = items.at(i)
        out = "{out}{x}"
        i = i + 1
    }
    "{out}]"
}

fn change(d: Deque<Int>) -> Int {
    var d = d
    d.push_front(100)
    d.push_back(200)
    let _gone = d.pop_back()
    let _gone2 = d.pop_front()
    let _gone3 = d.pop_front()
    d.len()
}

fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<Int> = Deque.of([1, 2, 3])
    let before = d
    let n = change(d)
    let items = text(d.to_list())
    io.println("{n} {items}")?
    d.push_back(4)
    let old = text(before.to_list())
    let same = yes(before == Deque.of([1, 2, 3]))
    io.println("{old} {same}")
}
"#;
    assert_runs(src, &[], "2 [1, 2, 3]\n[1, 2, 3] true\n");
}

#[test]
fn equal_deques_need_not_have_equal_rings() {
    let src = r#"fn yes(b: Bool) -> String {
    if b { "true" } else { "false" }
}

fn text(items: List<Int>) -> String {
    var out = "["
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out}, "
        }
        let x = items.at(i)
        out = "{out}{x}"
        i = i + 1
    }
    "{out}]"
}

fn main(io: Stdout) -> Result<(), IoError> {
    var a: Deque<Int> = Deque.new()
    var i = 0
    while i < 40 {
        a.push_front(i)
        i = i + 1
    }
    while a.len() > 3 {
        let _x = a.pop_back()
    }
    let b: Deque<Int> = Deque.of([39, 38, 37])
    let c: Deque<Int> = Deque.of([39, 38, 36])
    let ab = yes(a == b)
    let ac = yes(a == c)
    io.println("{ab} {ac}")
}
"#;
    assert_runs(src, &[], "true false\n");
}

/// A pseudo-random mix of pushes and pops at both ends, checked against a
/// `List` model, through grows and shrinks of the ring.
const RANDOM_OPS: &str = r#"fn yes(b: Bool) -> String {
    if b { "true" } else { "false" }
}

fn text(items: List<Int>) -> String {
    var out = "["
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out}, "
        }
        let x = items.at(i)
        out = "{out}{x}"
        i = i + 1
    }
    "{out}]"
}

fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<Int> = Deque.new()
    var model: List<Int> = []
    var seed = 12345
    var step = 0
    var bad = 0
    while step < 3000 {
        seed = (seed * 1103515245 + 12345) % 2147483648
        // Phases of growth and of draining make the ring grow and shrink.
        let phase = (step / 500) % 2
        let kind = (seed / 65536) % 8
        let grow = phase == 0
        if (grow && kind < 5) || (!grow && kind < 2) {
            if kind % 2 == 0 {
                d.push_back(step)
                model.push(step)
            } else {
                d.push_front(step)
                var next: List<Int> = [step]
                for x in model {
                    next.push(x)
                }
                model = next
            }
        } else if kind % 2 == 0 {
            let got = d.pop_front().unwrap_or(-1)
            var want = -1
            if model.len() > 0 {
                want = model.at(0)
                var rest: List<Int> = []
                var j = 1
                while j < model.len() {
                    rest.push(model.at(j))
                    j = j + 1
                }
                model = rest
            }
            if got != want {
                bad = bad + 1
            }
        } else {
            let got = d.pop_back().unwrap_or(-1)
            let want = model.pop().unwrap_or(-1)
            if got != want {
                bad = bad + 1
            }
        }
        step = step + 1
    }
    let n = d.len()
    let same = yes(d.to_list() == model)
    io.println("{bad} {n} {same}")
}
"#;

#[test]
fn random_pushes_and_pops_from_both_ends_match_a_list_model() {
    let outcome = run_on(FakeHost::new(), RANDOM_OPS, &[]);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert!(outcome.stdout.starts_with("0 "), "{}", outcome.stdout);
    assert!(outcome.stdout.ends_with(" true\n"), "{}", outcome.stdout);
}

#[test]
fn a_push_past_the_item_limit_panics_with_p0051() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<Int> = Deque.with_max_entries(3)
    d.push_back(1)
    d.push_front(2)
    d.push_back(3)
    let _x = d.pop_front()
    d.push_back(4)
    io.println("four")?
    d.push_back(5)
    io.println("unreachable")
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the fifth item panics");
    assert_eq!(panic.code, "P0051", "{outcome:#?}");
    assert_eq!(outcome.stdout, "four\n");
}

#[test]
fn a_negative_limit_is_zero() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var d: Deque<Int> = Deque.with_max_entries(-5)
    io.println("start")?
    d.push_front(1)
    io.println("unreachable")
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the first push panics");
    assert_eq!(panic.code, "P0051", "{outcome:#?}");
}

/// Pushing and popping at the front is amortized O(1): `n` of each, with
/// a ring that grows and then shrinks, stays within a budget linear in
/// `n`, and the larger run costs about as many steps per operation.
#[test]
fn push_front_and_pop_front_are_amortized_constant_in_steps() {
    fn program(n: u32) -> String {
        format!(
            r#"fn main(io: Stdout) -> Result<(), IoError> {{
    var d: Deque<Int> = Deque.new()
    var i = 0
    while i < {n} {{
        d.push_front(i)
        i = i + 1
    }}
    var sum = 0
    while !d.is_empty() {{
        sum = sum + d.pop_front().unwrap_or(0)
    }}
    io.println("{{sum}}")
}}
"#
        )
    }
    for n in [1_000u32, 8_000] {
        let limits = Limits {
            max_steps: Some(u64::from(n) * 60),
            ..Limits::NONE
        };
        let outcome = run_with_limits(
            &program(n),
            &mut FakeHost::new(),
            &[PROGRAM_PATH.to_string()],
            limits,
        );
        assert!(outcome.panic.is_none(), "n={n}: {outcome:#?}");
        let want = u64::from(n) * u64::from(n - 1) / 2;
        assert_eq!(outcome.stdout, format!("{want}\n"));
    }
}
