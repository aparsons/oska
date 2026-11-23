// SPDX-License-Identifier: Apache-2.0
//! Snapshot assertions (spec #367, ticket #373): `t.assert_snapshot(value)`
//! compares the value, rendered as `dbg` renders it, with a file under
//! `snapshots/<test-file>/<test-name>.snap`, and `oskac test --update`
//! writes new and changed snapshots. Each test runs `oskac test` on a
//! scratch package or file, on the interpreter (`--interpret`) and
//! natively, and requires the two backends to report the same.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::golden::mask_durations;
use common::portable::{output_of, Scratch};
use common::Json;

/// The two backends: the interpreter, then native code.
const BACKENDS: [&[&str]; 2] = [&["--interpret"], &[]];

fn oskac_in(dir: &Path, args: &[&str]) -> Output {
    output_of(
        Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(dir)
            .env("OSKA_CACHE", "off"),
    )
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A package in `dir` whose `src/lib.osk` is `lib`.
fn package(dir: &Path, lib: &str) {
    std::fs::write(
        dir.join("oska.toml"),
        "[package]\nname = \"shots\"\nversion = \"0.1.0\"\n",
    )
    .expect("write the manifest");
    std::fs::create_dir_all(dir.join("src")).expect("make src");
    std::fs::write(under(dir, "src/lib.osk"), lib).expect("write the library");
}

/// `oskac test` with `args` in `dir` on each backend: the exit code and
/// the report, which must be the same on both, durations aside. Each
/// backend starts from the snapshots `dir` holds before the call.
fn test_on_both(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let before = snapshot_files(dir);
    run_on_both(dir, args, || restore(dir, &before))
}

/// [`test_on_both`], calling `reset` before each backend runs.
fn run_on_both(dir: &Path, args: &[&str], reset: impl Fn()) -> (Option<i32>, String) {
    let mut results = Vec::new();
    for backend in BACKENDS {
        reset();
        let mut all = vec!["test"];
        all.extend_from_slice(args);
        all.extend_from_slice(backend);
        let output = oskac_in(dir, &all);
        results.push((output.status.code(), mask_durations(&stdout(&output))));
    }
    assert_eq!(results[0], results[1], "the backends differ");
    let (code, report) = results.remove(0);
    // Masked, a duration is no JSON: 0 stands in for it.
    (
        code,
        report.replace("\"duration_ms\":<ms>", "\"duration_ms\":0"),
    )
}

/// Every file under `dir/snapshots`, by its `/`-separated path from
/// `dir`, with its text.
fn snapshot_files(dir: &Path) -> Vec<(String, String)> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let relative = path.strip_prefix(root).expect("under the root");
                let parts: Vec<String> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let text = std::fs::read_to_string(&path).expect("a text file");
                found.push((parts.join("/"), text));
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, &dir.join("snapshots"), &mut found);
    found.sort();
    found
}

/// Put `dir/snapshots` back to `files`.
fn restore(dir: &Path, files: &[(String, String)]) {
    let _ = std::fs::remove_dir_all(dir.join("snapshots"));
    for (path, text) in files {
        let file = under(dir, path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("make the directory");
        std::fs::write(file, text).expect("write the snapshot");
    }
}

/// `dir` joined with each part of the `/`-separated `path`, one at a time.
fn under(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_path_buf(), |at, part| at.join(part))
}

fn files(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
}

/// The `failure` of the only test in a JSON report.
fn only_failure(report: &str) -> Json {
    let report = Json::parse(report.trim());
    let Some(Json::Arr(tests)) = report.get("tests") else {
        panic!("no tests in {report:?}");
    };
    assert_eq!(tests.len(), 1, "{tests:?}");
    tests[0].get("failure").expect("a failure").clone()
}

fn text(json: Option<&Json>) -> String {
    match json {
        Some(Json::Str(text)) => text.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

const SHOWS_A_LIST: &str = "pub fn evens(n: Int) -> List<Int> {
    var out: List<Int> = []
    for i in 0..n {
        out.push(i * 2)
    }
    out
}

test \"shows evens\" {
    t.assert_snapshot(evens(3))
}
";

