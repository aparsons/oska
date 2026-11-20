//! Snapshot assertions (spec #367, ticket #373): `t.assert_snapshot(value)`
//! compares the value, rendered as `dbg` renders it, with a file under
//! `snapshots/<test-file>/<test-name>.snap`, and `oskac test --update`
//! writes new and changed snapshots. Each test runs `oskac test` on a
//! scratch package or file, on the interpreter (`--interpret`) and
//! natively, and requires the two backends to report the same.

mod common;

use std::path::Path;
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
    std::fs::write(dir.join("src/lib.osk"), lib).expect("write the library");
}

/// `oskac test` with `args` in `dir` on each backend: the exit code and
/// the report, which must be the same on both, durations aside. Each
/// backend starts from the snapshots `dir` holds before the call.
fn test_on_both(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let before = snapshot_files(dir);
    let mut results = Vec::new();
    for backend in BACKENDS {
        restore(dir, &before);
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
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("make the directory");
        std::fs::write(file, text).expect("write the snapshot");
    }
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
fn a_test_with_more_than_one_snapshot_numbers_them_in_source_order() {
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
    std::fs::write(scratch.join("work/shapes.osk"), SHOWS_A_LIST).expect("write the file");
    for backend in BACKENDS {
        let _ = std::fs::remove_dir_all(scratch.join("work/snapshots"));
        let mut args = vec!["test", "work/shapes.osk", "--update"];
        args.extend_from_slice(backend);
        let output = oskac_in(&scratch, &args);
        assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
        let written =
            std::fs::read_to_string(scratch.join("work/snapshots/shapes/shows evens.snap"))
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
fn a_snapshot_assertion_outside_the_test_body_or_run_twice_fails() {
    let scratch = Scratch::new("snapshots", "misuse");
    package(
        &scratch,
        "fn helper(t: TestContext) {
    t.assert_snapshot(1)
}

test \"in a helper\" {
    helper(t)
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    assert!(
        text(failure.get("message")).contains("directly in the test's body"),
        "{report}"
    );
    package(
        &scratch,
        "test \"in a loop\" {
    for i in 0..2 {
        t.assert_snapshot(i)
    }
}
",
    );
    let (code, report) = test_on_both(&scratch, &["--format=json", "--update"]);
    assert_eq!(code, Some(1), "{report}");
    let failure = only_failure(&report);
    assert_eq!(text(failure.get("code")), "P0014");
    assert!(
        text(failure.get("message")).contains("more than once"),
        "{report}"
    );
    assert_eq!(snapshot_files(&scratch), []);
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
