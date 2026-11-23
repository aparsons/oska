// SPDX-License-Identifier: Apache-2.0
//! The authority grant (#451): `oskac check --grant=<file>` and `oskac run
//! --grant=<file>` compare a program's authority with the most an
//! embedder allows it, by the authority baseline's rules and format, and
//! refuse it with exit code 3 when it goes past. A grant is never written
//! or updated by `oskac`.
//!
//! Every case of the authority baseline tests is checked against a grant
//! written from its program before (`tests/fixtures/authority_diff/`).
//! The grant's own cases are directories under
//! `tests/fixtures/authority_grant/`: a `grant.json` and a `program.osk`.
//! The output, exit code and standard error are golden files under
//! `tests/golden/authority_grant/`, in text and JSON.
//!
//! To regenerate the golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test authority_grant` and
//! review the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::portable::{json_escape, output_of, output_with_input, Scratch};

const DIFF_FIXTURES: &str = "crates/oskac/tests/fixtures/authority_diff";
const FIXTURES: &str = "crates/oskac/tests/fixtures/authority_grant";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh scratch directory of its own for each call, so parallel tests
/// never share one.
fn scratch_dir(name: &str) -> Scratch {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Scratch::new("oska-authority-grant", &format!("{name}-{n}"))
}

/// `oskac args`, run from the repository root, with a build cache of its
/// own in `dir`.
fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command
        .args(args)
        .current_dir(root())
        .env("OSKA_CACHE_DIR", dir.join("cache"))
        .env_remove("OSKA_CACHE")
        .env_remove("OSKA_CACHE_LIMIT");
    command
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    let mut command = command(dir, args);
    command.stdin(Stdio::null());
    output_of(&mut command)
}

/// What an `oskac` command shows: the exit code, standard output and
/// standard error, with the grant's path and `oskac`'s version written as
/// placeholders so the golden files hold on every machine and release.
fn shown(output: &Output, grant: &Path) -> String {
    let shown = format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let path = grant.display().to_string();
    shown
        .replace(&json_escape(&path), "<grant>")
        .replace(&path, "<grant>")
        .replace(env!("CARGO_PKG_VERSION"), "<version>")
}

