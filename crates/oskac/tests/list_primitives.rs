// SPDX-License-Identifier: Apache-2.0
//! `List.set`, `pop` and `truncate` on both backends (spec #798, ticket
//! #800, ADR 0031): a program that uses them stops at the same step, with
//! the same output and report, on the interpreter and in native code
//! (ADR 0021), and a bad `set` index gives the same `P0002` report, kept
//! in a golden file. The behaviour tests, which are differential too, are
//! `crates/oska-run/tests/list_primitives.rs`.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::golden::assert_golden_file;
use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// Uses all three methods, in loops, on shared and unshared lists.
const HEAVY: &str = r#"fn main(io: Stdout) {
    var xs: List<Int> = []
    var i = 0
    while i < 60 {
        xs.push(i)
        i = i + 1
    }
    var keep: List<List<Int>> = []
    i = 0
    while i < 60 {
        keep.push(xs)
        xs.set(i, i * 2)
        i = i + 1
    }
    var sum = 0
    while true {
        match xs.pop() {
            Some(v) => { sum = sum + v }
            None => { break }
        }
        if xs.len() == 30 {
            xs.truncate(10)
        }
    }
    let n = keep.len()
    let first = keep.at(0).at(0)
    let _ = io.println("{n} {sum} {first}")
}
"#;

/// A bad `set` index.
const BAD_SET: &str = r#"fn main(io: Stdout) {
    var xs = ["a", "b", "c"]
    xs.set(2, "z")
    let _ = io.println(xs.at(2))
    xs.set(3, "never")
    let _ = io.println("unreached")
}
"#;

/// The steps `src` takes on the interpreter: the least `--max-steps`
/// under which it does not stop with `P0038`.
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

#[test]
fn a_program_using_set_pop_and_truncate_stops_at_the_same_step_on_both_backends() {
    let steps = steps_taken("list_primitives_total", HEAVY);
    assert!(steps > 100, "the program takes many steps: {steps}");
    let mut limits = vec![1, 2, 3, steps / 7, steps / 2, steps - 1, steps];
    limits.retain(|&limit| limit >= 1);
    limits.sort_unstable();
    limits.dedup();
    for limit in limits {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("list_primitives_{limit}"), HEAVY).with_run_flags(&[&flag]);
        let outcome = assert_same(&case);
        if limit < steps {
            assert_eq!(outcome.exit_code, Some(101), "{flag}: {outcome:#?}");
            assert!(
                outcome.stderr.contains("error[P0038]"),
                "{flag}: {outcome:#?}"
            );
        } else {
            assert_eq!(outcome.exit_code, Some(0), "{flag}: {outcome:#?}");
            assert_eq!(outcome.stdout, "60 2760 0\n", "{outcome:#?}");
        }
    }
}

#[test]
fn a_bad_set_index_gives_the_same_p0002_report_on_both_backends() {
    let outcome = assert_same(&Case::file("list_set_p0002", BAD_SET));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    let transcript = format!(
        "--- stdout\n{}--- stderr\n{}--- exit {:?}\n",
        outcome.stdout, outcome.stderr, outcome.exit_code
    );
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/list_primitives/set_p0002.txt");
    assert_golden_file(&golden, &transcript);
}
