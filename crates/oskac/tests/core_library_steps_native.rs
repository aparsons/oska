// SPDX-License-Identifier: Apache-2.0
//! The core library under `--max-steps` (spec #660; ADR 0021 and 0034):
//! `<`, `<=`, `>` and `>=` on options and lists, the `List`, `Option` and
//! `Result` methods of `std.core`, and the `String` methods count the same
//! steps, and stop at the same step with the same output, on the
//! interpreter and natively. Each program is a loop that calls every
//! method of its group, so the cut falls inside different calls.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::run_limits::{oskac_in, program};
use differential::{assert_same, Case};

/// The fewest steps `src` takes: the smallest limit under which it runs to
/// its end, found on the interpreter.
fn steps_taken(name: &str, src: &str) -> u64 {
    let dir = program(&format!("{name}_count"), src);
    let ends = |limit: u64| {
        let flag = format!("--max-steps={limit}");
        let out = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        out.status.code() == Some(0)
    };
    let (mut low, mut high) = (1u64, 1u64 << 20);
    assert!(ends(high), "{name} ends under {high} steps");
    while low < high {
        let mid = (low + high) / 2;
        if ends(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

/// Run `src` under limits that cut it at several places and require both
/// backends to agree on each: the output, the exit code and the step it
/// stopped at.
fn assert_steps_agree(name: &str, src: &str) {
    let steps = steps_taken(name, src);
    assert!(steps > 10, "{name} takes real work: {steps} steps");
    for limit in [
        1,
        steps / 7,
        steps / 3,
        steps / 2,
        steps - 1,
        steps,
        steps + 1,
    ] {
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
fn operators_on_options_and_lists_count_the_same_steps() {
    assert_steps_agree(
        "core_steps_operators",
        r#"struct Version { major: Int, minor: Int }
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

fn score(a: Bool, b: Bool, c: Bool, d: Bool) -> Int {
    var n = 0
    if a { n = n + 1 }
    if b { n = n + 2 }
    if c { n = n + 4 }
    if d { n = n + 8 }
    n
}

fn main(io: Stdout) -> Result<(), IoError> {
    let rows = [[], [1], [1, 2], [1, 3], [2], [1, 2, 0]]
    let words = [["b"], ["a", "z"], ["a"], []]
    let maybe = [None, Some(3), Some(1)]
    let versions = [[Version(major: 1, minor: 2)], [Version(major: 1, minor: 3), Version(major: 0, minor: 0)], []]
    var total = 0
    for a in rows {
        for b in rows {
            total = total + score(a < b, a <= b, a > b, a >= b)
        }
    }
    for a in words {
        for b in words {
            total = total + score(a < b, a <= b, a > b, a >= b) * 16
        }
    }
    for a in maybe {
        for b in maybe {
            total = total + score(a < b, a <= b, a > b, a >= b) * 256
        }
    }
    for a in versions {
        for b in versions {
            total = total + score(a < b, a <= b, a > b, a >= b) * 4096
        }
    }
    io.println("{total}")?
    Ok(())
}
"#,
    );
}

#[test]
fn list_methods_count_the_same_steps() {
    assert_steps_agree(
        "core_steps_list",
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0
    for round in 0..6 {
        let xs = [5 + round, 3, 9, 1, 7, 3]
        let evens = xs.filter(|x| x % 2 == 0)
        let odds = xs.filter(|x| x % 2 == 1)
        total = total + evens.len() + odds.len()
        total = total + xs.fold(0, |acc, x| acc * 3 + x) % 1000
        if xs.any(|x| x > 8) { total = total + 1 }
        if xs.all(|x| x > 0) { total = total + 2 }
        total = total + xs.find(|x| x > 4).unwrap_or(0)
        if xs.contains(7) { total = total + 4 }
        total = total + xs.first().unwrap_or(0) + xs.last().unwrap_or(0)
        if xs.is_empty() { total = total + 100 }
        let back = xs.reverse()
        let both = back.concat(xs)
        total = total + both.len() + both.get(0).unwrap_or(0)
        let sorted = both.sorted()
        total = total + sorted.get(0).unwrap_or(0) * 10 + sorted.last().unwrap_or(0)
        let by = xs.sorted_by(|a, b| b.cmp(a))
        total = total + by.get(0).unwrap_or(0)
        var ys = both
        ys.sort()
        total = total + ys.get(1).unwrap_or(0)
    }
    io.println("{total}")?
    Ok(())
}
"#,
    );
}

#[test]
fn option_and_result_methods_count_the_same_steps() {
    assert_steps_agree(
        "core_steps_option_result",
        r#"fn parse(s: String) -> Result<Int, String> {
    match s.parse_int() {
        Some(n) => Ok(n)
        None => Err("bad {s}")
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0
    for round in 0..8 {
        let o = if round % 3 == 0 { None } else { Some(round) }
        if o.is_none() { total = total + 1 }
        total = total + o.map(|x| x * 2).unwrap_or(0)
        total = total + o.and_then(|x| if x > 3 { Some(x) } else { None }).unwrap_or(-1)
        total = total + o.unwrap_or_else(|| 40)
        total = total + o.filter(|x| x % 2 == 0).unwrap_or(7)
        let r = parse(if round % 2 == 0 { "12" } else { "x" })
        if r.is_err() { total = total + 3 }
        total = total + r.map(|x| x + round).unwrap_or(0)
        total = total + r.map_err(|e| e.len()).unwrap_or_else(|n| n)
        total = total + r.and_then(|x| if x > 5 { Ok(x) } else { Err("small") }).unwrap_or_else(|e| e.len())
        total = total + r.ok().unwrap_or(0) + r.err().map(|e| e.len()).unwrap_or(0)
    }
    io.println("{total}")?
    Ok(())
}
"#,
    );
}

#[test]
fn string_methods_count_the_same_steps() {
    assert_steps_agree(
        "core_steps_string",
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let texts = ["  key=value  ", "plain", "", "àb=c", "prefix-body-suffix"]
    var total = 0
    for round in 0..4 {
        for t in texts {
            total = total + t.to_uppercase().len()
            total = total + t.trim_start().len() * 2
            total = total + t.trim_end().len() * 3
            total = total + t.find("=").unwrap_or(-1)
            total = total + t.split_once("=").unwrap_or([]).len()
            total = total + t.strip_prefix("prefix-").unwrap_or("").len()
            total = total + t.strip_suffix("-suffix").unwrap_or("").len()
            if t.is_empty() { total = total + 50 + round }
        }
    }
    io.println("{total}")?
    Ok(())
}
"#,
    );
}

#[test]
fn a_comparison_that_panics_midway_panics_the_same_on_both_backends() {
    // The sort has compared some pairs when `cmp` panics: both backends
    // print the same text before it, report the same panic and exit 101.
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var xs = [4, 2, 1, 7, 3, 9, 5]
    let none: List<Int> = []
    let n = xs.len()
    io.println("before {n}")?
    xs = xs.sorted_by(|a, b| if a == 9 { none.at(0).cmp(b) } else { a.cmp(b) })
    let m = xs.len()
    io.println("after {m}")?
    Ok(())
}
"#;
    let outcome = assert_same(&Case::file("core_sort_panics_midway", src));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert_eq!(outcome.stdout, "before 7\n");
    assert!(outcome.stderr.contains("P0002"), "{outcome:#?}");
}
