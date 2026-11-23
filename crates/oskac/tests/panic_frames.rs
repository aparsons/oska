// SPDX-License-Identifier: Apache-2.0
//! Panic call stacks (ticket #370, spec #367): a panic's diagnostic
//! carries `frames`, the calls that led to it, innermost first, on the
//! interpreter and the native backend alike. The differential harness
//! (seam 1) holds the two equal, in the human output and in the JSON;
//! `oskac test` carries them in a failing test's diagnostic.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::process::Command;

use common::portable::Scratch;
use common::Json;
use differential::{assert_same, assert_same_at_depth_limit, Case};

/// A division by zero two calls below `main`.
const TWO_DEEP: &str = "fn half(n: Int) -> Int {
    100 / n
}

fn middle(n: Int) -> Int {
    half(n) + 1
}

fn main(stdout: Stdout) {
    let x = middle(0)
    let _ = stdout.println(\"{x}\")
}
";

/// A division by zero three calls below `main`, one of them through a
/// closure.
const THREE_DEEP: &str = "fn half(n: Int) -> Int {
    100 / n
}

fn middle(n: Int) -> Int {
    let f = |m: Int| half(m)
    f(n) + 1
}

fn outer(n: Int) -> Int {
    middle(n) * 2
}

fn main(stdout: Stdout) {
    let x = outer(0)
    let _ = stdout.println(\"{x}\")
}
";

/// Recursion as deep as its argument: `down(n)` nests `n + 1` calls
/// under `main`.
const RUNAWAY: &str = "fn down(n: Int) -> Int {
    if n == 0 { 0 } else { down(n - 1) + 1 }
}

fn main(stdout: Stdout, args: Args) {
    let n = args.get(1).unwrap_or(\"0\").parse_int().unwrap_or(0)
    let x = down(n)
    let _ = stdout.println(\"{x}\")
}
";

/// [`RUNAWAY`] nesting as many calls as the limit allows.
fn deepest(name: &str) -> Case {
    Case::file(name, RUNAWAY).with_args(&["49999"])
}

/// The `frames` of the one panic diagnostic on `stderr`, a JSON line, as
/// `function@line:column` each, and its `frames_omitted`.
fn frames(stderr: &str) -> (Vec<String>, u64) {
    let line = stderr
        .lines()
        .find(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON diagnostic on stderr: {stderr}"));
    let diagnostic = Json::parse(line);
    frames_of(&diagnostic)
}

fn frames_of(diagnostic: &Json) -> (Vec<String>, u64) {
    let Some(Json::Arr(frames)) = diagnostic.get("frames") else {
        panic!("no `frames` in {diagnostic:?}");
    };
    let shown = frames
        .iter()
        .map(|frame| {
            let field = |key: &str| frame.get(key).unwrap_or_else(|| panic!("no `{key}`"));
            let (Json::Str(function), Json::Num(line), Json::Num(column)) =
                (field("function"), field("line"), field("column"))
            else {
                panic!("a frame of the wrong shape: {frame:?}");
            };
            assert!(frame.get("path").is_some(), "a frame names its file");
            assert!(frame.get("span").is_some(), "a frame has its span");
            format!("{function}@{line}:{column}")
        })
        .collect();
    let Some(Json::Num(omitted)) = diagnostic.get("frames_omitted") else {
        panic!("no `frames_omitted` in {diagnostic:?}");
    };
    (shown, *omitted)
}

#[test]
fn a_panic_two_calls_deep_lists_its_frames_under_the_panic_line() {
    let outcome = assert_same(&Case::file("frames_two_deep", TWO_DEEP));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert_eq!(
        outcome.stderr,
        "main.osk:2:5: error[P0004]: division by zero: `100 / 0` (bytes 29..36)\n    \
         at half (main.osk:2:5)\n    \
         at middle (main.osk:6:5)\n    \
         at main (main.osk:10:13)\n",
        "{outcome:#?}"
    );
}

#[test]
fn a_panic_two_calls_deep_prints_the_same_frames_as_json() {
    let case = Case::file("frames_two_deep_json", TWO_DEEP).with_run_flags(&["--format=json"]);
    let outcome = assert_same(&case);
    assert_eq!(
        frames(&outcome.stderr),
        (
            vec![
                "half@2:5".to_string(),
                "middle@6:5".to_string(),
                "main@10:13".to_string()
            ],
            0
        ),
        "{outcome:#?}"
    );
}

#[test]
fn a_panic_three_calls_deep_names_the_closure_on_its_way() {
    let case = Case::file("frames_three_deep_json", THREE_DEEP).with_run_flags(&["--format=json"]);
    let outcome = assert_same(&case);
    assert_eq!(
        frames(&outcome.stderr),
        (
            vec![
                "half@2:5".to_string(),
                "<closure in middle>@6:22".to_string(),
                "middle@7:5".to_string(),
                "outer@11:5".to_string(),
                "main@15:13".to_string()
            ],
            0
        ),
        "{outcome:#?}"
    );
    let human = assert_same(&Case::file("frames_three_deep", THREE_DEEP));
    assert!(
        human
            .stderr
            .contains("    at <closure in middle> (main.osk:6:22)\n    at middle (main.osk:7:5)\n"),
        "{human:#?}"
    );
}

#[test]
fn a_deep_recursion_panic_reports_a_bounded_list_and_the_count_left_out() {
    let case = Case::file("frames_runaway_json", RUNAWAY)
        .with_args(&["1000000"])
        .with_run_flags(&["--format=json"]);
    let (_, native) = assert_same_at_depth_limit(&case, &deepest("frames_deepest_json"));
    let (shown, omitted) = frames(&native.stderr);
    assert_eq!(shown.len(), 20, "{native:#?}");
    assert!(shown.iter().all(|f| f == "down@2:28"), "{shown:?}");
    // 50,000 calls of `down` nest, and `main` called the first: 50,001.
    assert_eq!(omitted, 50_001 - 20, "{native:#?}");
    let (_, human) = assert_same_at_depth_limit(
        &Case::file("frames_runaway", RUNAWAY).with_args(&["1000000"]),
        &deepest("frames_deepest"),
    );
    assert!(
        human
            .stderr
            .ends_with("    at down (main.osk:2:28)\n    ... 49981 more frames\n"),
        "{human:#?}"
    );
}

/// A test whose assertion fails inside a helper.
const HELPER_TEST: &str = "fn check_positive(t: TestContext, n: Int) {
    t.assert(n > 0)
}

test \"negative numbers are positive\" {
    check_positive(t, 0 - 1)
}
";

/// The frames of the failure of the one test in `src`, run by `oskac
/// test --format=json`, which must fail it, the same on both backends.
fn failed_test_frames(name: &str, src: &str) -> (Vec<String>, u64) {
    let scratch = Scratch::new("panic_frames", name);
    std::fs::write(scratch.path().join("main.osk"), src).expect("write the test file");
    let run = |extra: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .current_dir(scratch.path())
            .args(["test", "--format=json", "main.osk"])
            .args(extra)
            .output()
            .expect("run oskac test");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8");
        // A failed test makes `oskac test` exit with 1.
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {stdout}");
        stdout
    };
    let interpreted = run(&["--interpret"]);
    let native = run(&[]);
    // A test's `duration_ms` differs from run to run, so it is masked.
    assert_eq!(
        common::golden::mask_durations(&interpreted),
        common::golden::mask_durations(&native),
        "the backends differ"
    );
    let report = Json::parse(interpreted.trim());
    let Some(Json::Arr(tests)) = report.get("tests") else {
        panic!("no tests in {interpreted}");
    };
    frames_of(tests[0].get("failure").expect("the test failed"))
}