fn assert_golden(actual: &str, name: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/authority_grant")
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

/// What [`shown`] shows on standard output and on standard error.
fn parts(shown: &str) -> (&str, &str) {
    let (head, stderr) = shown.split_once("--- stderr\n").expect("a stderr section");
    let stdout = head.split_once("--- stdout\n").expect("a stdout section").1;
    (stdout, stderr)
}

/// The first line of what [`shown`] shows: `exit: <code>`.
fn exit(shown: &str) -> String {
    shown.lines().next().unwrap_or_default().to_string()
}

/// A grant and the program checked against it, in a scratch directory.
struct Case {
    dir: Scratch,
    grant: PathBuf,
    program: String,
}

impl Case {
    /// A baseline test's case: its `after.osk`, against a grant that is
    /// the case's own `baseline.json`, or the baseline of its
    /// `before.osk`, as an embedder writes a grant from a saved report.
    fn from_baseline_case(case: &str) -> Case {
        let dir = scratch_dir(case);
        let grant = dir.join("grant.json");
        let own = root().join(DIFF_FIXTURES).join(case).join("baseline.json");
        if own.exists() {
            std::fs::copy(&own, &grant).expect("should copy the case's baseline");
        } else {
            let flag = format!("--update-baseline={}", grant.display());
            let before = format!("{DIFF_FIXTURES}/{case}/before.osk");
            let output = oskac(&dir, &["check", &before, &flag]);
            assert!(output.status.success(), "{output:?}");
        }
        Case {
            dir,
            grant,
            program: format!("{DIFF_FIXTURES}/{case}/after.osk"),
        }
    }

    /// One of the grant's own cases: its `program.osk`, or the case
    /// directory as a package when it has an `oska.toml`.
    fn own(case: &str) -> Case {
        let dir = scratch_dir(case);
        let grant = dir.join("grant.json");
        let fixture = root().join(FIXTURES).join(case);
        std::fs::copy(fixture.join("grant.json"), &grant).expect("should copy the case's grant");
        let program = if fixture.join("oska.toml").exists() {
            format!("{FIXTURES}/{case}")
        } else {
            format!("{FIXTURES}/{case}/program.osk")
        };
        Case {
            dir,
            grant,
            program,
        }
    }

    fn flag(&self) -> String {
        format!("--grant={}", self.grant.display())
    }

    /// `oskac <command> <program> --grant=<grant> extra...`, as shown.
    fn on(&self, command: &str, extra: &[&str]) -> String {
        let flag = self.flag();
        let mut args = vec![command, self.program.as_str(), flag.as_str()];
        args.extend_from_slice(extra);
        shown(&oskac(&self.dir, &args), &self.grant)
    }

    /// `oskac <command> - --grant=<grant> extra...` with the program on
    /// standard input, as shown.
    fn piped(&self, command: &str, extra: &[&str]) -> String {
        let src = std::fs::read(root().join(&self.program)).expect("should read the program");
        let flag = self.flag();
        let mut args = vec![command, "-", flag.as_str()];
        args.extend_from_slice(extra);
        let output = output_with_input(&mut self::command(&self.dir, &args), &src);
        shown(&output, &self.grant)
    }

    /// Check the program against the grant in text and JSON, compare both
    /// with the golden files `<name>.txt` and `<name>.json`, and return
    /// the exit code.
    fn golden_check(&self, name: &str) -> String {
        let text = self.on("check", &[]);
        let json = self.on("check", &["--format=json"]);
        assert_golden(&text, &format!("{name}.txt"));
        assert_golden(&json, &format!("{name}.json"));
        assert_eq!(exit(&text), exit(&json), "text and JSON exit differently");
        exit(&text)
    }
}

fn baseline_case(case: &str) -> String {
    Case::from_baseline_case(case).golden_check(case)
}

#[test]
fn a_function_that_gains_a_capability_goes_past_the_grant() {
    assert_eq!(baseline_case("gains_net"), "exit: 3");
}

#[test]
fn narrowing_fs_to_dir_read_is_within_the_grant() {
    assert_eq!(baseline_case("narrows"), "exit: 0");
}

#[test]
fn a_pure_function_that_becomes_impure_goes_past_the_grant() {
    assert_eq!(baseline_case("becomes_impure"), "exit: 3");
}

#[test]
fn a_new_audit_site_goes_past_the_grant() {
    assert_eq!(baseline_case("audit_added"), "exit: 3");
}

#[test]
fn a_removed_audit_site_is_within_the_grant() {
    assert_eq!(baseline_case("audit_removed"), "exit: 0");
}

#[test]
fn a_renamed_function_is_one_removed_and_one_added() {
    assert_eq!(baseline_case("renamed"), "exit: 0");
}

#[test]
fn a_new_helper_within_the_grants_authority_passes() {
    assert_eq!(baseline_case("helper_within"), "exit: 0");
}

#[test]
fn a_new_helper_with_a_kind_the_grant_never_had_goes_past_it() {
    assert_eq!(baseline_case("helper_new_kind"), "exit: 3");
}

#[test]
fn a_test_blocks_attenuations_and_audit_sites_are_within_the_grant() {
    assert_eq!(baseline_case("test_block"), "exit: 0");
}

#[test]
fn an_impure_function_that_gains_a_cap_fn_parameter_goes_past_the_grant() {
    assert_eq!(baseline_case("gains_cap_fn"), "exit: 3");
}

#[test]
fn a_grant_from_another_oskac_is_named_in_the_diff() {
    assert_eq!(baseline_case("older_oskac"), "exit: 3");
}

#[test]
fn duplicate_function_names_in_the_grant_stop_the_diff_with_an_error() {
    assert_eq!(baseline_case("duplicate_names"), "exit: 1");
}

#[test]
fn a_grant_with_only_main_refuses_a_helper_that_reaches_a_kind_main_lacks() {
    let case = Case::own("main_only_helper_new_kind");
    assert_eq!(case.golden_check("main_only_helper_new_kind"), "exit: 3");
}

#[test]
fn a_grant_with_only_main_allows_a_helper_within_mains_authority() {
    let case = Case::own("main_only_helper_within");
    assert_eq!(case.golden_check("main_only_helper_within"), "exit: 0");
}

#[test]
fn run_refuses_a_program_past_its_grant_and_runs_nothing() {
    let case = Case::own("main_only_helper_new_kind");
    let shown = case.on("run", &[]);
    assert_golden(&shown, "run_main_only_helper_new_kind.txt");
    assert!(
        shown.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
        "{shown}"
    );
    let json = case.on("run", &["--format=json"]);
    assert_golden(&json, "run_main_only_helper_new_kind.json");
    // One JSON object on one line, the error inside it.
    let (_, stderr) = parts(&json);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("{\"authority_diff\":{"), "{stderr}");
    assert!(stderr.ends_with("\"verdict\":\"widened\",\"error\":\"authority goes past the grant <grant>: a function gained authority or became impure, an audit site was added, or the program reaches a new kind; a grant is set by whoever runs the code, and oskac never updates it; the program was not run\"}\n"), "{stderr}");
    assert!(
        json.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
        "{json}"
    );
}

#[test]
fn run_runs_a_program_within_its_grant() {
    let case = Case::own("main_only_helper_within");
    assert_eq!(
        case.on("run", &[]),
        "exit: 0\n--- stdout\nhello, grant\n--- stderr\n"
    );
    assert_eq!(
        case.on("run", &["--interpret"]),
        "exit: 0\n--- stdout\nhello, grant\n--- stderr\n"
    );
}

