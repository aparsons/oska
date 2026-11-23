// SPDX-License-Identifier: Apache-2.0
//! String literals made once (#500, spec #521): the interpreter makes the
//! text of each string literal once and shares it each time the literal is
//! evaluated, instead of copying it into a new text every time. A run under
//! `--max-memory` still charges each evaluation of a literal for the whole
//! text, as before, and still gets those bytes back once the text is let go
//! of.

mod common;

use common::{assert_runs, run_under_limit};
use oska_run::codes;

/// A literal of 40 bytes, so that each copy of it is easy to see in the
/// bytes a run holds.
const FORTY: &str = "a literal of exactly forty bytes, padded";

/// Whether `src` runs to the end under `--max-memory=<limit>`, or stops
/// with `P0040`.
fn finishes_under(src: &str, limit: u64) -> bool {
    let outcome = run_under_limit(src, limit);
    match &outcome.panic {
        None => {
            assert_eq!(outcome.exit_code, 0, "{outcome:?}");
            true
        }
        Some(panic) => {
            assert_eq!(panic.code, codes::MEMORY_LIMIT, "{outcome:?}");
            false
        }
    }
}

/// The smallest `--max-memory` that `src` runs to the end under.
fn least_limit(src: &str) -> u64 {
    let (mut fails, mut finishes) = (0, 1 << 20);
    assert!(finishes_under(src, finishes), "{src} needs more than 1 MiB");
    while finishes - fails > 1 {
        let mid = (fails + finishes) / 2;
        if finishes_under(src, mid) {
            finishes = mid;
        } else {
            fails = mid;
        }
    }
    finishes
}

/// Holds `depth` texts of the same literal at once: one in each of
/// `depth` nested calls.
fn holding(depth: u32) -> String {
    format!(
        r#"fn hold(n: Int) -> Int {{
    let held = "{FORTY}"
    if n == 0 {{
        return held.len()
    }}
    hold(n - 1) + held.len()
}}

fn main() {{
    let _ = hold({depth})
}}
"#
    )
}

#[test]
fn each_evaluation_of_a_literal_is_charged_its_whole_text() {
    // A text is charged 64 bytes on a 64-bit host, its record and its
    // reference counts, and then its own bytes: 104 for this literal.
    // 100 more texts held at once need 100 times that.
    assert_eq!(FORTY.len(), 40);
    let fewer = least_limit(&holding(100));
    let more = least_limit(&holding(200));
    assert_eq!(more - fewer, 100 * 104);
}

#[test]
fn a_literal_let_go_of_gives_its_bytes_back() {
    // 200,000 texts of 104 bytes each are 20 MB, far past a 64 KiB limit:
    // the run ends only because each text's bytes come back once it is
    // let go of.
    let src = format!(
        r#"fn main(io: Stdout) {{
    var i = 0
    var bytes = 0
    while i < 200000 {{
        let passing = "{FORTY}"
        bytes = bytes + passing.len()
        i = i + 1
    }}
    let _ = io.println("{{bytes}}")
}}
"#
    );
    let outcome = run_under_limit(&src, 64 * 1024);
    assert_eq!(outcome.panic, None, "{outcome:?}");
    assert_eq!(outcome.stdout, "8000000\n");
}

#[test]
fn each_literal_keeps_its_own_text_however_often_it_is_evaluated() {
    // Literals of the same length, the empty literal, and one literal
    // evaluated in a loop and from many calls each give their own text.
    assert_runs(
        r#"fn pick(n: Int) -> String {
    if n % 3 == 0 {
        return "abc"
    }
    if n % 3 == 1 {
        return "xyz"
    }
    ""
}

fn main(io: Stdout) {
    var out = ""
    var i = 0
    while i < 7 {
        let picked = pick(i)
        out = "{out}{picked}|"
        i = i + 1
    }
    let _ = io.println(out)
    if "abc" == pick(3) && "" == pick(5) && "xyz" != pick(3) {
        let _ = io.println("same")
    }
}
"#,
        &[],
        "abc|xyz||abc|xyz||abc|\nsame\n",
    );
}
