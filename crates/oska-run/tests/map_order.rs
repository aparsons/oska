// SPDX-License-Identifier: Apache-2.0
//! `std.map`'s `Map<K, V>` (#601, ADR 0028): the insertion-ordered table.
//!
//! Every order here is pinned exactly. Each helper that runs a program also
//! runs it natively on Linux and compares (`common/mod.rs`), so each
//! expected output holds on the interpreter and the native backend alike.

mod common;

use common::assert_runs;

/// A program that imports `std.map`, with a `show` that prints a
/// `Map<String, Int>` as `key=value` pairs on one line, and the given
/// `body` as `main`'s statements.
fn program(body: &str) -> String {
    format!(
        "use std.map\n\n\
         fn show(io: Stdout, label: String, m: map.Map<String, Int>) -> Result<(), IoError> {{\n    \
             var line = label\n    \
             for e in m.entries() {{\n        \
                 let n = e.value\n        \
                 line = \"{{line}} {{e.key}}={{n}}\"\n    \
             }}\n    \
             io.println(line)\n\
         }}\n\n\
         fn yes(b: Bool) -> String {{\n    \
             if b {{ \"true\" }} else {{ \"false\" }}\n\
         }}\n\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             var m: map.Map<String, Int> = map.Map.new()\n{body}\n    Ok(())\n}}\n"
    )
}

#[test]
fn entries_keys_and_values_follow_insertion_order() {
    let src = program(
        r#"    m.put("pear", 1)
    m.put("apple", 2)
    m.put("zebra", 3)
    m.put("mango", 4)
    show(io, "entries:", m)?
    let keys = m.keys()
    let values = m.values()
    var text = "keys:"
    for k in keys {
        text = "{text} {k}"
    }
    io.println(text)?
    var sum = ""
    for v in values {
        sum = "{sum} {v}"
    }
    io.println("values:{sum}")?"#,
    );
    assert_runs(
        &src,
        &[],
        "entries: pear=1 apple=2 zebra=3 mango=4\nkeys: pear apple zebra mango\nvalues: 1 2 3 4\n",
    );
}

#[test]
fn an_update_keeps_the_keys_place() {
    let src = program(
        r#"    m.put("a", 1)
    m.put("b", 2)
    m.put("c", 3)
    m.put("a", 10)
    m.put("b", 20)
    show(io, "after:", m)?
    let n = m.len()
    io.println("len {n}")?"#,
    );
    assert_runs(&src, &[], "after: a=10 b=20 c=3\nlen 3\n");
}

#[test]
fn remove_then_put_moves_the_key_to_the_end() {
    let src = program(
        r#"    m.put("a", 1)
    m.put("b", 2)
    m.put("c", 3)
    let gone = m.remove("a")
    match gone {
        Some(v) => io.println("removed {v}")?
        None => io.println("nothing")?
    }
    show(io, "after remove:", m)?
    m.put("a", 4)
    show(io, "after put:", m)?
    let again = m.remove("zzz")
    match again {
        Some(v) => io.println("removed {v}")?
        None => io.println("nothing")?
    }
    show(io, "unchanged:", m)?"#,
    );
    assert_runs(
        &src,
        &[],
        "removed 1\nafter remove: b=2 c=3\nafter put: b=2 c=3 a=4\nnothing\nunchanged: b=2 c=3 a=4\n",
    );
}

#[test]
fn a_missing_key_gives_none_and_never_a_panic() {
    let src = program(
        r#"    let empty = m.get("x")
    match empty {
        Some(v) => io.println("some {v}")?
        None => io.println("none")?
    }
    let none_removed = m.remove("x")
    match none_removed {
        Some(v) => io.println("some {v}")?
        None => io.println("none")?
    }
    let a = m.is_empty()
    let b = m.contains_key("x")
    let sa = yes(a)
    let sb = yes(b)
    io.println("{sa} {sb}")?
    m.put("x", 7)
    m.put("y", 8)
    let c = m.is_empty()
    let d = m.contains_key("x")
    let e = m.contains_key("w")
    let sc = yes(c)
    let sd = yes(d)
    let se = yes(e)
    io.println("{sc} {sd} {se}")?
    let found = m.get("y")
    match found {
        Some(v) => io.println("y is {v}")?
        None => io.println("no y")?
    }
    let missing = m.get("w")
    match missing {
        Some(v) => io.println("some {v}")?
        None => io.println("none")?
    }"#,
    );
    assert_runs(
        &src,
        &[],
        "none\nnone\ntrue false\nfalse true false\ny is 8\nnone\n",
    );
}