#[test]
fn the_full_cycle_fails_writes_passes_fails_on_a_change_and_rewrites() {
    let scratch = Scratch::new("snapshots", "cycle");
    package(&scratch, SHOWS_A_LIST);

    // No snapshot yet: the test fails, says so, and names `--update`.
    let (code, report) = test_on_both(&scratch, &["--format=json"]);
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    let message = text(failure.get("message"));
    assert!(
        message.contains("snapshots/lib/shows evens.snap")
            && message.contains("oskac test --update")
            && message.contains("+ [0, 2, 4]"),
        "{message}"
    );
    assert_eq!(snapshot_files(&scratch), []);

    // `--update` writes it, and says so.
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert!(
        report.contains(
            "\"snapshots\":[{\"path\":\"snapshots/lib/shows evens.snap\",\"status\":\"new\"}]"
        ),
        "{report}"
    );
    assert_eq!(
        snapshot_files(&scratch),
        files(&[("snapshots/lib/shows evens.snap", "[0, 2, 4]\n")])
    );

    // Now it passes.
    let (code, report) = test_on_both(&scratch, &["--format=json"]);
    assert_eq!(code, Some(0), "{report}");

    // A changed value fails with the difference.
    package(&scratch, &SHOWS_A_LIST.replace("evens(3)", "evens(2)"));
    let (code, report) = test_on_both(&scratch, &["--format=json"]);
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    let message = text(failure.get("message"));
    assert!(
        message.contains("- [0, 2, 4]\n+ [0, 2]") && message.contains("oskac test --update"),
        "{message}"
    );
    assert_eq!(text(failure.get("path")), "src/lib.osk");

    // `--update` rewrites it, and says it changed.
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert!(report.contains("\"status\":\"changed\""), "{report}");
    assert_eq!(
        snapshot_files(&scratch),
        files(&[("snapshots/lib/shows evens.snap", "[0, 2]\n")])
    );
}

#[test]
fn the_human_report_shows_the_difference_and_lists_what_update_wrote() {
    let scratch = Scratch::new("snapshots", "human");
    package(&scratch, SHOWS_A_LIST);
    restore(
        &scratch,
        &files(&[("snapshots/lib/shows evens.snap", "[0, 2]\n")]),
    );
    let (code, report) = test_on_both(&scratch, &[]);
    assert_eq!(code, Some(1), "{report}");
    assert!(
        report.contains("error[P0014]: ") && report.contains("\n    - [0, 2]\n    + [0, 2, 4]\n"),
        "{report}"
    );
    let (code, report) = test_on_both(&scratch, &["--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert!(
        report.contains("changed snapshots/lib/shows evens.snap\n")
            && report.contains("1 snapshot written"),
        "{report}"
    );
}

#[test]
fn update_writes_only_what_is_new_or_changed_and_deletes_nothing() {
    let scratch = Scratch::new("snapshots", "keeps");
    package(&scratch, SHOWS_A_LIST);
    let kept = files(&[
        ("snapshots/lib/a test that is gone.snap", "old\n"),
        ("snapshots/lib/shows evens.snap", "[0, 2, 4]\n"),
    ]);
    restore(&scratch, &kept);
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert!(report.contains("\"snapshots\":[]"), "{report}");
    assert_eq!(snapshot_files(&scratch), kept);
}

#[test]
fn a_test_with_more_than_one_snapshot_numbers_them_in_the_order_they_run() {
    let scratch = Scratch::new("snapshots", "numbered");
    package(
        &scratch,
        "test \"two values\" {
    t.assert_snapshot(\"first\")
    t.assert_snapshot(2)
}

test \"one value\" {
    t.assert_snapshot(true)
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(
        snapshot_files(&scratch),
        files(&[
            ("snapshots/lib/one value.snap", "true\n"),
            ("snapshots/lib/two values-1.snap", "\"first\"\n"),
            ("snapshots/lib/two values-2.snap", "2\n"),
        ])
    );
    let (code, report) = test_on_both(&scratch, &[]);
    assert_eq!(code, Some(0), "{report}");
}

#[test]
fn a_single_file_keeps_its_snapshots_beside_it() {
    let scratch = Scratch::new("snapshots", "file");
    std::fs::create_dir_all(scratch.join("work")).expect("make work");
    std::fs::write(under(&scratch, "work/shapes.osk"), SHOWS_A_LIST).expect("write the file");
    for backend in BACKENDS {
        let _ = std::fs::remove_dir_all(under(&scratch, "work/snapshots"));
        let mut args = vec!["test", "work/shapes.osk", "--update"];
        args.extend_from_slice(backend);
        let output = oskac_in(&scratch, &args);
        assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
        let written =
            std::fs::read_to_string(under(&scratch, "work/snapshots/shapes/shows evens.snap"))
                .expect("the snapshot");
        assert_eq!(written, "[0, 2, 4]\n");
    }
}

#[test]
fn a_stored_snapshot_with_windows_line_endings_still_matches() {
    let scratch = Scratch::new("snapshots", "crlf");
    package(&scratch, SHOWS_A_LIST);
    restore(
        &scratch,
        &files(&[("snapshots/lib/shows evens.snap", "[0, 2, 4]\r\n")]),
    );
    let (code, report) = test_on_both(&scratch, &[]);
    assert_eq!(code, Some(0), "{report}");
}

#[test]
fn a_test_name_cannot_put_a_snapshot_outside_its_directory() {
    let scratch = Scratch::new("snapshots", "escape");
    package(
        &scratch,
        "test \"../../escaped\" {
    t.assert_snapshot(1)
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(
        snapshot_files(&scratch),
        files(&[("snapshots/lib/_._.._escaped.snap", "1\n")])
    );
    assert!(!scratch.join("escaped.snap").exists());
}

#[test]
fn snapshot_assertions_in_a_helper_or_a_loop_are_numbered_as_they_run() {
    let scratch = Scratch::new("snapshots", "run_order");
    package(
        &scratch,
        "fn helper(t: TestContext, n: Int) {
    t.assert_snapshot(n)
}

test \"in a helper\" {
    helper(t, 7)
}

test \"in a loop\" {
    t.assert_snapshot(\"before\")
    for i in 0..2 {
        helper(t, i)
    }
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--update"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(
        snapshot_files(&scratch),
        files(&[
            ("snapshots/lib/in a helper.snap", "7\n"),
            ("snapshots/lib/in a loop-1.snap", "\"before\"\n"),
            ("snapshots/lib/in a loop-2.snap", "0\n"),
            ("snapshots/lib/in a loop-3.snap", "1\n"),
        ])
    );
    let (code, report) = test_on_both(&scratch, &[]);
    assert_eq!(code, Some(0), "{report}");
}

#[test]
fn a_panic_after_a_snapshot_assertion_is_the_failure_and_nothing_is_written() {
    let scratch = Scratch::new("snapshots", "panic_wins");
    package(
        &scratch,
        "fn half(n: Int, by: Int) -> Int {
    n / by
}

test \"divides\" {
    t.assert_snapshot(1)
    t.assert_eq(half(4, 0), 2)
}
",
    );
    for args in [&["--format=json"][..], &["--format=json", "--update"]] {
        let (code, report) = test_on_both(&scratch, args);
        assert_eq!(code, Some(1), "{report}");
        let failure = only_failure(&report);
        assert_eq!(text(failure.get("code")), "P0004", "{report}");
    }
    assert_eq!(snapshot_files(&scratch), []);
}

