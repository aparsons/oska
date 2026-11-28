// SPDX-License-Identifier: Apache-2.0
//! `SortedMap` and `SortedSet` (#806, spec #888, ADR 0033): prelude values
//! written in Oska, read back in ascending `Ord` order. Each helper that
//! runs a program also runs it natively on Linux and compares.

mod common;

use common::{assert_refused, assert_runs, run_on, run_under_limit, PROGRAM_PATH};
use oska_run::{codes, run_with_limits, FakeHost, Limits};

/// Shared helpers: print a list of `Int` on one line.
const LINES: &str = r#"fn ints(items: List<Int>) -> String {
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

fn pairs(items: List<SortedEntry<Int, String>>) -> String {
    var out = ""
    for e in items {
        out = "{out}{e.key}={e.value} "
    }
    out
}

fn flags(items: List<Bool>) -> String {
    var out = ""
    var i = 0
    while i < items.len() {
        if i > 0 {
            out = "{out} "
        }
        if items.at(i) {
            out = "{out}true"
        } else {
            out = "{out}false"
        }
        i = i + 1
    }
    out
}
"#;

fn with_helpers(main: &str) -> String {
    format!("{LINES}\n{main}")
}

#[test]
fn a_sorted_set_lists_in_ascending_order_whatever_the_insertion_order() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: SortedSet<Int> = SortedSet.new()
    let a = s.add(5)
    let b = s.add(1)
    let c = s.add(9)
    let d = s.add(3)
    let e = s.add(5)
    io.println(ints(s.to_list()))?
    io.println(flags([a, b, c, d, e]))?
    let n = s.len()
    let f = s.first().unwrap_or(-1)
    let l = s.last().unwrap_or(-1)
    io.println("{n} {f} {l}")?
    let has = s.contains(3)
    let hasnt = s.contains(4)
    io.println(flags([has, hasnt]))
}
"#,
    );
    assert_runs(
        &src,
        &[],
        "[1, 3, 5, 9]\ntrue true true true false\n4 1 9\ntrue false\n",
    );
}

#[test]
fn a_sorted_set_removes_and_drops_repeats_in_of() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s = SortedSet.of([4, 2, 4, 8, 2, 6])
    io.println(ints(s.to_list()))?
    let r1 = s.remove(4)
    let r2 = s.remove(4)
    let r3 = s.remove(100)
    let r4 = s.remove(2)
    let r5 = s.remove(8)
    io.println(flags([r1, r2, r3, r4, r5]))?
    io.println(ints(s.to_list()))?
    let _ = s.remove(6)
    let empty = s.is_empty()
    let f = s.first().is_none()
    let l = s.last().is_none()
    io.println(flags([empty, f, l]))
}
"#,
    );
    assert_runs(
        &src,
        &[],
        "[2, 4, 6, 8]\ntrue false false true true\n[6]\ntrue true true\n",
    );
}

#[test]
fn a_sorted_map_puts_gets_and_removes_by_key() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var m: SortedMap<Int, String> = SortedMap.new()
    let a = m.put(30, "c")
    let b = m.put(10, "a")
    let c = m.put(20, "b")
    let d = m.put(10, "A")
    io.println(flags([a, b, c, d]))?
    io.println(pairs(m.to_list()))?
    let g = m.get(10).unwrap_or("none")
    let miss = m.get(11).unwrap_or("none")
    io.println("{g} {miss}")?
    let f = m.first().map(|e| e.key).unwrap_or(-1)
    let l = m.last().map(|e| e.value).unwrap_or("none")
    io.println("{f} {l}")?
    let r = m.remove(20).unwrap_or("none")
    let r2 = m.remove(20).unwrap_or("none")
    io.println("{r} {r2}")?
    io.println(pairs(m.to_list()))?
    let has = m.contains(30)
    let n = m.len()
    io.println(flags([has]))?
    io.println("{n}")
}
"#,
    );
    assert_runs(
        &src,
        &[],
        "true true true false\n10=A 20=b 30=c \nA none\n10 c\nb none\n10=A 30=c \ntrue\n2\n",
    );
}

