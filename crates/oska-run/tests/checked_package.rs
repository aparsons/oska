//! A checked package runs through `run_program` and `test_program`, the
//! entries `oskac run` and `oskac test` use on a package (#117).

use oska_check::{check_package, CheckOptions, ModuleSource, Package};
use oska_run::{run_program, test_program, FakeHost, TestOptions, TestStatus};

const MAIN: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello from a package\")\n}\n\ntest \"adds\" {\n    t.assert(1 + 1 == 2)\n}\n";

fn checked() -> oska_check::program::Program {
    let package = Package {
        manifest: Some("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n".to_string()),
        modules: vec![ModuleSource::in_package("main", MAIN)],
    };
    let report = check_package(&package, CheckOptions::default());
    report.program.expect("the package checks")
}

#[test]
fn a_checked_program_runs_without_its_source() {
    let outcome = run_program(&checked(), &mut FakeHost::new(), &["demo".to_string()]);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "hello from a package\n");
}

#[test]
fn a_checked_programs_tests_run() {
    let report = test_program(&checked(), &mut FakeHost::new(), TestOptions::default());
    assert!(report.diagnostics.is_empty());
    let statuses: Vec<(&str, TestStatus)> = report
        .tests
        .iter()
        .map(|t| (t.name.as_str(), t.status))
        .collect();
    assert_eq!(statuses, [("adds", TestStatus::Passed)]);
}
