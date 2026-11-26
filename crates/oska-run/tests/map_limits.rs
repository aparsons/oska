// SPDX-License-Identifier: Apache-2.0
//! `std.map`'s limits (#603, ADR 0028): the probe limit with its second
//! and third hash, `P0045` when every hash is flooded, the entry limit
//! and `P0044`, and a flood of 100,000 colliding keys that must neither
//! run unbounded nor take more steps than a budget.
//!
//! The same-step check on both backends is `crates/oskac/tests/map_steps.rs`
//! (ADR 0021): the step limit and the native backend live there.

mod common;

use common::{assert_runs, run_on, PROGRAM_PATH};
use oska_run::{run_with_limits, FakeHost, Limits, RunOutcome};

/// Run `src` on `host` under `limits`, on the interpreter only: the step
/// limit's native side is `crates/oskac/tests/map_steps.rs`.
fn run_with_limits_on(host: &mut FakeHost, src: &str, args: &[&str], limits: Limits) -> RunOutcome {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run_with_limits(src, host, &args, limits)
}

/// FNV-1a's prime, and its offset basis, as `std.rt`'s `bytes_hash` and
/// the prelude's `Hash for String` use them (ADR 0028).
const PRIME: u64 = 1_099_511_628_211;
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// The keys' first hash agrees on this many low bits: every table up to
/// `1 << COLLIDING_BITS` slots puts them all in one slot.
const COLLIDING_BITS: u32 = 20;

/// The FNV-1a hash of `bytes`: `String.hash`.
fn fnv(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
}

/// The inverse of the odd `n` modulo 2^64.
fn inverse(n: u64) -> u64 {
    let mut x = n;
    for _ in 0..6 {
        x = x.wrapping_mul(2u64.wrapping_sub(n.wrapping_mul(x)));
    }
    x
}

/// `count` distinct keys whose first hash is 0 in its low
/// [`COLLIDING_BITS`] bits, as an attacker who knows the hash picks them.
///
/// Each is a 3-character head and a 4-character tail of printable ASCII.
/// FNV-1a's step (xor a byte, multiply by an odd number) is a bijection on
/// the low bits, so for each tail the head state that ends at 0 is one
/// number; the heads are found by their state.
fn colliding_keys(count: usize) -> Vec<String> {
    let mask = (1u64 << COLLIDING_BITS) - 1;
    let alphabet: Vec<u8> = (b' '..=b'~').collect();
    let n = alphabet.len();
    let mut heads: std::collections::HashMap<u64, Vec<[u8; 3]>> = std::collections::HashMap::new();
    for &a in &alphabet {
        for &b in &alphabet {
            for &c in &alphabet {
                let state = fnv(&[a, b, c]) & mask;
                heads.entry(state).or_default().push([a, b, c]);
            }
        }
    }
    let back = inverse(PRIME);
    let mut keys = Vec::with_capacity(count);
    for i in 0..n.pow(4) {
        let tail = [
            alphabet[i % n],
            alphabet[i / n % n],
            alphabet[i / (n * n) % n],
            alphabet[i / (n * n * n)],
        ];
        // The head's state that the tail's bytes take to 0, undone from the end.
        let mut need = 0u64;
        for &byte in tail.iter().rev() {
            need = (need.wrapping_mul(back) & mask) ^ (u64::from(byte) & mask);
        }
        if let Some(found) = heads.get(&need) {
            for head in found {
                let mut key = head.to_vec();
                key.extend_from_slice(&tail);
                assert_eq!(fnv(&key) & mask, 0, "{key:?} should collide");
                keys.push(String::from_utf8(key).expect("printable ASCII"));
                if keys.len() == count {
                    return keys;
                }
            }
        }
    }
    panic!("fewer than {count} colliding keys");
}

/// A program that reads the keys, one a line, from `keys.txt` and puts
/// each in a `Map<String, Int>`, then prints its length and the value of
/// the first and the last key.
const FLOOD: &str = r#"use std.map

fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let text = read.restrict_to_file("keys.txt").read_to_string()?.assume_trusted()
    var m: map.Map<String, Int> = map.Map.new()
    var i = 0
    for key in text.lines() {
        m.put(key, i)
        i = i + 1
    }
    let n = m.len()
    io.println("{n} entries")?
    var again = 0
    for key in text.lines() {
        match m.get(key) {
            Some(v) => { again = again + v }
            None => { again = again - 1 }
        }
    }
    io.println("sum {again}")?
    Ok(())
}
"#;

fn flood_host(keys: &[String]) -> FakeHost {
    FakeHost::new().with_file("keys.txt", &keys.join("\n"))
}

/// The steps the flood may take: a map of 100,000 entries does about 100
/// steps a key with the flood defence, and over 5 billion without it.
const FLOOD_BUDGET: u64 = 100_000_000;

#[test]
fn the_flood_keys_collide_under_the_first_hash() {
    let keys = colliding_keys(100_000);
    let mask = (1u64 << COLLIDING_BITS) - 1;
    assert_eq!(keys.len(), 100_000);
    let mut distinct = keys.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), keys.len());
    assert!(keys.iter().all(|k| fnv(k.as_bytes()) & mask == 0));
}