#[test]
fn a_map_is_a_value_copied_on_write() {
    let src = program(
        r#"    m.put("a", 1)
    m.put("b", 2)
    var copy = m
    copy.put("c", 3)
    copy.put("a", 9)
    let _gone = copy.remove("b")
    show(io, "original:", m)?
    show(io, "copy:", copy)?"#,
    );
    assert_runs(&src, &[], "original: a=1 b=2\ncopy: a=9 c=3\n");
}

#[test]
fn equality_is_by_content_and_order() {
    let src = program(
        r#"    var one: map.Map<String, Int> = map.Map.new()
    var two: map.Map<String, Int> = map.Map.new()
    var swapped: map.Map<String, Int> = map.Map.new()
    var other: map.Map<String, Int> = map.Map.new()
    one.put("a", 1)
    one.put("b", 2)
    two.put("a", 1)
    two.put("b", 2)
    swapped.put("b", 2)
    swapped.put("a", 1)
    other.put("a", 1)
    other.put("b", 3)
    let same = one == two
    let reordered = one == swapped
    let different = one == other
    let s1 = yes(same)
    let s2 = yes(reordered)
    let s3 = yes(different)
    io.println("{s1} {s2} {s3}")?
    let _gone = two.remove("a")
    two.put("a", 1)
    let moved = one == two
    let s4 = yes(moved)
    io.println("{s4}")?"#,
    );
    assert_runs(&src, &[], "true false false\nfalse\n");
}

#[test]
fn many_keys_keep_their_order_through_growth_and_compaction() {
    // 300 puts, then every key that is a multiple of 3 removed, then those
    // below 50 put again, then every multiple of 7 put (an update, or a
    // new key at the end). The expected values come from Python's
    // `OrderedDict`, which has the same rules.
    let src = program(
        r#"    var i = 0
    while i < 300 {
        m.put("k{i}", i)
        i = i + 1
    }
    i = 0
    while i < 300 {
        let _gone = m.remove("k{i}")
        i = i + 3
    }
    i = 0
    while i < 50 {
        m.put("k{i}", i + 1000)
        i = i + 3
    }
    i = 0
    while i < 300 {
        m.put("k{i}", i * 2)
        i = i + 7
    }
    var checksum = 0
    var position = 0
    for e in m.entries() {
        checksum = (checksum * 31 + e.value + position) % 1000003
        position = position + 1
    }
    let n = m.len()
    io.println("len {n} checksum {checksum}")?
    let mut_first = m.keys().at(0)
    let last = m.keys().at(n - 1)
    io.println("first {mut_first} last {last}")?
    var wrong = 0
    i = 0
    while i < 300 {
        let got = m.get("k{i}")
        let expected = i % 3 != 0 || i < 50 || i % 7 == 0
        match got {
            Some(_) => { if !expected { wrong = wrong + 1 } }
            None => { if expected { wrong = wrong + 1 } }
        }
        i = i + 1
    }
    io.println("wrong {wrong}")?"#,
    );
    assert_runs(
        &src,
        &[],
        "len 229 checksum 361935\nfirst k1 last k294\nwrong 0\n",
    );
}