/// Make `link` a symlink to the directory `target`: `false` where this
/// platform or account cannot make one (Windows without the privilege).
fn symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    match made {
        Ok(()) => true,
        Err(err) => {
            eprintln!("skipped: cannot make a symlink here: {err}");
            false
        }
    }
}

#[test]
fn update_follows_no_symlink_out_of_the_package() {
    let scratch = Scratch::new("snapshots", "symlink");
    let outside = scratch.join("outside");
    let pkg = scratch.join("pkg");
    std::fs::create_dir_all(&outside).expect("make outside");
    std::fs::create_dir_all(&pkg).expect("make pkg");
    package(&pkg, SHOWS_A_LIST);
    if !symlink_dir(&outside, &pkg.join("snapshots")) {
        return;
    }
    let (code, report) = run_on_both(&pkg, &["--format=json", "--update"], || {});
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    assert!(
        text(failure.get("message")).contains("`snapshots` is a symlink"),
        "{report}"
    );
    assert_eq!(
        std::fs::read_dir(&outside).expect("outside").count(),
        0,
        "a snapshot was written through the symlink"
    );

    // A symlinked directory inside `snapshots/` is refused too, for
    // reading as well as writing.
    std::fs::remove_file(pkg.join("snapshots"))
        .or_else(|_| std::fs::remove_dir(pkg.join("snapshots")))
        .expect("remove the link");
    std::fs::create_dir_all(pkg.join("snapshots")).expect("make snapshots");
    std::fs::write(outside.join("shows evens.snap"), "[0, 2, 4]\n").expect("plant");
    assert!(symlink_dir(&outside, &under(&pkg, "snapshots/lib")));
    let (code, report) = run_on_both(&pkg, &["--format=json"], || {});
    assert_eq!(code, Some(1), "{report}");
    assert!(
        text(only_failure(&report).get("message")).contains("`snapshots/lib` is a symlink"),
        "{report}"
    );
}

