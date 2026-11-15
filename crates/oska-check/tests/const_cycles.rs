//! A module constant that refers to itself, directly or through other
//! constants, is a checker error at its initializer (#278, spec #184):
//! each constant lowers to a function its uses call (#83), so a cycle
//! would call itself until the stack overflowed. No program is produced
//! for one, so neither backend sees it.

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check, check_modules};

fn messages(report: &oska_check::CheckReport) -> Vec<&str> {
    report
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect()
}

#[test]
fn a_constant_that_refers_to_itself_is_an_error_at_its_initializer() {
    assert_errors("const A: Int = A", &[("E0236", "A")]);
}

#[test]
fn the_message_names_the_cycle() {
    let report = check("const A: Int = A");
    assert_eq!(
        messages(&report),
        ["constant `A` depends on itself: A -> A"]
    );
}

#[test]
fn a_constant_that_refers_to_itself_inside_an_expression_is_an_error() {
    assert_errors("const A: Int = 1 + A * 2", &[("E0236", "1 + A * 2")]);
}

#[test]
fn two_constants_that_refer_to_each_other_are_both_reported() {
    let src = "const A: Int = B\nconst B: Int = A";
    assert_errors(src, &[("E0236", "B"), ("E0236", "A")]);
    assert_eq!(
        messages(&check(src)),
        [
            "constant `A` depends on itself: A -> B -> A",
            "constant `B` depends on itself: B -> A -> B",
        ]
    );
}

#[test]
fn a_longer_cycle_is_named_in_full_and_a_constant_outside_it_is_not_reported() {
    let src = "const A: Int = B\nconst B: Int = C\nconst C: Int = A\nconst D: Int = A";
    assert_eq!(
        messages(&check(src)),
        [
            "constant `A` depends on itself: A -> B -> C -> A",
            "constant `B` depends on itself: B -> C -> A -> B",
            "constant `C` depends on itself: C -> A -> B -> C",
        ]
    );
}

#[test]
fn a_program_with_a_cycle_has_no_checked_program() {
    let report = check("const A: Int = A\nfn a() -> Int {\n    A\n}");
    assert!(report.program.is_none());
}

#[test]
fn constants_that_refer_to_other_constants_without_a_cycle_are_fine() {
    assert_ok(
        "const BASE: Int = 10\nconst NEXT: Int = BASE + 1\nconst AFTER: Int = NEXT + BASE\n\
         const LATE: Int = EARLY\nconst EARLY: Int = 3\n\
         fn after() -> Int {\n    AFTER + LATE\n}",
    );
}

#[test]
fn a_cycle_across_modules_is_reported_in_each_module_naming_the_modules() {
    // Constants reach each other across modules only through `use`, so
    // the import cycle (E0207) is reported too.
    let report = check_modules(&[
        (
            "main",
            "use pkg.util.{B}\n\nfn main() {}\n\npub(pkg) const A: Int = B\n",
        ),
        ("util", "use pkg.main.{A}\n\npub(pkg) const B: Int = A\n"),
    ]);
    let found: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (common::path_of(d.location.as_deref()), d.code))
        .filter(|(_, code)| *code == "E0236")
        .collect();
    assert_eq!(
        found,
        [("src/main.osk", "E0236"), ("src/util.osk", "E0236")]
    );
    let cycles: Vec<&str> = messages(&report)
        .into_iter()
        .filter(|m| m.starts_with("constant"))
        .collect();
    assert_eq!(
        cycles,
        [
            "constant `A` depends on itself: A -> util.B -> A",
            "constant `B` depends on itself: B -> main.A -> B",
        ]
    );
    assert!(report.program.is_none());
}

#[test]
fn constants_across_modules_without_a_cycle_are_fine() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util.{BASE}\n\nfn main() {}\n\nconst NEXT: Int = BASE + 1\n",
        ),
        ("util", "pub(pkg) const BASE: Int = 1\n"),
    ]);
}