#[test]
fn of_lets_the_last_value_win_in_a_map_and_drops_repeats_in_a_set() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let m: SortedMap<Int, String> = SortedMap.of([
        SortedEntry(key: 2, value: "x"),
        SortedEntry(key: 1, value: "y"),
        SortedEntry(key: 2, value: "z"),
    ])
    io.println(pairs(m.to_list()))?
    let s = SortedSet.of(["pear", "apple", "pear", "fig"])
    for item in s.to_list() {
        io.println(item)?
    }
    Ok(())
}
"#,
    );
    assert_runs(&src, &[], "1=y 2=z \napple\nfig\npear\n");
}

#[test]
fn empty_and_one_item_collections_have_first_and_last() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var s: SortedSet<Int> = SortedSet.new()
    var m: SortedMap<Int, Int> = SortedMap.new()
    io.println(flags([s.first().is_none(), s.last().is_none(), m.first().is_none(), m.last().is_none()]))?
    let g = m.get(1).is_none()
    let r = m.remove(1).is_none()
    io.println(flags([g, r]))?
    let _ = s.add(7)
    let _ = m.put(7, 70)
    let a = s.first().unwrap_or(0)
    let b = s.last().unwrap_or(0)
    let c = m.first().map(|e| e.value).unwrap_or(0)
    let d = m.last().map(|e| e.key).unwrap_or(0)
    io.println("{a} {b} {c} {d}")
}
"#,
    );
    assert_runs(&src, &[], "true true true true\ntrue true\n7 7 70 7\n");
}

#[test]
fn a_copy_made_before_a_change_does_not_see_it() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var a = SortedSet.of([3, 1])
    let before = a
    let _ = a.add(2)
    let _ = a.remove(1)
    io.println(ints(before.to_list()))?
    io.println(ints(a.to_list()))?
    var m: SortedMap<Int, String> = SortedMap.new()
    let _ = m.put(1, "one")
    let snap = m
    let _ = m.put(1, "uno")
    let _ = m.put(0, "zero")
    io.println(pairs(snap.to_list()))?
    io.println(pairs(m.to_list()))
}
"#,
    );
    assert_runs(&src, &[], "[1, 3]\n[2, 3]\n1=one \n0=zero 1=uno \n");
}

#[test]
fn two_collections_are_equal_when_their_lists_are() {
    let src = with_helpers(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = SortedSet.of([1, 2, 3])
    let b = SortedSet.of([3, 2, 1, 2])
    let c = SortedSet.of([1, 2])
    let m1: SortedMap<Int, String> = SortedMap.of([SortedEntry(key: 1, value: "a"), SortedEntry(key: 2, value: "b")])
    let m2: SortedMap<Int, String> = SortedMap.of([SortedEntry(key: 2, value: "b"), SortedEntry(key: 1, value: "a")])
    let m3: SortedMap<Int, String> = SortedMap.of([SortedEntry(key: 2, value: "b"), SortedEntry(key: 1, value: "z")])
    io.println(flags([a == b, a == c, a != c, m1 == m2, m1 == m3]))
}
"#,
    );
    assert_runs(&src, &[], "true false true true false\n");
}

#[test]
fn text_keys_order_by_code_point_and_options_and_lists_are_keys() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    let s = SortedSet.of(["b", "B", "a", "", "ab", "é"])
    for item in s.to_list() {
        io.println("[{item}]")?
    }
    let o: SortedSet<Option<Int>> = SortedSet.of([Some(2), None, Some(1)])
    let n = o.len()
    let head = o.first().unwrap_or(Some(-1)).unwrap_or(-5)
    io.println("{n} {head}")?
    let l: SortedSet<List<Int>> = SortedSet.of([[2], [1, 5], [1], []])
    for item in l.to_list() {
        let k = item.len()
        io.println("{k}")?
    }
    Ok(())
}
"#;
    assert_runs(src, &[], "[]\n[B]\n[a]\n[ab]\n[b]\n[é]\n3 -5\n0\n1\n2\n1\n");
}

