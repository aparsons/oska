// SPDX-License-Identifier: Apache-2.0
//! `Set<T>` operations (#803, spec #798, ADR 0032): `union`,
//! `intersection` and `difference`. Each helper that runs a program also
//! runs it natively on Linux and compares (`common/mod.rs`), so each
//! expected output holds on the interpreter and the native backend alike.

mod common;

use common::{assert_runs, run_on};
use oska_run::FakeHost;

/// A program with `show` and two sets, `a` = pear apple zebra and
/// `b` = apple mango pear kiwi, then the given `body` in `main`.
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
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             let a: Set<String> = Set.of([\"pear\", \"apple\", \"zebra\"])\n    \
             let b: Set<String> = Set.of([\"apple\", \"mango\", \"pear\", \"kiwi\"])\n{body}\n    Ok(())\n}}\n"
    )
}

#[test]
fn union_is_the_left_items_then_the_new_right_items() {
    let src = program(
        r#"    show(io, "ab:", a.union(b))?
    show(io, "ba:", b.union(a))?"#,
    );
    assert_runs(
        &src,
        &[],
        "ab: pear apple zebra mango kiwi\nba: apple mango pear kiwi zebra\n",
    );
}

#[test]
fn intersection_keeps_the_left_order() {
    let src = program(
        r#"    show(io, "ab:", a.intersection(b))?
    show(io, "ba:", b.intersection(a))?"#,
    );
    assert_runs(&src, &[], "ab: pear apple\nba: apple pear\n");
}

#[test]
fn difference_keeps_the_left_order() {
    let src = program(
        r#"    show(io, "ab:", a.difference(b))?
    show(io, "ba:", b.difference(a))?"#,
    );
    assert_runs(&src, &[], "ab: zebra\nba: mango kiwi\n");
}

#[test]
fn an_operation_makes_a_new_set_and_changes_neither_operand() {
    let src = program(
        r#"    var u = a.union(b)
    let _x = u.add("extra")
    let _y = u.remove("pear")
    var i = a.intersection(b)
    let _z = i.add("extra")
    var d = a.difference(b)
    let _w = d.add("extra")
    show(io, "a:", a)?
    show(io, "b:", b)?
    show(io, "u:", u)?"#,
    );
    assert_runs(
        &src,
        &[],
        "a: pear apple zebra\nb: apple mango pear kiwi\nu: apple zebra mango kiwi extra\n",
    );
}

#[test]
fn the_empty_set_and_the_same_set_give_the_plain_answers() {
    let src = program(
        r#"    let e: Set<String> = Set.new()
    show(io, "a+e:", a.union(e))?
    show(io, "e+a:", e.union(a))?
    show(io, "a&e:", a.intersection(e))?
    show(io, "a-e:", a.difference(e))?
    show(io, "e-a:", e.difference(a))?
    show(io, "a&a:", a.intersection(a))?
    show(io, "a-a:", a.difference(a))?
    show(io, "a+a:", a.union(a))?"#,
    );
    assert_runs(
        &src,
        &[],
        "a+e: pear apple zebra\ne+a: pear apple zebra\na&e:\na-e: pear apple zebra\ne-a:\n\
         a&a: pear apple zebra\na-a:\na+a: pear apple zebra\n",
    );
}

#[test]
fn a_union_past_the_entry_limit_panics_with_p0044() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var a: Set<Int> = Set.with_max_entries(3)
    let _a1 = a.add(1)
    let _a2 = a.add(2)
    var b: Set<Int> = Set.new()
    let _b1 = b.add(2)
    let _b2 = b.add(3)
    let fits = a.union(b)
    let n = fits.len()
    io.println("{n}")?
    let _b3 = b.add(4)
    let over = a.union(b)
    let m = over.len()
    io.println("unreached {m}")?
    Ok(())
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the union is past the limit");
    assert_eq!(panic.code, "P0044", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
}

#[test]
fn a_big_union_intersection_and_difference_agree_on_both_backends() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var a: Set<Int> = Set.new()
    var b: Set<Int> = Set.new()
    var i = 0
    while i < 300 {
        let _x = a.add(i)
        let _y = b.add(i * 2)
        i = i + 1
    }
    let u = a.union(b)
    let n = a.intersection(b)
    let d = a.difference(b)
    let ul = u.len()
    let nl = n.len()
    let dl = d.len()
    var first = -1
    for item in d.to_list() {
        if first < 0 {
            first = item
        }
    }
    var last = -1
    for item in u.to_list() {
        last = item
    }
    io.println("{ul} {nl} {dl} {first} {last}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "450 150 150 1 598\n");
}

#[test]
fn colliding_items_in_a_union_stop_with_p0045() {
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
    var a: Set<Key> = Set.new()
    var b: Set<Key> = Set.new()
    var i = 0
    while i < 20 {
        let _x = a.add(Key(n: i))
        let _y = b.add(Key(n: i + 1000))
        i = i + 1
    }
    var u = a
    var round = 0
    while round < 100 {
        u = u.union(b)
        let _z = b.add(Key(n: round + 5000))
        round = round + 1
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
fn a_hash_flood_through_intersection_and_difference_stops_with_p0045() {
    // The operands are small enough to build; the colliding items are what
    // the operation re-adds.
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
    var a: Set<Key> = Set.new()
    let b: Set<Key> = Set.new()
    var i = 0
    while i < 1000 {
        let _x = a.add(Key(n: i))
        i = i + 1
    }
    let d = a.difference(b)
    io.println("unreached")?
    Ok(())
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the flood panics");
    assert_eq!(panic.code, "P0045", "{outcome:#?}");
}
