// SPDX-License-Identifier: Apache-2.0
//! `Set<T>` core (#802, spec #798, ADR 0032): `new`, `of`, `add`, `remove`,
//! `contains`, `len`, `is_empty`, `to_list` and `with_max_entries`, in
//! insertion order. Each helper that runs a program also runs it natively
//! on Linux and compares (`common/mod.rs`), so each expected output holds
//! on the interpreter and the native backend alike.

mod common;

use common::{assert_runs, run_on};
use oska_run::FakeHost;

/// A program with a `show` that prints a `Set<String>` on one line, and
/// the given `body` as `main`'s statements after `var s`.
fn program(body: &str) -> String {
    format!(
        "\
         fn show(io: Stdout, label: String, s: Set<String>) -> Result<(), IoError> {{\n    \
             var line = label\n    \
             for item in s.to_list() {{\n        \
                 line = \"{{line}} {{item}}\"\n    \
             }}\n    \
             io.println(line)\n\
         }}\n\n\
         fn yes(b: Bool) -> String {{\n    \
             if b {{ \"true\" }} else {{ \"false\" }}\n\
         }}\n\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             var s: Set<String> = Set.new()\n{body}\n    Ok(())\n}}\n"
    )
}

#[test]
fn set_needs_no_import() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Int> = Set.new()
    let _a = s.add(1)
    let made: Set<Int> = Set.of([1])
    let limited: Set<Int> = Set.with_max_entries(2)
    let n = s.len()
    let k = limited.len()
    let same = made == s
    io.println("{n} {k}")?
    io.println(if same { "same" } else { "differ" })?
    Ok(())
}
"#;
    assert_runs(src, &[], "1 0\nsame\n");
}

#[test]
fn a_program_that_writes_use_std_set_keeps_working() {
    let src = r#"use std.set

fn main(io: Stdout) -> Result<(), IoError> {
    var s: set.Set<String> = set.Set.new()
    let _a = s.add("a")
    var p: Set<String> = Set.new()
    let _b = p.add("a")
    let same = s == p
    io.println(if same { "same" } else { "differ" })?
    Ok(())
}
"#;
    assert_runs(src, &[], "same\n");
}

#[test]
fn to_list_follows_insertion_order() {
    let src = program(
        r#"    let _a = s.add("pear")
    let _b = s.add("apple")
    let _c = s.add("zebra")
    let _d = s.add("mango")
    show(io, "items:", s)?"#,
    );
    assert_runs(&src, &[], "items: pear apple zebra mango\n");
}

#[test]
fn of_drops_repeats_and_keeps_first_seen_order() {
    let src = program(
        r#"    s = Set.of(["b", "a", "b", "c", "a", "d"])
    show(io, "of:", s)?
    let n = s.len()
    io.println("{n}")?"#,
    );
    assert_runs(&src, &[], "of: b a c d\n4\n");
}

#[test]
fn add_says_whether_the_set_changed_and_keeps_a_held_items_place() {
    let src = program(
        r#"    let first = s.add("x")
    let _y = s.add("y")
    let again = s.add("x")
    let a = yes(first)
    let b = yes(again)
    io.println("{a} {b}")?
    show(io, "items:", s)?"#,
    );
    assert_runs(&src, &[], "true false\nitems: x y\n");
}

#[test]
fn remove_says_whether_it_held_the_item_and_never_panics() {
    let src = program(
        r#"    let _a = s.add("a")
    let _b = s.add("b")
    let _c = s.add("c")
    let gone = s.remove("b")
    let missing = s.remove("zzz")
    let again = s.remove("b")
    let a = yes(gone)
    let b = yes(missing)
    let c = yes(again)
    io.println("{a} {b} {c}")?
    show(io, "items:", s)?"#,
    );
    assert_runs(&src, &[], "true false false\nitems: a c\n");
}

#[test]
fn remove_then_add_puts_the_item_last() {
    let src = program(
        r#"    s = Set.of(["a", "b", "c"])
    let _gone = s.remove("a")
    let _back = s.add("a")
    show(io, "items:", s)?"#,
    );
    assert_runs(&src, &[], "items: b c a\n");
}

#[test]
fn contains_len_and_is_empty_follow_the_items() {
    let src = program(
        r#"    let empty = s.is_empty()
    let _a = s.add("a")
    let has = s.contains("a")
    let lacks = s.contains("b")
    let _gone = s.remove("a")
    let after = s.contains("a")
    let now_empty = s.is_empty()
    let n = s.len()
    let a = yes(empty)
    let b = yes(has)
    let c = yes(lacks)
    let d = yes(after)
    let e = yes(now_empty)
    io.println("{a} {b} {c} {d} {e} {n}")?"#,
    );
    assert_runs(&src, &[], "true true false false true 0\n");
}