#[test]
fn a_failed_assertion_inside_a_helper_reports_frames_back_to_the_test() {
    assert_eq!(
        failed_test_frames("helper_test", HELPER_TEST),
        (
            vec![
                "check_positive@2:5".to_string(),
                "test \"negative numbers are positive\"@6:5".to_string()
            ],
            0
        )
    );
}

#[test]
fn a_test_named_with_the_bytes_a_native_report_separates_frames_by_keeps_its_frames() {
    // `\x1f` and `\x1e` end the pieces of a native report's frames, and
    // `\x1d` escapes them in a name.
    let src = HELPER_TEST.replace("negative numbers are positive", "a\u{1f}b\u{1e}c\u{1d}d");
    assert_eq!(
        failed_test_frames("separator_name", &src),
        (
            vec![
                "check_positive@2:5".to_string(),
                "test \"a\u{1f}b\u{1e}c\u{1d}d\"@6:5".to_string()
            ],
            0
        )
    );
}

#[test]
fn a_native_report_reads_back_its_frames_and_the_count_left_out() {
    let report = "error[P0021]: a \u{1f}b\u{1e}1..2 (bytes 5..9)\u{1f}down\u{1e}5..9\
                  \u{1f}<closure in main>\u{1e}20..30\u{1f}+7\n";
    let read = oska_native::test_failure(report.as_bytes()).expect("a report");
    // A message holding the separators stays whole.
    assert_eq!(read.message, "a \u{1f}b\u{1e}1..2");
    let frames = read.frames.expect("its frames");
    let shown: Vec<(&str, usize, usize)> = frames
        .frames
        .iter()
        .map(|f| (f.function.as_str(), f.span.start, f.span.end))
        .collect();
    assert_eq!(shown, [("down", 5, 9), ("<closure in main>", 20, 30)]);
    assert_eq!(frames.omitted, 7);
}