#[test]
fn a_user_type_with_impl_ord_is_a_key() {
    let src = r#"struct Version { major: Int, minor: Int }
impl Eq for Version {
    fn eq(self, other: Version) -> Bool {
        self.major == other.major && self.minor == other.minor
    }
}
impl Ord for Version {
    fn cmp(self, other: Version) -> Ordering {
        let by_major = self.major.cmp(other.major)
        match by_major {
            Ordering.Equal => self.minor.cmp(other.minor)
            _ => by_major
        }
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var m: SortedMap<Version, String> = SortedMap.new()
    let _ = m.put(Version(major: 2, minor: 0), "two")
    let _ = m.put(Version(major: 1, minor: 9), "one-nine")
    let _ = m.put(Version(major: 1, minor: 2), "one-two")
    let _ = m.put(Version(major: 1, minor: 9), "again")
    for e in m.to_list() {
        let a = e.key.major
        let b = e.key.minor
        io.println("{a}.{b} {e.value}")?
    }
    Ok(())
}
"#;
    assert_runs(src, &[], "1.2 one-two\n1.9 again\n2.0 two\n");
}

#[test]
fn a_float_key_and_a_secret_key_are_refused_like_the_ord_refusal() {
    for ty in ["SortedSet<Float>", "SortedMap<Float, Int>"] {
        let src = format!("fn main() {{\n    let s: {ty} = {}.new()\n}}\n", &ty[..9]);
        assert_refused(&src, "E0265");
    }
    let src = "fn main() {\n    let s: SortedSet<Secret<Bytes>> = SortedSet.new()\n}\n";
    assert_refused(src, "E0356");
}

#[test]
fn random_puts_and_removes_match_a_plain_sorted_list_model() {
    let src = r#"fn insert(model: List<Int>, x: Int) -> List<Int> {
    var out: List<Int> = []
    var done = false
    for y in model {
        if !done && x < y {
            out.push(x)
            done = true
        }
        if y == x {
            done = true
        }
        out.push(y)
    }
    if !done {
        out.push(x)
    }
    out
}

fn erase(model: List<Int>, x: Int) -> List<Int> {
    var out: List<Int> = []
    for y in model {
        if y != x {
            out.push(y)
        }
    }
    out
}

fn main(io: Stdout) -> Result<(), IoError> {
    var s: SortedSet<Int> = SortedSet.new()
    var m: SortedMap<Int, Int> = SortedMap.new()
    var model: List<Int> = []
    var seed = 12345
    var step = 0
    var bad = 0
    while step < 600 {
        seed = (seed * 1103515245 + 12345) % 2147483648
        let x = (seed / 65536) % 40
        if (seed / 7) % 3 == 0 {
            let had = s.remove(x)
            let had_m = m.remove(x).is_some()
            let expect = model.contains(x)
            if had != expect || had_m != expect {
                bad = bad + 1
            }
            model = erase(model, x)
        } else {
            let fresh = s.add(x)
            let fresh_m = m.put(x, step)
            let expect = !model.contains(x)
            if fresh != expect || fresh_m != expect {
                bad = bad + 1
            }
            model = insert(model, x)
        }
        let listed = s.to_list()
        if listed != model {
            bad = bad + 1
        }
        let keys = m.to_list()
        if keys.len() != model.len() {
            bad = bad + 1
        } else {
            var i = 0
            while i < keys.len() {
                if keys.at(i).key != model.at(i) {
                    bad = bad + 1
                }
                i = i + 1
            }
        }
        step = step + 1
    }
    let n = s.len()
    io.println("{bad} {n}")
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    let stdout = outcome.stdout.trim().to_string();
    assert!(stdout.starts_with("0 "), "the model disagreed: {stdout}");
}

/// A key whose `cmp` is not a total order, by the body of `cmp`.
fn bad_order(cmp_body: &str) -> String {
    format!(
        r#"struct K {{ n: Int }}
impl Eq for K {{
    fn eq(self, other: K) -> Bool {{ self.n == other.n }}
}}
impl Ord for K {{
    fn cmp(self, other: K) -> Ordering {{
{cmp_body}
    }}
}}

fn main(io: Stdout) -> Result<(), IoError> {{
    var s: SortedSet<K> = SortedSet.new()
    var m: SortedMap<K, Int> = SortedMap.new()
    var seed = 99
    var i = 0
    while i < 300 {{
        seed = (seed * 1103515245 + 12345) % 2147483648
        let k = K(n: (seed / 65536) % 50)
        let _ = s.add(k)
        let _ = m.put(k, i)
        let _ = s.contains(k)
        let _ = m.get(k)
        if i % 3 == 0 {{
            let _ = s.remove(k)
            let _ = m.remove(k)
        }}
        i = i + 1
    }}
    let _ = s.len()
    let _ = m.len()
    let _ = s.first()
    let _ = m.last()
    let _ = s.to_list()
    io.println("done")
}}
"#
    )
}

fn assert_bad_order_ends(cmp_body: &str) {
    let src = bad_order(cmp_body);
    // Both backends run it (the helper compares them): a wrong answer at
    // worst, never a panic of memory or a step overrun.
    let outcome = run_on(FakeHost::new(), &src, &[]);
    assert!(outcome.diagnostics.is_empty(), "{outcome:#?}");
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "done\n", "{outcome:#?}");
    // And inside a step limit set well above what the honest program needs.
    let limits = Limits {
        max_steps: Some(5_000_000),
        ..Limits::NONE
    };
    let bounded = run_with_limits(
        &src,
        &mut FakeHost::new(),
        &[PROGRAM_PATH.to_string()],
        limits,
    );
    assert!(bounded.panic.is_none(), "{bounded:#?}");
    assert_eq!(bounded.stdout, "done\n");
}