#[test]
fn integer_and_struct_keys_work() {
    let src = r#"use std.map

struct Point { x: Int, y: Int }

impl Eq for Point {
    fn eq(self, other: Point) -> Bool {
        self.x == other.x && self.y == other.y
    }
}

impl Hash for Point {
    fn hash(self) -> Int {
        self.x * 1000003 + self.y
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var ints: map.Map<Int, String> = map.Map.new()
    ints.put(30, "c")
    ints.put(-1, "a")
    ints.put(7, "b")
    ints.put(-1, "z")
    var text = ""
    for e in ints.entries() {
        let k = e.key
        text = "{text} {k}={e.value}"
    }
    io.println(text)?
    var points: map.Map<Point, Int> = map.Map.new()
    points.put(Point(x: 1, y: 2), 10)
    points.put(Point(x: 2, y: 1), 20)
    points.put(Point(x: 1, y: 2), 30)
    var shown = ""
    for e in points.entries() {
        let a = e.key.x
        let b = e.key.y
        let v = e.value
        shown = "{shown} ({a},{b})={v}"
    }
    io.println(shown)?
    let found = points.get(Point(x: 2, y: 1))
    match found {
        Some(v) => io.println("found {v}")?
        None => io.println("missing")?
    }
    Ok(())
}
"#;
    assert_runs(src, &[], " 30=c -1=z 7=b\n (1,2)=30 (2,1)=20\nfound 20\n");
}

#[test]
fn random_sequences_match_a_plain_list_model_entry_by_entry() {
    // A fixed-seed generator drives inserts, updates, removes and lookups
    // over a small key space (so collisions, tombstones and compaction all
    // happen) on a `Map` and on a list of pairs, and compares them after
    // every step. The program prints the number of disagreements, which
    // must be 0, and a digest of the final entries, which pins the order (the same
    // sequence on Python's `OrderedDict` gave it).
    let src = r#"use std.map

fn index_of(keys: List<Int>, key: Int) -> Int {
    var i = 0
    while i < keys.len() {
        if keys.at(i) == key {
            return i
        }
        i = i + 1
    }
    -1
}

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.new()
    var keys: List<Int> = []
    var values: List<Int> = []
    var seed = 20260101
    var step = 0
    var wrong = 0
    while step < 3000 {
        seed = (seed * 1103515245 + 12345) % 2147483648
        let action = seed / 65536 % 10
        seed = (seed * 1103515245 + 12345) % 2147483648
        let key = seed / 65536 % 97
        seed = (seed * 1103515245 + 12345) % 2147483648
        let value = seed / 65536
        let at = index_of(keys, key)
        if action < 5 {
            m.put(key, value)
            if at >= 0 {
                var nv: List<Int> = []
                var j = 0
                while j < values.len() {
                    nv.push(if j == at { value } else { values.at(j) })
                    j = j + 1
                }
                values = nv
            } else {
                keys.push(key)
                values.push(value)
            }
        } else if action < 8 {
            let removed = m.remove(key)
            if at >= 0 {
                match removed {
                    Some(v) => { if v != values.at(at) { wrong = wrong + 1 } }
                    None => { wrong = wrong + 1 }
                }
                var nk: List<Int> = []
                var nv: List<Int> = []
                var j = 0
                while j < keys.len() {
                    if j != at {
                        nk.push(keys.at(j))
                        nv.push(values.at(j))
                    }
                    j = j + 1
                }
                keys = nk
                values = nv
            } else {
                match removed {
                    Some(_) => { wrong = wrong + 1 }
                    None => {}
                }
            }
        } else {
            let got = m.get(key)
            match got {
                Some(v) => { if at < 0 || v != values.at(at) { wrong = wrong + 1 } }
                None => { if at >= 0 { wrong = wrong + 1 } }
            }
        }
        let held = m.entries()
        if held.len() != keys.len() || m.len() != keys.len() {
            wrong = wrong + 1
        } else {
            var j = 0
            while j < keys.len() {
                if held.at(j).key != keys.at(j) || held.at(j).value != values.at(j) {
                    wrong = wrong + 1
                }
                j = j + 1
            }
        }
        step = step + 1
    }
    var digest = 0
    for e in m.entries() {
        digest = (digest * 131 + e.key * 7 + e.value) % 1000000007
    }
    let n = m.len()
    io.println("wrong {wrong} len {n} digest {digest}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "wrong 0 len 62 digest 745069750\n");
}

#[test]
fn equality_of_a_generic_struct_holding_a_list_checks_and_runs() {
    // `==` through a generic `Eq` impl of a struct with a `List<K>` field
    // used to crash the checker (a trial copy's trust variable leaked into
    // the recorded type arguments); `Map`'s equality is that shape.
    let src = r#"struct Pair<K> { items: List<K>, n: Int }

impl<K: Eq> Eq for Pair<K> {
    fn eq(self, other: Pair<K>) -> Bool {
        self.n == other.n
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let a: Pair<String> = Pair(items: [], n: 1)
    let b: Pair<String> = Pair(items: [], n: 1)
    if a == b {
        io.println("equal")?
    }
    Ok(())
}
"#;
    assert_runs(src, &[], "equal\n");
}