#[test]
fn a_test_sees_only_its_stand_in_filesystem_not_the_snapshots() {
    let scratch = Scratch::new("snapshots", "stand_in");
    package(
        &scratch,
        "test \"cannot read its snapshot\" {
    let read = t.fs.read_only().restrict_to(\"snapshots\")
    t.assert(!read.list_dir(Path.new(\".\")).is_ok())
    t.assert_snapshot(1)
}
",
    );
    restore(
        &scratch,
        &files(&[("snapshots/lib/cannot read its snapshot.snap", "1\n")]),
    );
    let (code, report) = test_on_both(&scratch, &[]);
    assert_eq!(code, Some(0), "{report}");
}

/// Claude Code's Stop-hook input, run in `cwd`.
fn stop_gate(cwd: &Path) -> common::hook::Hooked {
    let input = format!(
        "{{\"session_id\":\"s\",\"hook_event_name\":\"Stop\",\"cwd\":{},\"stop_hook_active\":false}}",
        common::portable::json_str(&cwd.to_string_lossy())
    );
    common::hook::hook(&["hook", "claude", "--event=stop"], &input, cwd)
}

#[test]
fn the_stop_gate_blocks_on_a_missing_snapshot_and_passes_once_it_is_written() {
    let scratch = Scratch::new("snapshots", "stop");
    package(&scratch, SHOWS_A_LIST);
    let hooked = stop_gate(&scratch);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(hooked.stderr.contains("P0014"), "{}", hooked.stderr);
    assert_eq!(snapshot_files(&scratch), []);
    restore(
        &scratch,
        &files(&[("snapshots/lib/shows evens.snap", "[0, 2, 4]\n")]),
    );
    let hooked = stop_gate(&scratch);
    assert_eq!(hooked.code, Some(0), "{}", hooked.stderr);
}

#[test]
fn update_on_any_other_command_is_a_usage_error() {
    let scratch = Scratch::new("snapshots", "usage");
    package(&scratch, SHOWS_A_LIST);
    let output = oskac_in(&scratch, &["check", "--update"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(snapshot_files(&scratch), []);
}

/// Each frame of a JSON failure, as `function@line:column`.
fn frames_of(failure: &Json) -> Vec<String> {
    let Some(Json::Arr(frames)) = failure.get("frames") else {
        panic!("no `frames` in {failure:?}");
    };
    frames
        .iter()
        .map(|frame| {
            let field = |key: &str| frame.get(key).unwrap_or_else(|| panic!("no `{key}`"));
            let (Json::Str(function), Json::Num(line), Json::Num(column)) =
                (field("function"), field("line"), field("column"))
            else {
                panic!("a frame of the wrong shape: {frame:?}");
            };
            format!("{function}@{line}:{column}")
        })
        .collect()
}

#[test]
fn a_snapshot_failure_in_a_helper_lists_the_call_stack() {
    let scratch = Scratch::new("snapshots", "helper_frames");
    package(
        &scratch,
        "fn helper(t: TestContext, n: Int) {
    t.assert_snapshot(n)
}

test \"in a helper\" {
    helper(t, 7)
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--format=json"]);
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    assert_eq!(
        frames_of(&failure),
        ["helper@2:5", "test \"in a helper\"@6:5"],
        "{report}"
    );
}

#[test]
fn two_tests_that_name_one_snapshot_file_both_fail_and_nothing_is_written() {
    let scratch = Scratch::new("snapshots", "collide");
    package(
        &scratch,
        "test \"a/b\" {
    t.assert_snapshot(1)
}

test \"a_b\" {
    t.assert_snapshot(2)
}

test \"x-1\" {
    t.assert_snapshot(3)
}

test \"x\" {
    t.assert_snapshot(4)
    t.assert_snapshot(5)
}

test \"alone\" {
    t.assert_snapshot(6)
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(1), "{report}");
    let report = Json::parse(report.trim());
    let Some(Json::Arr(tests)) = report.get("tests") else {
        panic!("no tests in {report:?}");
    };
    let failed: Vec<(String, String)> = tests
        .iter()
        .filter_map(|test| {
            let failure = test.get("failure").filter(|f| **f != Json::Null)?;
            Some((text(test.get("name")), text(failure.get("message"))))
        })
        .collect();
    let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["a/b", "a_b", "x-1", "x"], "{report:?}");
    for (name, message) in &failed {
        assert!(
            message.contains("snapshots/lib/") && message.contains("also"),
            "{name}: {message}"
        );
    }
    assert!(failed[0].1.contains("\"a_b\""), "{}", failed[0].1);
    assert!(failed[2].1.contains("\"x\""), "{}", failed[2].1);
    assert_eq!(
        snapshot_files(&scratch),
        files(&[("snapshots/lib/alone.snap", "6\n")])
    );
}
