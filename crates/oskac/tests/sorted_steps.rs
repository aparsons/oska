// SPDX-License-Identifier: Apache-2.0
//! `SortedMap` and `SortedSet` under `--max-steps` (spec #888, ticket #806,
//! ADR 0021 and ADR 0033): they are written in Oska, so each call and loop back-edge
//! counts, and both backends stop at the same step with the same output.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Puts, replaces and removes on both collections, in a scrambled order.
const SORTED_HEAVY: &str = r#"fn main(io: Stdout) {
    var s: SortedSet<Int> = SortedSet.new()
    var m: SortedMap<Int, Int> = SortedMap.new()
    var i = 0
    while i < 60 {
        let k = (i * 37) % 41
        let _ = s.add(k)
        let _ = m.put(k, i)
        i = i + 1
    }
    i = 0
    while i < 60 {
        let k = (i * 11) % 41
        if i % 3 == 0 {
            let _ = s.remove(k)
            let _ = m.remove(k)
        }
        let _ = s.contains(k)
        let _ = m.get(k)
        i = i + 1
    }
    let n = s.len()
    let f = s.first().unwrap_or(-1)
    let l = m.last().map(|e| e.key).unwrap_or(-1)
    let _ = io.println("{n} {f} {l}")
}
"#;

/// The same under a `cmp` that is not a total order.
const BAD_ORDER: &str = r#"struct K { n: Int }
impl Eq for K {
    fn eq(self, other: K) -> Bool { self.n == other.n }
}
impl Ord for K {
    fn cmp(self, other: K) -> Ordering {
        if (self.n * 7 + other.n) % 3 == 0 { Ordering.Less } else { Ordering.Greater }
    }
}

fn main(io: Stdout) {
    var s: SortedSet<K> = SortedSet.new()
    var i = 0
    while i < 80 {
        let _ = s.add(K(n: (i * 13) % 29))
        let _ = s.contains(K(n: i))
        if i % 4 == 0 {
            let _ = s.remove(K(n: i))
        }
        i = i + 1
    }
    let n = s.len()
    let _ = io.println("{n}")
}
"#;

/// The steps `src` takes on the interpreter: the least `--max-steps` under
/// which it ends in its own way.
fn steps_taken(name: &str, src: &str) -> u64 {
    let dir = program(name, src);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let output = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
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

fn assert_stops_the_same(name: &str, src: &str, floor: u64) {
    let steps = steps_taken(&format!("{name}_total"), src);
    assert!(steps > floor, "{name} takes steps: {steps}");
    let mut limits = vec![1, 2, 3, steps / 31, steps / 7, steps / 3, steps / 2];
    limits.extend([steps * 2 / 3, steps - 2, steps - 1, steps]);
    limits.retain(|&limit| limit >= 1);
    limits.sort_unstable();
    limits.dedup();
    for limit in limits {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("{name}_{limit}"), src).with_run_flags(&[&flag]);
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
fn a_sorted_heavy_program_stops_at_the_same_step_on_both_backends() {
    assert_stops_the_same("sorted_steps", SORTED_HEAVY, 1000);
}

#[test]
fn an_inconsistent_cmp_stops_at_the_same_step_on_both_backends() {
    assert_stops_the_same("sorted_bad_order_steps", BAD_ORDER, 300);
}
