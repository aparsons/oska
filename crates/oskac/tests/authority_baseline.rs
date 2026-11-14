//! The authority baseline and its diff (#196): `oskac check
//! --update-baseline=<file>` writes a program's authority as a stable,
//! span-free file, and `oskac check --baseline=<file>` compares the
//! current report with it and exits with 3 when authority widened.
//!
//! Each case is a directory under `tests/fixtures/authority_diff/`: the
//! program before (`before.osk`), whose baseline the test writes, and the
//! program after (`after.osk`), which is checked against it. A case may
//! bring its own `baseline.json` instead of a `before.osk`. The output of
//! the check against the baseline, its exit code and standard error are
//! golden files under `tests/golden/authority_diff/`, in text and JSON.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test authority_baseline` and
//! review the diff.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURES: &str = "crates/oskac/tests/fixtures/authority_diff";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("oskac should run")
}

/// A scratch file for case `name`, removed if a previous run left it.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("authority_baseline");
    std::fs::create_dir_all(&dir).expect("should create the scratch directory");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

/// Write the baseline of `program` (a path from the repository root) to
/// `to` with `--update-baseline`, which must succeed quietly.
fn update_baseline(program: &str, to: &Path) {
    let flag = format!("--update-baseline={}", to.display());
    let output = oskac(&["check", program, &flag]);
    assert!(
        output.status.success(),
        "writing the baseline of {program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The baseline case `case` is compared with: its own `baseline.json`, or
/// one written from its `before.osk`.
fn baseline_of(case: &str) -> PathBuf {
    let own = root().join(FIXTURES).join(case).join("baseline.json");
    if own.exists() {
        return own;
    }
    let path = scratch(&format!("{case}.json"));
    update_baseline(&format!("{FIXTURES}/{case}/before.osk"), &path);
    path
}

/// What `oskac check <case>/after.osk --baseline=<baseline>` shows: the
/// exit code, standard output and standard error, with the baseline's
/// path and `oskac`'s version written as placeholders so the golden files
/// hold on every machine and release.
fn compare(case: &str, baseline: &Path, json: bool) -> String {
    let program = format!("{FIXTURES}/{case}/after.osk");
    let flag = format!("--baseline={}", baseline.display());
    let mut args = vec!["check", program.as_str(), flag.as_str()];
    if json {
        args.push("--format=json");
    }
    let output = oskac(&args);
    let shown = format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let escaped = baseline.display().to_string().replace('\\', "\\\\");
    shown
        .replace(&escaped, "<baseline>")
        .replace(&baseline.display().to_string(), "<baseline>")
        .replace(env!("CARGO_PKG_VERSION"), "<version>")
}

fn assert_golden(actual: &str, name: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/authority_diff")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "the output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// Compare case `case`'s program with its baseline, in text and JSON,
/// against the golden files, and return the exit code.
fn golden_case(case: &str) -> String {
    let baseline = baseline_of(case);
    let text = compare(case, &baseline, false);
    let json = compare(case, &baseline, true);
    assert_golden(&text, &format!("{case}.txt"));
    assert_golden(&json, &format!("{case}.json"));
    let exit = |shown: &str| shown.lines().next().unwrap_or_default().to_string();
    assert_eq!(exit(&text), exit(&json), "text and JSON exit differently");
    exit(&text)
}

#[test]
fn a_function_that_gains_a_capability_fails_the_gate() {
    assert_eq!(golden_case("gains_net"), "exit: 3");
}

#[test]
fn narrowing_fs_to_dir_read_is_reported_and_passes() {
    assert_eq!(golden_case("narrows"), "exit: 0");
}

#[test]
fn a_pure_function_that_becomes_impure_fails_the_gate() {
    assert_eq!(golden_case("becomes_impure"), "exit: 3");
}

#[test]
fn a_new_audit_site_fails_the_gate() {
    assert_eq!(golden_case("audit_added"), "exit: 3");
}

#[test]
fn a_removed_audit_site_is_reported_and_passes() {
    assert_eq!(golden_case("audit_removed"), "exit: 0");
}

#[test]
fn a_renamed_function_is_one_removed_and_one_added() {
    assert_eq!(golden_case("renamed"), "exit: 0");
}

#[test]
fn a_new_helper_within_the_programs_authority_passes() {
    assert_eq!(golden_case("helper_within"), "exit: 0");
}

#[test]
fn a_new_helper_with_a_kind_the_program_never_had_fails_the_gate() {
    assert_eq!(golden_case("helper_new_kind"), "exit: 3");
}

#[test]
fn a_test_blocks_attenuations_and_audit_sites_are_reported_and_pass() {
    assert_eq!(golden_case("test_block"), "exit: 0");
}

#[test]
fn an_impure_function_that_gains_a_cap_fn_parameter_widened() {
    assert_eq!(golden_case("gains_cap_fn"), "exit: 3");
}

#[test]
fn a_baseline_from_another_oskac_is_named_in_the_diff() {
    assert_eq!(golden_case("older_oskac"), "exit: 3");
}

#[test]
fn duplicate_function_names_stop_the_diff_with_an_error() {
    assert_eq!(golden_case("duplicate_names"), "exit: 1");
}

/// Write `src` to a scratch file named `name` and return its path.
fn source(name: &str, src: &str) -> PathBuf {
    let path = scratch(name);
    std::fs::write(&path, src).expect("should write the source file");
    path
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn the_baseline_is_sorted_span_free_and_the_same_bytes_everywhere() {
    let written = scratch("narrows_before.json");
    update_baseline(&format!("{FIXTURES}/narrows/before.osk"), &written);
    let bytes = std::fs::read(&written).expect("the baseline should be written");
    let expected = format!(
        "{{\n  \"format\": 1,\n  \"oskac\": \"{}\",\n  \"functions\": [\n    \
         {{\"name\":\"main\",\"authority\":[\"Fs\",\"Stdout\"],\"pure\":false}},\n    \
         {{\"name\":\"notes\",\"authority\":[\"Fs\"],\"pure\":false}}\n  ],\n  \
         \"audit\": [],\n  \"attenuations\": [\n    \
         {{\"function\":\"notes\",\"method\":\"read_only\",\"source\":\"Dir\",\"result\":\"DirRead\",\"root\":null,\"count\":1}},\n    \
         {{\"function\":\"notes\",\"method\":\"restrict_to\",\"source\":\"Fs\",\"result\":\"Dir\",\"root\":\"notes\",\"count\":1}}\n  \
         ]\n}}\n",
        env!("CARGO_PKG_VERSION")
    );
    // Byte for byte, so LF line endings and the final newline on every
    // platform.
    assert_eq!(String::from_utf8_lossy(&bytes), expected);

    // Moving the code, or checking it from another path, changes nothing.
    let original = std::fs::read_to_string(root().join(FIXTURES).join("narrows/before.osk"))
        .expect("the fixture should read");
    let moved = source("moved.osk", &format!("// Moved down.\n\n\n{original}"));
    let rewritten = scratch("moved.json");
    update_baseline(&text(&moved), &rewritten);
    assert_eq!(std::fs::read(&rewritten).ok(), Some(bytes));
}

#[test]
fn an_unchanged_program_passes_and_says_so() {
    let baseline = scratch("valid_package.json");
    let package = "crates/oskac/tests/fixtures/packages/modules";
    update_baseline(package, &baseline);
    let flag = format!("--baseline={}", text(&baseline));
    let output = oskac(&["check", package, &flag]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.ends_with(&format!(
            "baseline: {}\n  no change in authority\n",
            text(&baseline)
        )),
        "{stdout}"
    );
}

#[test]
fn a_saved_check_report_serves_as_the_baseline() {
    let report = oskac(&[
        "check",
        &format!("{FIXTURES}/narrows/before.osk"),
        "--format=json",
    ]);
    assert!(report.status.success());
    let saved = scratch("narrows_report.json");
    std::fs::write(&saved, &report.stdout).expect("should save the report");
    let written = baseline_of("narrows");
    let shown = |baseline: &Path| {
        compare("narrows", baseline, false)
            .lines()
            .filter(|line| !line.starts_with("baseline: "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(shown(&saved), shown(&written));
    assert!(shown(&saved).starts_with("exit: 0\n"));
    // A report says nothing of which oskac wrote it.
    assert!(compare("narrows", &saved, true).contains("\"baseline_oskac\": null,"));

    // So a widening against a saved report fails the gate too.
    let report = oskac(&[
        "check",
        &format!("{FIXTURES}/gains_net/before.osk"),
        "--format=json",
    ]);
    let saved = scratch("gains_net_report.json");
    std::fs::write(&saved, &report.stdout).expect("should save the report");
    assert!(compare("gains_net", &saved, false).starts_with("exit: 3\n"));
}

const BROKEN: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(missing)\n}\n";

#[test]
fn diagnostics_exit_1_with_no_diff() {
    let program = source("broken.osk", BROKEN);
    // The baseline is never read: the program's report is incomplete.
    let absent = scratch("absent.json");
    let flag = format!("--baseline={}", text(&absent));
    let output = oskac(&["check", &text(&program), &flag]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("baseline"));
    let output = oskac(&["check", &text(&program), &flag, "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("authority_diff"));
}

#[test]
fn a_program_with_diagnostics_writes_no_baseline() {
    let program = source("broken_update.osk", BROKEN);
    let baseline = scratch("broken_update.json");
    let flag = format!("--update-baseline={}", text(&baseline));
    let output = oskac(&["check", &text(&program), &flag]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!baseline.exists(), "the baseline should not be written");
    assert!(String::from_utf8_lossy(&output.stderr).contains("was not written"));
}

#[test]
fn a_baseline_that_cannot_be_read_fails_closed() {
    let program = format!("{FIXTURES}/gains_net/before.osk");
    let bad = [
        ("not_json", "functions"),
        ("array", "[]"),
        ("no_functions", r#"{"audit": [], "attenuations": []}"#),
        ("no_audit", r#"{"functions": [], "attenuations": []}"#),
        (
            "format_2",
            r#"{"format": 2, "functions": [], "audit": [], "attenuations": []}"#,
        ),
        (
            "with_diagnostics",
            r#"{"diagnostics": [{"code":"E0200"}], "functions": [], "audit": [], "attenuations": []}"#,
        ),
        (
            "no_pure",
            r#"{"functions": [{"name":"main","authority":["Stdout"]}], "audit": [], "attenuations": []}"#,
        ),
        (
            "authority_not_strings",
            r#"{"functions": [{"name":"main","authority":[1],"pure":false}], "audit": [], "attenuations": []}"#,
        ),
        (
            "zero_count",
            r#"{"functions": [], "audit": [{"function":"main","kind":"expose","count":0}], "attenuations": []}"#,
        ),
        (
            "audit_without_function",
            r#"{"functions": [], "audit": [{"kind":"expose"}], "attenuations": []}"#,
        ),
        (
            "duplicate_key",
            r#"{"functions": [], "functions": [], "audit": [], "attenuations": []}"#,
        ),
    ];
    for (name, contents) in bad {
        let baseline = scratch(&format!("bad_{name}.json"));
        std::fs::write(&baseline, contents).expect("should write the baseline");
        let flag = format!("--baseline={}", text(&baseline));
        for json in [false, true] {
            let mut args = vec!["check", program.as_str(), flag.as_str()];
            if json {
                args.push("--format=json");
            }
            let output = oskac(&args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{name}: {stderr}");
            assert!(stderr.starts_with("error: "), "{name}: {stderr}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("authority_diff"));
        }
    }
    let missing = scratch("missing.json");
    let output = oskac(&["check", &program, &format!("--baseline={}", text(&missing))]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn baseline_flags_are_usage_errors_out_of_place() {
    let program = format!("{FIXTURES}/gains_net/before.osk");
    for args in [
        vec![
            "check",
            &program,
            "--baseline=a.json",
            "--update-baseline=b.json",
        ],
        vec!["check", &program, "--baseline=a.json", "--baseline=b.json"],
        vec!["check", &program, "--baseline="],
        vec![
            "check",
            "--update-baseline=a.json",
            "--update-baseline=b.json",
        ],
        vec!["run", &program, "--baseline=a.json"],
        vec!["test", &program, "--update-baseline=a.json"],
    ] {
        let output = oskac(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    // There is no flag that lets a widening pass.
    let help = oskac(&["--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("--baseline=<file>") && help.contains("--update-baseline=<file>"));
    assert!(!help.contains("allow"), "{help}");
}
