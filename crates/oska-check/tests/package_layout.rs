//! The layout rules of a package (spec #115, ticket #117): the manifest
//! is read and its errors are reported with its path, module and
//! directory names are snake_case, `main` is only in `src/main.osk`, and
//! a package without `src/main.osk` is a library.

mod common;

use common::{assert_modules_errors, assert_modules_ok, check_modules, path_of};
use oska_check::{check_package, CheckOptions, ModuleSource, Package};

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
const MAIN: &str = "fn main() {}\n";

fn package(manifest: &str, modules: &[(&str, &str)]) -> Package {
    Package {
        manifest: Some(manifest.to_string()),
        modules: modules
            .iter()
            .map(|(module, src)| ModuleSource::in_package(module, src))
            .collect(),
    }
}

#[test]
fn a_valid_manifest_adds_no_diagnostic() {
    let report = check_package(
        &package(MANIFEST, &[("main", MAIN)]),
        CheckOptions::default(),
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.program.is_some());
}

#[test]
fn manifest_errors_are_reported_with_the_manifests_path_before_the_modules() {
    let manifest = "[package]\nnaem = \"demo\"\nversion = \"0.1.0\"\n";
    let report = check_package(
        &package(manifest, &[("main", "fn main() {}\nfn f(x: Strin) {}\n")]),
        CheckOptions::default(),
    );
    let found: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (path_of(d.location.as_deref()), d.code))
        .collect();
    assert_eq!(
        found,
        [
            ("oska.toml", "E0402"),
            ("oska.toml", "E0404"),
            ("src/main.osk", "E0200")
        ]
    );
    assert!(
        report.program.is_none(),
        "an invalid manifest has no program"
    );
}

#[test]
fn a_file_name_that_is_not_snake_case_is_an_error_on_that_file() {
    assert_modules_errors(
        &[("main", MAIN), ("Bad_Name", "")],
        &[("src/Bad_Name.osk", "E0410", "")],
    );
}

#[test]
fn every_directory_and_file_name_of_a_module_path_is_checked() {
    let report = check_modules(&[("main", MAIN), ("good_dir.Worse", ""), ("my-dir.fine", "")]);
    let found: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (path_of(d.location.as_deref()), d.code))
        .collect();
    assert_eq!(
        found,
        [
            ("src/good_dir/Worse.osk", "E0410"),
            ("src/my-dir/fine.osk", "E0410")
        ]
    );
    assert!(
        report.diagnostics[1].message.contains("`my-dir`"),
        "{}",
        report.diagnostics[1].message
    );
}

#[test]
fn main_is_allowed_only_in_src_main_osk() {
    assert_modules_errors(
        &[("main", MAIN), ("tools.cli", "fn main() {}\n")],
        &[("src/tools/cli.osk", "E0412", "main")],
    );
}

#[test]
fn a_method_or_a_private_helper_named_main_elsewhere_is_not_a_main() {
    assert_modules_ok(&[
        ("main", MAIN),
        (
            "other",
            "struct Runner {\n    n: Int\n}\nimpl Runner {\n    fn main(self) -> Int {\n        self.n\n    }\n}\n",
        ),
    ]);
}

#[test]
fn a_single_file_may_have_main_wherever_it_is() {
    let report = oska_check::check_file("tools/cli.osk", MAIN, CheckOptions::default());
    assert!(report.diagnostics.is_empty());
}

#[test]
fn a_package_without_a_main_module_is_a_library() {
    let library = package(MANIFEST, &[("greeting", "fn greet() -> Int {\n    1\n}\n")]);
    assert!(library.is_library());
    let program = package(MANIFEST, &[("main", MAIN), ("greeting", "")]);
    assert!(!program.is_library());
    let report = check_package(&library, CheckOptions::default());
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report.program.is_some(),
        "a one-module library has a program"
    );
}