#[test]
fn run_dash_with_a_grant_checks_the_snippet_on_stdin_against_it() {
    let within = Case::own("within");
    assert_eq!(
        within.piped("run", &[]),
        "exit: 0\n--- stdout\nran\n--- stderr\n"
    );
    let past = Case::own("main_only_helper_new_kind");
    let shown = past.piped("run", &[]);
    assert!(
        shown.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
        "{shown}"
    );
    let checked = past.piped("check", &["--format=json"]);
    assert!(checked.starts_with("exit: 3\n"), "{checked}");
    assert!(checked.contains("\"grant\": \"<grant>\""), "{checked}");
    assert!(checked.contains("\"verdict\": \"widened\""), "{checked}");
}

#[test]
fn check_and_run_with_a_grant_refuse_a_package_past_it() {
    let case = Case::own("package_past");
    assert_eq!(case.golden_check("package_past"), "exit: 3");
    for run in [case.on("run", &[]), case.on("run", &["--interpret"])] {
        assert!(
            run.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
            "{run}"
        );
        // A package's functions are named by module, as in its report.
        assert!(run.contains("added fn pkg.main.fetch: NetConnect"), "{run}");
    }
}

#[test]
fn check_and_run_with_a_grant_pass_a_package_within_it() {
    let case = Case::own("package_within");
    assert_eq!(case.golden_check("package_within"), "exit: 0");
    for run in [case.on("run", &[]), case.on("run", &["--interpret"])] {
        assert_eq!(run, "exit: 0\n--- stdout\nran\n--- stderr\n");
    }
}

#[test]
fn an_unchanged_program_is_within_its_grant_and_says_so() {
    let case = Case::own("within");
    assert_eq!(case.golden_check("within"), "exit: 0");
    let shown = case.on("check", &[]);
    let (stdout, _) = parts(&shown);
    assert!(
        stdout.ends_with("grant: <grant>\n  no change in authority\n"),
        "{stdout}"
    );
}

#[test]
fn a_saved_check_report_serves_as_the_grant() {
    let case = Case::own("within");
    let dir = case.dir.path().to_path_buf();
    let report = oskac(&dir, &["check", &case.program, "--format=json"]);
    assert!(report.status.success());
    std::fs::write(&case.grant, &report.stdout).expect("should save the report");
    let checked = case.on("check", &["--format=json"]);
    assert!(checked.starts_with("exit: 0\n"), "{checked}");
    // A report says nothing of which oskac wrote it.
    assert!(checked.contains("\"grant_oskac\": null,"), "{checked}");
    assert_eq!(
        case.on("run", &[]),
        "exit: 0\n--- stdout\nran\n--- stderr\n"
    );

    // A snippet past what the saved report allows is refused.
    let past = Case::own("main_only_helper_new_kind");
    std::fs::write(&past.grant, &report.stdout).expect("should save the report");
    assert!(past.on("check", &[]).starts_with("exit: 3\n"));
    assert!(past
        .on("run", &[])
        .starts_with("exit: 3\n--- stdout\n--- stderr\n"));
}

/// Security (fail closed): a program with diagnostics is never run, and
/// its grant is never read, on `check` or on `run`, on either backend.
#[test]
fn a_program_with_diagnostics_is_compared_with_nothing_and_never_runs() {
    let case = Case::own("diagnostics");
    // Even a grant that cannot be read is never reached.
    std::fs::remove_file(&case.grant).expect("should remove the grant");
    assert_eq!(case.golden_check("diagnostics"), "exit: 1");
    let run = case.on("run", &[]);
    assert_golden(&run, "run_diagnostics.txt");
    assert_eq!(case.on("run", &["--interpret"]), run);
    let json = case.on("run", &["--format=json"]);
    assert_golden(&json, "run_diagnostics.json");
    for shown in [&run, &json] {
        let (stdout, stderr) = parts(shown);
        assert!(shown.starts_with("exit: 1\n"), "{shown}");
        assert_eq!(stdout, "");
        assert!(stderr.contains("E0"), "{shown}");
        // The grant, which does not exist, was never read.
        assert!(!stderr.contains("<grant>"), "{shown}");
    }
}

/// Security: a snippet that vouches for text it did not write fails a
/// grant that lists no audit site, on `check` and on `run`, although it
/// gains no capability.
#[test]
fn an_assume_trusted_the_grant_does_not_list_is_refused_on_check_and_run() {
    let case = Case::own("assume_trusted");
    assert_eq!(case.golden_check("assume_trusted"), "exit: 3");
    for run in [case.on("run", &[]), case.on("run", &["--interpret"])] {
        assert!(
            run.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
            "{run}"
        );
        assert!(
            run.contains("new audit site fn vouch: assume_trusted +1"),
            "{run}"
        );
    }
    let piped = case.piped("run", &[]);
    assert!(
        piped.starts_with("exit: 3\n--- stdout\n--- stderr\n"),
        "{piped}"
    );
}

