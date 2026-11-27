// SPDX-License-Identifier: Apache-2.0
//! The set operations on both backends (#803, spec #798, ADR 0032): the
//! interpreter and native code print the same output, stop at the same
//! step under `--max-steps`, and stop with the same `P0045` on a hash
//! flood, which must not hang.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, Case};

const SET_OPS: &str = r#"fn main(io: Stdout) {
    var a: Set<String> = Set.new()
    var b: Set<String> = Set.new()
    var i = 0
    while i < 60 {
        let _x = a.add("item{i}")
        let twice = i * 2
        let _y = b.add("item{twice}")
        i = i + 1
    }
    let u = a.union(b)
    let n = a.intersection(b)
    let d = b.difference(a)
    let ul = u.len()
    let nl = n.len()
    let dl = d.len()
    var line = ""
    for item in d.to_list() {
        line = "{line} {item}"
    }
    let _ = io.println("{ul} {nl} {dl}{line}")
}
"#;

const FLOOD: &str = r#"struct Key { n: Int }

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
    var a: Set<Key> = Set.new()
    var i = 0
    while i < 1000 {
        let _x = a.add(Key(n: i))
        i = i + 1
    }
    let d = a.difference(a)
    let _ = io.println("unreached")
}
"#;

#[test]
fn the_three_operations_print_the_same_on_both_backends() {
    let outcome = assert_same(&Case::file("set_ops_plain", SET_OPS));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(outcome.stdout.starts_with("90 30 "), "{outcome:#?}");
}

#[test]
fn the_operations_stop_at_the_same_step_on_both_backends() {
    for limit in [50, 500, 5_000, 20_000] {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("set_ops_steps_{limit}"), SET_OPS).with_run_flags(&[&flag]);
        assert_same(&case);
    }
}

#[test]
fn a_hash_flood_stops_with_p0045_on_both_backends() {
    let outcome = assert_same(&Case::file("set_ops_flood", FLOOD));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert!(outcome.stderr.contains("error[P0045]"), "{outcome:#?}");
}
