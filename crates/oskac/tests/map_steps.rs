// SPDX-License-Identifier: Apache-2.0
//! `Map` under `--max-steps` (spec #591, ticket #603, ADR 0021 and
//! ADR 0028): a map is written in Oska, so each probe of its table is a
//! loop back-edge and each of its methods a call, and both backends count
//! them alike. A map-heavy program stops at the same step, with the same
//! output and report, on the interpreter and in native code, and a flood
//! of keys that collide under every hash stops with `P0045` the same way.
//!
//! Copied from `run_limits_steps_native.rs`: the same `Case`, the same
//! `assert_same`.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Puts, updates, removes and reads 150 keys: the table grows, probes,
/// compacts and rebuilds on the way.
const MAP_HEAVY: &str = r#"fn main(io: Stdout) {
    var m: Map<String, Int> = Map.new()
    var i = 0
    while i < 150 {
        m.put("key{i}", i)
        i = i + 1
    }
    i = 0
    while i < 150 {
        m.put("key{i}", i * 2)
        i = i + 3
    }
    i = 0
    while i < 150 {
        let _gone = m.remove("key{i}")
        i = i + 5
    }
    var sum = 0
    i = 0
    while i < 150 {
        match m.get("key{i}") {
            Some(v) => { sum = sum + v }
            None => { sum = sum - 1 }
        }
        i = i + 1
    }
    let n = m.len()
    let _ = io.println("{n} {sum}")
}
"#;

/// Every key has the same hash, so every hash of the list is flooded.
const FLOODED: &str = r#"struct Key { n: Int }

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

fn main(io: Stdout) {
    var m: Map<Key, Int> = Map.new()
    var i = 0
    while i < 500 {
        m.put(Key(n: i), i)
        i = i + 1
    }
    let n = m.len()
    let _ = io.println("{n}")
}
"#;

/// The steps `src` takes on the interpreter: the least `--max-steps` under
/// which it ends in its own way, by returning or by another panic.
fn steps_taken(name: &str, src: &str) -> u64 {
    let dir = program(name, src);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let output = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        // Stopped by the step limit is not the end; any other end is.
        !String::from_utf8_lossy(&output.stderr).contains("error[P0038]")
    };
    let (mut low, mut high) = (1, 1);
    while !ends(high) {
        low = high;
        high *= 2;
    }
    while low + 1 < high {
        let middle = (low + high) / 2;
        if ends(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    if ends(low) {
        low
    } else {
        high
    }
}

/// A spread of limits below `steps`, and `steps` itself.
fn limits_up_to(steps: u64) -> Vec<u64> {
    let mut limits = vec![1, 2, 3, steps / 97, steps / 31, steps / 7, steps / 3];
    limits.extend([steps / 2, steps * 2 / 3, steps - 2, steps - 1, steps]);
    limits.retain(|&limit| limit >= 1);
    limits.sort_unstable();
    limits.dedup();
    limits
}

#[test]
fn a_map_heavy_program_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken("map_steps_total", MAP_HEAVY);
    assert!(
        steps > 1_000,
        "a map-heavy program takes many steps: {steps}"
    );
    for limit in limits_up_to(steps) {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("map_steps_{limit}"), MAP_HEAVY).with_run_flags(&[&flag]);
        let outcome = assert_same(&case);
        if limit < steps {
            assert_eq!(outcome.exit_code, Some(101), "{flag}: {outcome:#?}");
            assert!(
                outcome.stderr.contains("error[P0038]"),
                "{flag}: {outcome:#?}"
            );
        } else {
            assert_eq!(outcome.exit_code, Some(0), "{flag}: {outcome:#?}");
        }
    }
}

#[test]
fn a_flooded_map_panics_with_p0045_the_same_on_both_backends() {
    let case = Case::file("map_flooded", FLOODED);
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert!(
        outcome.stderr.contains("error[P0045]: "),
        "{}",
        outcome.stderr
    );
}

#[test]
fn a_flooded_map_stops_at_the_same_step_under_a_limit() {
    let steps = steps_taken("map_flooded_total", FLOODED);
    // Stopped by `P0045` before the end: any limit of at least this many
    // steps ends the same way, and a smaller one with `P0038`.
    for (limit, code) in [
        (steps - 1, "P0038"),
        (steps, "P0045"),
        (steps + 1000, "P0045"),
    ] {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("map_flooded_{limit}"), FLOODED).with_run_flags(&[&flag]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(101), "{flag}");
        assert!(
            outcome.stderr.contains(&format!("error[{code}]")),
            "{flag}: {outcome:#?}"
        );
    }
}
