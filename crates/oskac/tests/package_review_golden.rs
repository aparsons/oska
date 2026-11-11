//! Fixes from the review of spec #115 (packages), at seam 2: `oskac`
//! over fixture packages on disk, with golden output under
//! `tests/golden/packages/review/`. Each case also asserts the line that
//! matters, so a regenerated golden cannot quietly hide a regression.
//! Regenerate with `UPDATE_GOLDEN=1 cargo test -p oskac --test
//! package_review_golden` and review the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &str = "crates/oskac/tests/fixtures/packages";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Run `oskac args` in `dir`, relative to the repository root, and
/// compare the transcript with the golden file `name`. Returns the
/// transcript.
fn golden(name: &str, dir: &str, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(repo_root().join(dir))
        .output()
        .expect("oskac should run");
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/packages/review")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&path, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", path.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        path.display()
    );
    actual
}

fn fixture(name: &str) -> String {
    format!("{FIXTURES}/{name}")
}

fn assert_has(transcript: &str, line: &str) {
    assert!(
        transcript.contains(line),
        "expected {line:?} in\n{transcript}"
    );
}

#[test]
fn only_src_main_osk_is_the_entry_and_src_inside_src_is_a_module() {
    let nested = fixture("nested_src");
    let out = golden("nested_src_check", "", &["check", &nested]);
    assert_has(&out, "exit: 0");
    assert_has(&out, "fn pkg.src.util.f: pure\nfn pkg.util.f: pure");
    let out = golden("nested_src_run", "", &["run", &nested]);
    assert_has(&out, "--- stdout\n2\n");

    let entry = fixture("nested_src_main");
    let out = golden("nested_src_main_check", "", &["check", &entry]);
    assert_has(&out, "src/src/main.osk:1:4: error[E0412]");
    let out = golden("nested_src_main_run", "", &["run", &entry]);
    assert_has(&out, "error[E0414]");
}

#[test]
fn runtime_panics_and_test_failures_never_show_internal_names() {
    let dir = fixture("runtime_names");
    let run = golden("runtime_names_run", "", &["run", &dir]);
    assert_has(&run, "precondition violated: `a.b.c.half` requires `n > 0`");
    let test = golden("runtime_names_test", "", &["test", &dir]);
    assert_has(&test, "precondition violated: `half` requires `n > 0`");
    for out in [&run, &test] {
        assert!(!out.contains("pkg."), "{out}");
    }
}

#[test]
fn a_tests_module_is_its_own_field_and_names_have_no_double_colon() {
    let dir = fixture("runtime_names");
    let human = golden("runtime_names_test", "", &["test", &dir]);
    assert_has(&human, "test a.b.c: halves ... FAILED\n");
    assert_has(&human, "test main: main test ... ok\n");
    let json = golden(
        "runtime_names_test_json",
        "",
        &["test", &dir, "--format=json"],
    );
    assert_has(
        &json,
        r#"{"name":"halves","module":"a.b.c","status":"failed""#,
    );
    assert!(
        !human.contains("::") && !json.contains("::"),
        "{human}{json}"
    );
}

#[test]
fn hidden_keyword_and_reserved_module_names_are_refused() {
    let out = golden("hidden_check", "", &["check", &fixture("hidden")]);
    assert_has(&out, "hidden/src/.scratch.osk:1:1: error[E0410]");
    let out = golden("keyword_check", "", &["check", &fixture("keyword")]);
    assert_has(&out, "keyword/src/fn.osk:1:1: error[E0410]");
    assert_has(&out, "keyword/src/std.osk:1:1: error[E0410]");
}

#[test]
fn a_symlinked_manifest_is_refused() {
    let out = golden(
        "manifest_symlink_check",
        "",
        &["check", &fixture("manifest_symlink")],
    );
    assert_has(&out, "manifest_symlink/oska.toml:1:1: error[E0411]");
}

#[test]
fn a_manifest_that_is_not_utf8_is_a_coded_error_at_its_place() {
    let out = golden(
        "manifest_not_utf8_check",
        "",
        &["check", &fixture("manifest_not_utf8")],
    );
    assert_has(&out, "manifest_not_utf8/oska.toml:4:6: error[E0400]");
}

#[test]
fn a_package_with_no_modules_is_an_error() {
    for command in ["check", "run", "test"] {
        let out = golden(
            &format!("empty_src_{command}"),
            "",
            &[command, &fixture("empty_src")],
        );
        assert_has(&out, "error[E0413]");
    }
}

#[test]
fn human_paths_start_from_the_current_directory_and_json_paths_from_the_package() {
    let out = golden("paths_human", FIXTURES, &["check", "hidden/"]);
    assert_has(&out, "\nhidden/src/.scratch.osk:1:1: error[E0410]");
    let out = golden(
        "paths_json",
        FIXTURES,
        &["check", "hidden", "--format=json"],
    );
    assert_has(&out, r#""path":"src/.scratch.osk""#);
    let out = golden("paths_dot", &fixture("hidden"), &["check", "."]);
    assert_has(&out, "\nsrc/.scratch.osk:1:1: error[E0410]");
}

#[test]
fn check_inside_a_packages_subdirectory_names_the_package_above() {
    let out = golden("subdirectory_check", &fixture("valid/src"), &["check"]);
    assert_has(&out, "error[E0413]");
    assert_has(&out, "`oskac check ..`");
}
