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

/// Items that collide under every hash. One set holds at most 64 of them
/// (the probe limit of a `Map`), so an operand of 40 builds, and the
/// operation is what goes past it.
const KEY: &str = r#"struct Key { n: Int }

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

"#;

/// `build(a, b)` makes `a` of `Key(n: 0..na)` and `b` of
/// `Key(n: 1000..1000+nb)`, prints `built`, then runs `op`.
fn flood(na: i64, nb: i64, op: &str) -> String {
    format!(
        "{KEY}fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             var a: Set<Key> = Set.new()\n    \
             var b: Set<Key> = Set.new()\n    \
             var i = 0\n    \
             while i < {na} {{\n        \
                 let _x = a.add(Key(n: i))\n        \
                 i = i + 1\n    \
             }}\n    \
             i = 0\n    \
             while i < {nb} {{\n        \
                 let _y = b.add(Key(n: i + 1000))\n        \
                 i = i + 1\n    \
             }}\n    \
             io.println(\"built\")?\n{op}\n    Ok(())\n}}\n"
    )
}

#[test]
fn a_union_of_colliding_items_stops_with_p0045_after_both_operands_build() {
    // 40 + 40 colliding items: each operand builds, the union of 80 does not.
    let src = flood(
        40,
        40,
        "    let u = a.union(b)\n    let n = u.len()\n    io.println(\"unreached {n}\")?",
    );
    let outcome = run_on(FakeHost::new(), &src, &[]);
    assert_eq!(outcome.stdout, "built\n", "{outcome:#?}");
    let panic = outcome.panic.as_ref().expect("the union floods");
    assert_eq!(panic.code, "P0045", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
    // 30 + 30 fit under the probe limit, so the same union succeeds.
    let src = flood(
        30,
        30,
        "    let u = a.union(b)\n    let n = u.len()\n    io.println(\"{n}\")?",
    );
    assert_runs(&src, &[], "built\n60\n");
}

// `intersection` and `difference` cannot flood: each result is a subset of
// `self`, which already built. What they can do is finish, in `self`'s
// order, over items that all collide.
#[test]
fn intersection_and_difference_of_colliding_items_finish() {
    let src = flood(
        40,
        40,
        "    let i = a.intersection(b)\n    let d = a.difference(b)\n    let e = b.difference(a)\n    \
         let il = i.len()\n    let dl = d.len()\n    let el = e.len()\n    \
         io.println(\"{il} {dl} {el}\")?",
    );
    assert_runs(&src, &[], "built\n0 40 40\n");
}

/// `a` holds 1, 2, 3 under a limit of 3; `b` holds 2, 3. The `op` result
/// `r` takes `extra` more new items, and the program prints `ok` after.
const LIMITED: &str = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var a: Set<Int> = Set.with_max_entries(3)
    let _a1 = a.add(1)
    let _a2 = a.add(2)
    let _a3 = a.add(3)
    var b: Set<Int> = Set.new()
    let _b2 = b.add(2)
    let _b3 = b.add(3)
    var r = a.OP(b)
    let _r1 = r.add(10)
    let _r2 = r.add(11)
    let _r3 = r.add(12)
    io.println("ok")?
    Ok(())
}
"#;

#[test]
fn intersection_and_difference_keep_the_entry_limit_of_the_left_set() {
    // The intersection has 2 items and the difference 1, so room for 1 and
    // 2 more under the limit of 3: a third new item is past it.
    for op in ["intersection", "difference"] {
        let outcome = run_on(FakeHost::new(), &LIMITED.replace("OP", op), &[]);
        assert_eq!(outcome.stdout, "", "{op}: {outcome:#?}");
        let panic = outcome.panic.as_ref().expect("the result is at its limit");
        assert_eq!(panic.code, "P0044", "{op}: {outcome:#?}");
    }
}