const BROKEN: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(missing)\n}\n";

#[test]
fn diagnostics_exit_1_and_compare_nothing() {
    let dir = scratch_dir("broken");
    let program = dir.join("broken.osk");
    std::fs::write(&program, BROKEN).expect("should write the program");
    let program = program.display().to_string();
    // The grant is never read: the program's report is incomplete.
    let flag = format!("--grant={}", dir.join("absent.json").display());
    let output = oskac(&dir, &["check", &program, &flag]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("grant"));
    let output = oskac(&dir, &["check", &program, &flag, "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("authority_diff"));
    let output = oskac(&dir, &["run", &program, &flag]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn an_unreadable_or_malformed_grant_is_an_error_and_nothing_runs() {
    let case = Case::own("within");
    let bad = [
        ("not_json", "functions"),
        ("array", "[]"),
        (
            "no_format",
            r#"{"functions": [], "audit": [], "attenuations": []}"#,
        ),
        (
            "format_2",
            r#"{"format": 2, "functions": [], "audit": [], "attenuations": []}"#,
        ),
        (
            "no_pure",
            r#"{"format": 1, "functions": [{"name":"main","authority":["Stdout"]}], "audit": [], "attenuations": []}"#,
        ),
        (
            "duplicate_key",
            r#"{"format": 1, "functions": [], "functions": [], "audit": [], "attenuations": []}"#,
        ),
    ];
    for (name, contents) in bad {
        std::fs::write(&case.grant, contents).expect("should write the grant");
        for (command, extra) in [
            ("check", vec![]),
            ("check", vec!["--format=json"]),
            ("run", vec![]),
            ("run", vec!["--interpret"]),
        ] {
            let shown = case.on(command, &extra);
            // Nothing was compared and nothing ran: no check object and
            // no output of the program; the human check still lists the
            // program's authority, as with a baseline.
            let (stdout, stderr) = parts(&shown);
            assert!(shown.starts_with("exit: 1\n"), "{name}: {shown}");
            assert!(
                stderr.starts_with("error: the grant <grant>: "),
                "{name} {command} {extra:?}: {shown}"
            );
            let expected = if command == "check" && extra.is_empty() {
                "fn main: Stdout (io)\n"
            } else {
                ""
            };
            assert_eq!(stdout, expected, "{name} {command} {extra:?}");
        }
    }
    std::fs::remove_file(&case.grant).expect("should remove the grant");
    for command in ["check", "run"] {
        let shown = case.on(command, &[]);
        let (stdout, stderr) = parts(&shown);
        assert!(shown.starts_with("exit: 1\n"), "{shown}");
        assert!(
            stderr.starts_with("error: cannot read the grant <grant>: "),
            "{shown}"
        );
        assert!(!stdout.contains("ran"), "{shown}");
    }
}

#[test]
fn grant_flags_are_usage_errors_out_of_place() {
    let dir = scratch_dir("usage");
    let program = format!("{FIXTURES}/within/program.osk");
    for args in [
        vec!["check", &program, "--grant=a.json", "--baseline=b.json"],
        vec![
            "check",
            &program,
            "--grant=a.json",
            "--update-baseline=b.json",
        ],
        vec!["run", &program, "--grant=a.json", "--baseline=b.json"],
        vec!["check", &program, "--grant=a.json", "--grant=b.json"],
        vec!["check", &program, "--grant="],
        vec!["run", &program, "--grant="],
        vec!["test", &program, "--grant=a.json"],
        vec!["build", &program, "--grant=a.json"],
        vec!["fix", &program, "--grant=a.json"],
        vec!["lex", &program, "--grant=a.json"],
        vec!["parse", &program, "--grant=a.json"],
        vec!["explain", "E0001", "--grant=a.json"],
        vec!["cache", "info", "--grant=a.json"],
    ] {
        let output = oskac(&dir, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    // A grant is never written, and nothing lets a program pass it.
    let help = oskac(&dir, &["--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("--grant=<file>"), "{help}");
    assert!(
        !help.contains("update-grant") && !help.contains("allow"),
        "{help}"
    );
    let output = oskac(&dir, &["check", &program, "--update-grant=a.json"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_grant_is_never_written() {
    let case = Case::own("main_only_helper_new_kind");
    let before = std::fs::read(&case.grant).expect("the grant");
    let _ = case.on("check", &[]);
    let _ = case.on("run", &[]);
    assert_eq!(std::fs::read(&case.grant).ok(), Some(before));
}