/// The attack test: 100,000 keys that collide under the first hash finish
/// within the step budget (the probe limit moved them to a second hash),
/// or panic with the stable code, never run unbounded. Without the flood
/// defence each insert probes every key before it: 5 billion steps, and
/// the budget stops it with `P0038`.
#[test]
fn a_flood_of_100000_colliding_keys_finishes_within_the_step_budget() {
    let keys = colliding_keys(100_000);
    let mut host = flood_host(&keys);
    let limits = Limits {
        max_steps: Some(FLOOD_BUDGET),
        ..Limits::NONE
    };
    let outcome = run_with_limits_on(&mut host, FLOOD, &[PROGRAM_PATH], limits);
    let stopped = outcome.panic.as_ref().map(|p| p.code);
    assert_ne!(
        stopped,
        Some("P0038"),
        "the flood ran past the step budget: {outcome:#?}"
    );
    assert!(
        stopped.is_none() || stopped == Some("P0045"),
        "the flood stopped with {stopped:?}"
    );
    // These keys agree only in the low bits of their first hash, so the
    // second hash spreads them and the map takes all.
    assert!(outcome.diagnostics.is_empty(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "100000 entries\nsum 4999950000\n");
}

/// The same flood on both backends, with no limit: the defence makes it
/// fast enough to run without one, and native code agrees.
#[test]
fn the_flood_runs_the_same_natively() {
    let keys = colliding_keys(20_000);
    let outcome = run_on(flood_host(&keys), FLOOD, &[]);
    assert!(
        outcome.panic.is_none() && outcome.diagnostics.is_empty(),
        "{outcome:#?}"
    );
    assert_eq!(outcome.stdout, "20000 entries\nsum 199990000\n");
}

/// A key whose hash is one number whatever it holds, and so collides under
/// every hash of the list: nothing but a panic can bound it.
const SAME_HASH: &str = r#"use std.map

struct Key { n: Int }

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
    var m: map.Map<Key, Int> = map.Map.new()
    var i = 0
    while i < LIMIT {
        m.put(Key(n: i), i)
        i = i + 1
    }
    let n = m.len()
    io.println("{n} entries")?
    Ok(())
}
"#;

#[test]
fn keys_that_collide_under_every_hash_panic_with_p0045() {
    let src = SAME_HASH.replace("LIMIT", "1000");
    let outcome = run_on(FakeHost::new(), &src, &[]);
    let panic = outcome.panic.as_ref().expect("the flood should panic");
    assert_eq!(panic.code, "P0045", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
    assert_eq!(outcome.stdout, "");
    assert!(panic.message.contains("collide"), "{}", panic.message);
}

#[test]
fn colliding_keys_up_to_the_probe_limit_do_not_panic() {
    // 64 slots of one chain are within the limit; the map keeps working.
    let src = SAME_HASH.replace("LIMIT", "60");
    assert_runs(&src, &[], "60 entries\n");
}

const ENTRY_LIMIT: &str = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.with_max_entries(1000)
    var i = 0
    while i < COUNT {
        m.put(i, i)
        i = i + 1
    }
    let n = m.len()
    io.println("{n} entries")?
    Ok(())
}
"#;

/// An insert loop stops at the entry limit, with `P0044`, and not before:
/// 1000 entries are fine.
#[test]
fn an_insert_loop_stops_at_the_entry_limit_with_p0044() {
    let outcome = run_on(FakeHost::new(), &ENTRY_LIMIT.replace("COUNT", "1001"), &[]);
    let panic = outcome.panic.as_ref().expect("the 1001st entry panics");
    assert_eq!(panic.code, "P0044", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
    assert_eq!(outcome.stdout, "");
    assert_runs(&ENTRY_LIMIT.replace("COUNT", "1000"), &[], "1000 entries\n");
}

/// An update of a key the map holds, and a removal followed by an insert,
/// are no new entry: at the limit they still work.
#[test]
fn an_update_or_a_remove_then_put_at_the_limit_is_no_new_entry() {
    let src = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.with_max_entries(3)
    m.put(1, 1)
    m.put(2, 2)
    m.put(3, 3)
    m.put(2, 20)
    let _gone = m.remove(1)
    m.put(4, 4)
    var line = ""
    for e in m.entries() {
        let k = e.key
        let v = e.value
        line = "{line} {k}={v}"
    }
    io.println(line)?
    Ok(())
}
"#;
    assert_runs(src, &[], " 2=20 3=3 4=4\n");
}

/// The limit is a number a program picks, never more than the default.
#[test]
fn a_limit_above_the_default_is_the_default() {
    let src = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.with_max_entries(9999999999)
    m.put(1, 1)
    let n = m.len()
    io.println("{n}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "1\n");
}

/// Updating and removing keys of a large map takes a number of steps that
/// does not grow with the table: 4,000 updates of a 4,000-entry map stay
/// within a budget of a few hundred steps each.
#[test]
fn updates_of_a_large_map_take_steps_that_do_not_grow_with_its_size() {
    let src = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.new()
    var i = 0
    while i < 4000 {
        m.put(i, i)
        i = i + 1
    }
    i = 0
    while i < 4000 {
        m.put(i, i + 1)
        i = i + 1
    }
    let n = m.len()
    io.println("{n}")?
    Ok(())
}
"#;
    let limits = Limits {
        max_steps: Some(4000 * 250),
        ..Limits::NONE
    };
    let outcome = run_with_limits_on(&mut FakeHost::new(), src, &[PROGRAM_PATH], limits);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "4000\n");
}

/// A negative limit is 0: the map takes no entry, and says so with
/// `P0044` on the first new key, not by a wrong count or a crash.
#[test]
fn a_negative_limit_is_zero() {
    let src = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<Int, Int> = map.Map.with_max_entries(-5)
    let n = m.len()
    io.println("{n}")?
    m.put(1, 1)
    io.println("unreachable")
}
"#;
    let outcome = run_on(FakeHost::new(), src, &[]);
    let panic = outcome.panic.as_ref().expect("the first put panics");
    assert_eq!(panic.code, "P0044", "{outcome:#?}");
    assert_eq!(outcome.stdout, "0\n");
}
