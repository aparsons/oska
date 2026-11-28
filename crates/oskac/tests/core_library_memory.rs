// SPDX-License-Identifier: Apache-2.0
//! `--max-memory` and the list methods that build a new list (spec #660,
//! ticket #666): each of `sorted`, `concat` and `reverse` counts the list
//! it makes against the limit, so a program that keeps making them stops
//! with `P0040` and exit code 101 on the interpreter and natively. Like the
//! other memory tests, this is a resource bound, not a parity promise: it
//! checks only that each backend stops.

mod common;

use common::run_limits::{oskac_in, program};

/// The backends: the interpreter, and natively where the host has a
/// backend (elsewhere `oskac run` interprets, which runs it twice).
const BACKENDS: [&[&str]; 2] = [&["--interpret"], &[]];

/// Keeps every list that `$call` makes, without end.
fn keeps(call: &str) -> String {
    format!(
        r#"fn main() {{
    var xs: List<Int> = []
    for i in 0..200 {{
        xs.push(200 - i)
    }}
    var kept: List<List<Int>> = []
    while true {{
        kept.push({call})
    }}
}}
"#
    )
}

/// Doubles a list without end.
const DOUBLES: &str = r#"fn main() {
    var xs = [1, 2, 3]
    while true {
        xs = xs.concat(xs)
    }
}
"#;

#[test]
fn sorted_concat_and_reverse_count_against_the_memory_limit() {
    let cases = [
        ("memory_sorted", keeps("xs.sorted()")),
        ("memory_reverse", keeps("xs.reverse()")),
        ("memory_concat", keeps("xs.concat(xs)")),
        ("memory_concat_doubles", DOUBLES.to_string()),
    ];
    for (name, src) in cases {
        let dir = program(name, &src);
        for backend in BACKENDS {
            let mut args = vec!["run", "--no-cache", "--max-memory=4194304", "main.osk"];
            args.extend_from_slice(backend);
            let output = oskac_in(&dir, &args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(101),
                "{name} {backend:?}: {stderr}"
            );
            assert!(output.stdout.is_empty(), "{name} {backend:?}");
            assert!(stderr.contains("P0040"), "{name} {backend:?}: {stderr}");
        }
    }
}
