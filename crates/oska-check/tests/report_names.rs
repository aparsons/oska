//! Every function in the authority report has its own name (#123): a
//! method of `impl Trait for Type` is named `<Type as Trait>.method`, as
//! the checked program names it, so it never shares a name with an
//! inherent method of the same type.

mod common;

use common::{check, check_modules};
use oska_check::CheckReport;

fn names(report: &CheckReport) -> Vec<&str> {
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got {:?}",
        report.diagnostics
    );
    report.functions.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn a_trait_method_and_an_inherent_method_with_one_name_are_named_apart() {
    let report = check(
        "trait Describe {\n    fn describe(self) -> String\n}\n\
         struct Point { x: Int }\n\
         impl Point {\n    fn describe(self) -> String { \"inherent\" }\n}\n\
         impl Describe for Point {\n    fn describe(self) -> String { \"trait\" }\n}",
    );
    assert_eq!(
        names(&report),
        vec!["Point.describe", "<Point as Describe>.describe"]
    );
}

#[test]
fn two_impls_of_one_generic_trait_are_named_by_its_arguments() {
    let report = check(
        "struct Code { n: Int }\n\
         impl From<Int> for Code {\n    fn from(n: Int) -> Code { Code(n: n) }\n}\n\
         impl From<Bool> for Code {\n    fn from(b: Bool) -> Code { Code(n: 0) }\n}",
    );
    assert_eq!(
        names(&report),
        vec!["<Code as From<Int>>.from", "<Code as From<Bool>>.from"]
    );
}

#[test]
fn an_attenuation_in_a_trait_method_names_it_as_the_report_does() {
    let report = check(
        "trait Publish {\n    fn out(self, write: FsWrite) -> DirWrite\n}\n\
         struct Site { name: String }\n\
         impl Site {\n    fn out(self, write: FsWrite) -> DirWrite {\n        write.restrict_to(\"a\")\n    }\n}\n\
         impl Publish for Site {\n    fn out(self, write: FsWrite) -> DirWrite {\n        write.restrict_to(\"b\")\n    }\n}",
    );
    assert_eq!(names(&report), vec!["Site.out", "<Site as Publish>.out"]);
    let functions: Vec<&str> = report
        .attenuations
        .iter()
        .map(|a| a.function.as_str())
        .collect();
    assert_eq!(functions, vec!["Site.out", "<Site as Publish>.out"]);
}

#[test]
fn in_a_package_a_trait_method_is_named_by_the_keys_of_its_type_and_trait() {
    let report = check_modules(&[
        (
            "main",
            "use pkg.shapes.{Point}\nuse pkg.text.{Describe}\n\
             impl Describe for Point {\n    fn describe(self) -> String { \"trait\" }\n}\n\
             fn main() { }",
        ),
        (
            "shapes",
            "pub(pkg) struct Point { x: Int }\n\
             impl Point {\n    pub(pkg) fn describe(self) -> String { \"inherent\" }\n}",
        ),
        (
            "text",
            "pub(pkg) trait Describe {\n    fn describe(self) -> String\n}",
        ),
    ]);
    let mut found = names(&report);
    found.sort();
    assert_eq!(
        found,
        vec![
            "<pkg.shapes.Point as pkg.text.Describe>.describe",
            "pkg.main.main",
            "pkg.shapes.Point.describe",
        ]
    );
}
