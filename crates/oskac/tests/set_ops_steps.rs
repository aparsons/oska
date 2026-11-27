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

/// Two operands of 40 colliding items each build (one set holds at most 64
/// under a probe limit), then `OP` runs on them.
fn flood(op: &str) -> String {
    format!(
        "{KEY}fn main(io: Stdout) {{\n    \
             var a: Set<Key> = Set.new()\n    \
             var b: Set<Key> = Set.new()\n    \
             var i = 0\n    \
             while i < 40 {{\n        \
                 let _x = a.add(Key(n: i))\n        \
                 let _y = b.add(Key(n: i + 1000))\n        \
                 i = i + 1\n    \
             }}\n    \
             let _ = io.println(\"built\")\n    \
             let r = a.{op}(b)\n    \
             let n = r.len()\n    \
             let _ = io.println(\"{{n}}\")\n}}\n"
    )
}

/// `a` holds 1, 2, 3 under a limit of 3, and `a.OP(b)` is filled past it.
const LIMITED: &str = r#"fn main(io: Stdout) {
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
fn a_union_flood_stops_with_p0045_after_the_operands_build_on_both_backends() {
    let outcome = assert_same(&Case::file("set_ops_flood_union", &flood("union")));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert_eq!(outcome.stdout, "built\n", "{outcome:#?}");
    assert!(outcome.stderr.contains("error[P0045]"), "{outcome:#?}");
}

// An intersection and a difference are subsets of the left set, so they
// cannot flood: they finish over items that all collide.
#[test]
fn colliding_intersection_and_difference_finish_on_both_backends() {
    for (op, expected) in [("intersection", "0"), ("difference", "40")] {
        let case = Case::file(&format!("set_ops_collide_{op}"), &flood(op));
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(0), "{op}: {outcome:#?}");
        assert_eq!(outcome.stdout, format!("built\n{expected}\n"), "{op}");
    }
}

#[test]
fn every_operation_keeps_the_left_entry_limit_on_both_backends() {
    for op in ["union", "intersection", "difference"] {
        let src = LIMITED.replace("OP", op);
        let outcome = assert_same(&Case::file(&format!("set_ops_limit_{op}"), &src));
        // The union holds 3 already, so its first new item is past the
        // limit; the others have room for one or two.
        assert_eq!(outcome.exit_code, Some(101), "{op}: {outcome:#?}");
        assert!(
            outcome.stderr.contains("error[P0044]"),
            "{op}: {outcome:#?}"
        );
        assert_eq!(outcome.stdout, "", "{op}");
    }
}
