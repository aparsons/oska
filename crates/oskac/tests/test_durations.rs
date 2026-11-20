//! Per-test durations in `oskac test --format=json` (spec #367, ticket
//! #372): each test object carries `duration_ms`, an integer, measured the
//! same way natively and with `--interpret`, so an agent can see which
//! test is slow.
//!
//! Durations vary from run to run, so the golden file masks the number
//! (`common::golden::mask_durations`, which fails on anything but an
//! integer), and the other cases check the numbers by their bounds. Every
//! `test` case runs by default (natively where the host has a backend) and
//! with `--interpret`. To regenerate the golden file after an intended
//! change, run `UPDATE_GOLDEN=1 cargo test -p oskac --test test_durations`
//! and review the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::golden::{assert_golden_file, mask_durations};
use common::portable::{literal_path, output_of};
use common::Json;

/// `quick` passes at once, `slow` loops for a while and passes, and
/// `skipped` is `#[integration]`.
const FILE: &str = "crates/oskac/tests/fixtures/test_durations.osk";

fn repo_root() -> PathBuf {
    literal_path(Path::new(env!("CARGO_MANIFEST_DIR")), &["..", ".."])
}

/// What `oskac <args>`, run from the repository root, ended with and
/// printed, and how long it took.
fn oskac(args: &[&str]) -> (std::process::Output, Duration) {
    let started = Instant::now();
    let output = output_of(
        Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(repo_root()),
    );
    (output, started.elapsed())
}

/// `args`, and `args` with `--interpret`: the two backends.
fn both_backends(args: &[&str]) -> [Vec<String>; 2] {
    let native: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut interpreted = native.clone();
    interpreted.push("--interpret".to_string());
    [native, interpreted]
}

/// The JSON report of `oskac <args>`, and how long the run took.
fn report(args: &[String]) -> (Json, Duration) {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (output, took) = oskac(&args);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert_eq!(output.status.code(), Some(0), "{args:?}\n{stdout}");
    (Json::parse(&stdout), took)
}

/// Each test's name and `duration_ms` in `report`, in order.
fn durations(report: &Json) -> Vec<(String, u64)> {
    let Some(Json::Arr(tests)) = report.get("tests") else {
        panic!("a `tests` list in {report:?}");
    };
    tests
        .iter()
        .map(|test| {
            let Some(Json::Str(name)) = test.get("name") else {
                panic!("a name in {test:?}");
            };
            match test.get("duration_ms") {
                Some(Json::Num(ms)) => (name.clone(), *ms),
                other => panic!("test {name}: `duration_ms` should be an integer, not {other:?}"),
            }
        })
        .collect()
}

#[test]
fn every_test_in_the_json_report_has_an_integer_duration_on_both_backends() {
    for args in both_backends(&["test", FILE, "--format=json"]) {
        let (report, _) = report(&args);
        let names: Vec<String> = durations(&report).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["quick", "slow", "skipped"], "{args:?}");
    }
}

#[test]
fn a_skipped_test_takes_no_time() {
    for args in both_backends(&["test", FILE, "--format=json"]) {
        let (report, _) = report(&args);
        assert_eq!(
            durations(&report)[2],
            ("skipped".to_string(), 0),
            "{args:?}"
        );
    }
}

#[test]
fn a_slow_test_shows_its_time_and_no_more_than_the_whole_run_took() {
    for args in both_backends(&["test", FILE, "--format=json"]) {
        let (report, took) = report(&args);
        let durations = durations(&report);
        assert!(
            durations[1].1 >= 1,
            "{args:?}: the slow test should take a millisecond or more: {durations:?}"
        );
        let total: u64 = durations.iter().map(|(_, ms)| ms).sum();
        assert!(
            u128::from(total) <= took.as_millis(),
            "{args:?}: the tests took {total} ms in a run of {} ms: {durations:?}",
            took.as_millis()
        );
    }
}

#[test]
fn the_json_report_puts_the_duration_after_the_status() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/test_durations/json.txt");
    for args in both_backends(&["test", FILE, "--format=json", "--filter=quick"]) {
        let strs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (output, _) = oskac(&strs);
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
        assert_eq!(output.status.code(), Some(0), "{args:?}\n{stdout}");
        assert_golden_file(&golden, &mask_durations(&stdout));
    }
}

#[test]
fn the_human_report_shows_no_durations() {
    for args in both_backends(&["test", FILE]) {
        let strs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (output, _) = oskac(&strs);
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
        assert_eq!(
            stdout,
            "test quick ... ok\n\
             test slow ... ok\n\
             test skipped ... skipped (#[integration]; run with --integration)\n\
             \n\
             3 tests: 2 passed, 0 failed, 1 skipped\n",
            "{args:?}"
        );
    }
}

#[test]
fn masking_a_duration_keeps_the_rest_and_refuses_a_number_that_is_not_an_integer() {
    assert_eq!(
        mask_durations("{\"duration_ms\":0,\"x\":1},{\"duration_ms\":1234}"),
        "{\"duration_ms\":<ms>,\"x\":1},{\"duration_ms\":<ms>}"
    );
    for malformed in [
        "{\"duration_ms\":null}",
        "{\"duration_ms\":-1}",
        "{\"duration_ms\":\"3\"}",
    ] {
        let masked = std::panic::catch_unwind(|| mask_durations(malformed));
        assert!(masked.is_err(), "{malformed} should not be masked");
    }
}
