//! A failed flow leaves no trace (#503): when a value cannot flow where a
//! type is expected, the checker reports one mismatch and carries on as
//! if the flow had never been tried. No inference variable it solved on
//! the way stays solved, and no capability or trust flag it raised,
//! pinned or linked stays so. The same holds when the flow fails only on
//! one flag (`E0270`, `E0261`, ...): the attempt that found out which flag
//! leaves nothing behind either.

mod common;

use common::{assert_errors, assert_ok};

/// A pure function, `run` and `all` that take plain `fn`s, `all_cap` that
/// takes `cap fn`s, and a generic pair.
const ITEMS: &str = "fn pure() -> Int {\n    1\n}\n\
                     fn all(fs: List<fn() -> Int>) -> Int {\n    fs.len()\n}\n\
                     fn all_cap(fs: List<cap fn() -> Int>) -> Int {\n    fs.len()\n}\n\
                     struct Pair<A, B> { a: A, b: B }\n";

/// A closure that captures `out`, so it is a `cap fn`.
const CAPTURING: &str = "|| {\n        let _ = out\n        2\n    }";

/// `ITEMS`, then `fn f(out: Stdout) -> Int` with `body`.
fn program(body: &str) -> String {
    format!("{ITEMS}fn f(out: Stdout) -> Int {{\n{body}\n}}")
}

#[test]
fn a_flow_that_fails_on_its_types_leaves_no_variable_solved() {
    // The failed flow solves the list's element type to `Int` before it
    // meets `Int` against `String`.
    let body = "    let p = Pair(a: [], b: 1)\n    \
                let bad: Pair<List<Int>, String> = p\n    \
                let good: Pair<List<Bool>, Int> = p\n    \
                0";
    assert_errors(&program(body), &[("E0220", "p")]);
}

#[test]
fn the_variable_a_failed_flow_left_open_is_solved_by_the_next_flow() {
    let body = "    let p = Pair(a: [], b: 1)\n    \
                let bad: Pair<List<Int>, String> = p\n    \
                let good: Pair<List<Bool>, Int> = p\n    \
                let worse: Pair<List<Int>, Int> = p\n    \
                0";
    let src = program(body);
    let report = common::check(&src);
    let lines: Vec<(&str, usize)> = report
        .diagnostics
        .iter()
        .map(|d| (d.code, src[..d.span.start].lines().count()))
        .collect();
    let first = ITEMS.lines().count() + 3;
    assert_eq!(
        lines,
        [("E0220", first), ("E0220", first + 2)],
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn a_flow_that_fails_on_its_types_leaves_no_capability_pinned() {
    // The failed flow pins the list's capability flag as a plain `fn`
    // before it meets `Int` against `String`.
    let body = format!(
        "    var fs = [pure]\n    \
         let p = Pair(a: fs, b: 1)\n    \
         let bad: Pair<List<fn() -> Int>, String> = p\n    \
         fs = [{CAPTURING}]\n    \
         all_cap(fs)"
    );
    assert_errors(&program(&body), &[("E0220", "p")]);
}

#[test]
fn a_flow_that_fails_on_its_types_leaves_no_capability_raised() {
    // The failed assignment raises the list's capability flag to `cap fn`
    // before it meets `String` against `Int`.
    let body = format!(
        "    var p = Pair(a: [pure], b: 1)\n    \
         let q = Pair(a: [{CAPTURING}], b: \"x\")\n    \
         p = q\n    \
         all(p.a)"
    );
    assert_errors(&program(&body), &[("E0220", "q")]);
}

#[test]
fn a_flow_that_fails_only_on_the_cap_rule_leaves_no_variable_solved() {
    // Both the strict attempt and the one without the `cap` rule solve the
    // list's element type to `Int`; the second succeeds, which is how the
    // checker knows the `cap` rule was the only obstacle.
    let body = format!(
        "    let p = Pair(a: [], b: {CAPTURING})\n    \
         let bad: Pair<List<Int>, fn() -> Int> = p\n    \
         let good: Pair<List<Bool>, cap fn() -> Int> = p\n    \
         0"
    );
    assert_errors(&program(&body), &[("E0270", "p")]);
}

#[test]
fn a_flow_that_succeeds_still_solves_its_variables() {
    let body = "    let p = Pair(a: [], b: 1)\n    \
                let good: Pair<List<Int>, Int> = p\n    \
                let bad: Pair<List<Bool>, Int> = p\n    \
                0";
    assert_errors(&program(body), &[("E0220", "p")]);
    assert_ok(&program(
        "    let p = Pair(a: [], b: 1)\n    \
         let good: Pair<List<Int>, Int> = p\n    \
         let again: Pair<List<Int>, Int> = p\n    \
         0",
    ));
}