#[test]
fn a_constant_cmp_gives_a_wrong_answer_never_a_memory_error() {
    assert_bad_order_ends("        Ordering.Less");
    assert_bad_order_ends("        Ordering.Greater");
    assert_bad_order_ends("        Ordering.Equal");
}

#[test]
fn a_cmp_that_flips_with_its_operands_stays_inside_the_limits() {
    assert_bad_order_ends(
        "        if (self.n + other.n) % 2 == 0 { Ordering.Less } else { Ordering.Greater }",
    );
    assert_bad_order_ends("        if self.n % 2 == 0 { Ordering.Less } else { Ordering.Greater }");
}

#[test]
fn a_pseudo_random_cmp_stays_inside_the_limits() {
    assert_bad_order_ends(
        "        let mixed = (self.n * 31 + other.n * 17 + self.n * other.n) % 3
        match mixed {
            0 => Ordering.Less
            1 => Ordering.Equal
            _ => Ordering.Greater
        }",
    );
}

#[test]
fn an_endless_build_stops_at_max_steps() {
    // The inconsistent `cmp` cannot make a call run for ever: a loop that
    // never ends on its own is stopped by the limit, not by the collection.
    let src = r#"fn main(io: Stdout) {
    var s: SortedSet<Int> = SortedSet.new()
    var i = 0
    while true {
        let _ = s.add(i % 1000)
        i = i + 1
    }
}
"#;
    let limits = Limits {
        max_steps: Some(100_000),
        ..Limits::NONE
    };
    let outcome = run_with_limits(
        src,
        &mut FakeHost::new(),
        &[PROGRAM_PATH.to_string()],
        limits,
    );
    let panic = outcome.panic.as_ref().expect("the step limit stops it");
    assert_eq!(panic.code, "P0038", "{outcome:#?}");
}

#[test]
fn under_max_memory_a_growing_sorted_set_stops_with_p0040() {
    let src = "fn main(io: Stdout) {
    var s: SortedSet<Int> = SortedSet.new()
    var i = 0
    while i < 1000000 {
        let _ = s.add(i)
        i = i + 1
    }
    let _ = io.println(\"unreached\")
}
";
    let outcome = run_under_limit(src, 1 << 20);
    let Some(panic) = &outcome.panic else {
        panic!("a million items pass 1 MiB: {outcome:#?}");
    };
    assert_eq!(panic.code, codes::MEMORY_LIMIT, "{panic:#?}");
    assert_eq!(outcome.stdout, "");
}

#[test]
fn under_max_memory_a_growing_sorted_map_stops_with_p0040() {
    let src = "fn main(io: Stdout) {
    var m: SortedMap<Int, Int> = SortedMap.new()
    var i = 0
    while i < 1000000 {
        let _ = m.put(i, i)
        i = i + 1
    }
    let _ = io.println(\"unreached\")
}
";
    let outcome = run_under_limit(src, 1 << 20);
    let Some(panic) = &outcome.panic else {
        panic!("a million entries pass 1 MiB: {outcome:#?}");
    };
    assert_eq!(panic.code, codes::MEMORY_LIMIT, "{panic:#?}");
}
