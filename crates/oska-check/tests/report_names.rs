// SPDX-License-Identifier: Apache-2.0
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
    let functions: Vec<String> = report
        .attenuations
        .iter()
        .map(|a| a.function.to_string())
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

#[test]
fn impls_of_one_trait_for_two_instances_of_a_generic_type_are_named_apart() {
    let report = check(
        "trait Show {\n    fn show(self) -> String\n}\n\
         struct Pair<T> { a: T, b: T }\n\
         impl Show for Pair<Int> {\n    fn show(self) -> String { \"ints\" }\n}\n\
         impl Show for Pair<String> {\n    fn show(self) -> String { \"strings\" }\n}",
    );
    assert_eq!(
        names(&report),
        vec!["<Pair<Int> as Show>.show", "<Pair<String> as Show>.show"]
    );
}

#[test]
fn a_generic_impl_names_its_type_with_its_parameters() {
    let report = check(
        "trait Show {\n    fn show(self) -> String\n}\n\
         struct Pair<T> { a: T, b: T }\n\
         impl<T> Show for Pair<T> {\n    fn show(self) -> String { \"pair\" }\n}",
    );
    assert_eq!(names(&report), vec!["<Pair<T> as Show>.show"]);
}

#[test]
fn an_attenuation_in_an_instance_impl_names_it_as_the_report_does() {
    let report = check(
        "trait Publish {\n    fn out(self, write: FsWrite) -> DirWrite\n}\n\
         struct Site<T> { name: T }\n\
         impl Publish for Site<Int> {\n    fn out(self, write: FsWrite) -> DirWrite {\n        write.restrict_to(\"a\")\n    }\n}\n\
         impl Publish for Site<String> {\n    fn out(self, write: FsWrite) -> DirWrite {\n        write.restrict_to(\"b\")\n    }\n}",
    );
    let expected = vec![
        "<Site<Int> as Publish>.out",
        "<Site<String> as Publish>.out",
    ];
    assert_eq!(names(&report), expected);
    let functions: Vec<String> = report
        .attenuations
        .iter()
        .map(|a| a.function.to_string())
        .collect();
    assert_eq!(functions, expected);
}

#[test]
fn a_generic_trait_for_an_instance_names_both_arguments() {
    let report = check(
        "struct Code<T> { n: T }\n\
         impl From<Int> for Code<Int> {\n    fn from(n: Int) -> Code<Int> { Code(n: n) }\n}\n\
         impl From<Int> for Code<Bool> {\n    fn from(n: Int) -> Code<Bool> { Code(n: true) }\n}",
    );
    assert_eq!(
        names(&report),
        vec![
            "<Code<Int> as From<Int>>.from",
            "<Code<Bool> as From<Int>>.from"
        ]
    );
}

#[test]
fn in_a_package_the_instances_of_a_generic_type_are_named_by_keys() {
    let report = check_modules(&[
        (
            "main",
            "use pkg.shapes.{Pair}\nuse pkg.text.{Show}\n\
             impl Show for Pair<Int> {\n    fn show(self) -> String { \"ints\" }\n}\n\
             impl Show for Pair<String> {\n    fn show(self) -> String { \"strings\" }\n}\n\
             fn main() { }",
        ),
        ("shapes", "pub(pkg) struct Pair<T> { a: T, b: T }"),
        (
            "text",
            "pub(pkg) trait Show {\n    fn show(self) -> String\n}",
        ),
    ]);
    let mut found = names(&report);
    found.sort();
    assert_eq!(
        found,
        vec![
            "<pkg.shapes.Pair<Int> as pkg.text.Show>.show",
            "<pkg.shapes.Pair<String> as pkg.text.Show>.show",
            "pkg.main.main",
        ]
    );
}