#[test]
fn two_sets_are_equal_when_they_hold_the_same_items_in_the_same_order() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a: Set<Int> = Set.of([1, 2, 3])
    let b: Set<Int> = Set.of([1, 2, 3, 2])
    let c: Set<Int> = Set.of([3, 2, 1])
    let ab = a == b
    let ac = a == c
    let x = if ab { "true" } else { "false" }
    let y = if ac { "true" } else { "false" }
    io.println("{x} {y}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "true false\n");
}

#[test]
fn a_copy_does_not_see_a_later_change() {
    let src = program(
        r#"    let _a = s.add("a")
    var copy = s
    let _b = copy.add("b")
    let _c = s.add("c")
    show(io, "s:", s)?
    show(io, "copy:", copy)?"#,
    );
    assert_runs(&src, &[], "s: a c\ncopy: a b\n");
}

#[test]
fn a_set_of_structs_and_lists_uses_their_eq_and_hash() {
    let src = r#"struct P { x: Int, name: String }

impl Eq for P {
    fn eq(self, other: P) -> Bool {
        self.x == other.x && self.name == other.name
    }
}

impl Hash for P {
    fn hash(self) -> Int {
        self.x
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<P> = Set.new()
    let _a = s.add(P(x: 1, name: "a"))
    let _b = s.add(P(x: 1, name: "a"))
    let _c = s.add(P(x: 2, name: "a"))
    let n = s.len()
    var l: Set<List<Int>> = Set.new()
    let _d = l.add([1, 2])
    let _e = l.add([1, 2])
    let _f = l.add([2, 1])
    let m = l.len()
    io.println("{n} {m}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "2 2\n");
}

#[test]
fn a_big_set_finds_every_item() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Int> = Set.new()
    var i = 0
    while i < 2000 {
        let _a = s.add(i * 7)
        i = i + 1
    }
    var found = 0
    i = 0
    while i < 14000 {
        if s.contains(i) {
            found = found + 1
        }
        i = i + 1
    }
    var removed = 0
    i = 0
    while i < 2000 {
        if s.remove(i * 7) {
            removed = removed + 1
        }
        i = i + 2
    }
    let n = s.len()
    io.println("{found} {removed} {n}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "2000 1000 1000\n");
}

const ENTRY_LIMIT: &str = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Int> = Set.with_max_entries(1000)
    var i = 0
    while i < COUNT {
        let _a = s.add(i)
        i = i + 1
    }
    let n = s.len()
    io.println("{n} items")?
    Ok(())
}
"#;

#[test]
fn an_add_past_the_entry_limit_panics_with_p0044() {
    let outcome = run_on(FakeHost::new(), &ENTRY_LIMIT.replace("COUNT", "1001"), &[]);
    let panic = outcome.panic.as_ref().expect("the 1001st item panics");
    assert_eq!(panic.code, "P0044", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
    assert_runs(&ENTRY_LIMIT.replace("COUNT", "1000"), &[], "1000 items\n");
}

#[test]
fn an_add_of_a_held_item_at_the_limit_is_no_new_item() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Int> = Set.with_max_entries(2)
    let _a = s.add(1)
    let _b = s.add(2)
    let again = s.add(2)
    let gone = s.remove(1)
    let back = s.add(3)
    let a = if again { "true" } else { "false" }
    let b = if gone { "true" } else { "false" }
    let c = if back { "true" } else { "false" }
    io.println("{a} {b} {c}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "false true true\n");
}

#[test]
fn a_negative_limit_is_zero() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Int> = Set.with_max_entries(-5)
    let _a = s.add(1)
    io.println("unreached")?
    Ok(())
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("a zero limit panics");
    assert_eq!(panic.code, "P0044", "{outcome:#?}");
}

#[test]
fn items_that_collide_under_every_hash_panic_with_p0045() {
    let src = r#"struct Key { n: Int }

impl Eq for Key {
    fn eq(self, other: Key) -> Bool {
        self.n == other.n
    }
}

impl Hash for Key {
    fn hash(self) -> Int {
        7
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var s: Set<Key> = Set.new()
    var i = 0
    while i < 1000 {
        let _a = s.add(Key(n: i))
        i = i + 1
    }
    io.println("unreached")?
    Ok(())
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the flood panics");
    assert_eq!(panic.code, "P0045", "{outcome:#?}");
}

#[test]
fn under_max_memory_a_growing_set_stops_with_p0040() {
    let src = "fn main(io: Stdout) {
    var s: Set<Int> = Set.new()
    var i = 0
    while i < 1000000 {
        let _ = s.add(i)
        i = i + 1
    }
    let _ = io.println(\"unreached\")
}
";
    let outcome = common::run_under_limit(src, 1 << 20);
    let Some(panic) = &outcome.panic else {
        panic!("a million items pass 1 MiB: {outcome:#?}");
    };
    assert_eq!(panic.code, oska_run::codes::MEMORY_LIMIT, "{panic:#?}");
    assert_eq!(outcome.stdout, "");
}
