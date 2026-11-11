//! Seam 2 for packages (spec #115, ticket #117): `oskac check`, `run` and
//! `test` over fixture packages on disk, with golden output.
//!
//! Each case runs the real binary from the repository root (or from the
//! directory it names) and compares the exit code, stdout and stderr with
//! `tests/golden/packages/<case>.txt`. To regenerate the golden files
//! after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test packages_golden` and review
//! the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &str = "crates/oskac/tests/fixtures/packages";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One case: the golden file's name, the directory to run in (relative
/// to the repository root, "" for the root), and the arguments.
struct Case {
    name: String,
    dir: String,
    args: Vec<String>,
}

fn case(name: &str, args: &[&str]) -> Case {
    Case {
        name: name.to_string(),
        dir: String::new(),
        args: args.iter().map(|a| a.to_string()).collect(),
    }
}

fn fixture(name: &str) -> String {
    format!("{FIXTURES}/{name}")
}

fn output_of(case: &Case) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(&case.args)
        .current_dir(repo_root().join(&case.dir))
        .output()
        .expect("oskac should run");
    format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        case.args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn assert_golden(case: &Case) {
    let actual = output_of(case);
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/packages")
        .join(format!("{}.txt", case.name));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
}

#[test]
fn a_valid_package_checks_runs_and_tests() {
    let valid = fixture("valid");
    for command in ["check", "run", "test"] {
        assert_golden(&case(&format!("valid_{command}"), &[command, &valid]));
    }
    assert_golden(&case(
        "valid_check_json",
        &["check", &valid, "--format=json"],
    ));
}

/// The authority report names each function with its module path, so
/// `main.write` and `util.log.write` are told apart, and marks `pub`
/// functions (#119).
#[test]
fn the_authority_report_names_functions_by_module_and_marks_pub() {
    let modules = fixture("modules");
    assert_golden(&case("modules_check", &["check", &modules]));
    assert_golden(&case(
        "modules_check_json",
        &["check", &modules, "--format=json"],
    ));
    assert_golden(&case("modules_run", &["run", &modules]));
}

#[test]
fn no_path_means_the_package_in_the_current_directory() {
    for command in ["check", "run", "test"] {
        let mut c = case(&format!("valid_no_path_{command}"), &[command]);
        c.dir = fixture("valid");
        assert_golden(&c);
    }
    let mut dot = case("valid_dot", &["check", "."]);
    dot.dir = fixture("valid");
    assert_golden(&dot);
}

#[test]
fn a_file_inside_a_package_is_checked_alone_with_a_hint() {
    let file = format!("{}/src/main.osk", fixture("valid"));
    for command in ["check", "run", "test"] {
        assert_golden(&case(&format!("valid_file_{command}"), &[command, &file]));
    }
}

#[test]
fn a_library_is_checked_and_tested_but_not_run() {
    let library = fixture("library");
    for command in ["check", "run", "test"] {
        assert_golden(&case(&format!("library_{command}"), &[command, &library]));
    }
}

#[test]
fn every_manifest_error_names_the_manifest_its_line_and_its_column() {
    for name in [
        "unknown_key",
        "unknown_key_no_fix",
        "unknown_table",
        "dependencies",
        "dotted_key",
        "date",
        "multiline_string",
        "inline_table",
        "missing_name",
        "bad_name",
        "bad_version",
        "syntax",
    ] {
        let dir = fixture(&format!("manifest_{name}"));
        assert_golden(&case(&format!("manifest_{name}"), &["check", &dir]));
    }
    let dir = fixture("manifest_unknown_key");
    assert_golden(&case(
        "manifest_unknown_key_json",
        &["check", &dir, "--format=json"],
    ));
}

#[test]
fn a_bad_name_a_misplaced_main_and_a_symlink_are_refused() {
    for name in ["bad_name", "main_elsewhere", "symlink"] {
        let dir = fixture(name);
        assert_golden(&case(name, &["check", &dir]));
    }
    let symlink = fixture("symlink");
    assert_golden(&case("symlink_run", &["run", &symlink]));
}

#[test]
fn a_directory_without_a_manifest_is_an_error_with_a_hint() {
    let dir = fixture("no_manifest");
    for command in ["check", "run", "test"] {
        assert_golden(&case(&format!("no_manifest_{command}"), &[command, &dir]));
    }
    assert_golden(&case("no_manifest_json", &["check", &dir, "--format=json"]));
}
