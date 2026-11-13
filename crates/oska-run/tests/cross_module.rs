//! A package's modules run as one program (spec #115, #118): the checker
//! joins them into one checked program, and the interpreter runs it.

mod common;

use oska_check::{check_package, CheckOptions, ModuleSource, Package};
use oska_run::{FakeHost, RunOutcome};

/// Check the package of `modules`, each a `(module path, source)` pair,
/// and run its checked program on a fresh [`FakeHost`].
fn run_modules(modules: &[(&str, &str)]) -> RunOutcome {
    let package = Package {
        manifest: None,
        modules: modules
            .iter()
            .map(|(module, source)| ModuleSource::in_package(module, source))
            .collect(),
    };
    let report = check_package(&package, CheckOptions::default());
    let program = report
        .program
        .unwrap_or_else(|| panic!("{modules:?} should check: {:#?}", report.diagnostics));
    common::run_program_differential(
        &program,
        &format!("{modules:#?}"),
        &mut FakeHost::new(),
        &[common::PROGRAM_PATH.to_string()],
    )
}

fn assert_prints(modules: &[(&str, &str)], expected: &str) {
    let outcome = run_modules(modules);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{:#?} {:#?}",
        outcome.diagnostics,
        outcome.panic
    );
    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(outcome.stdout, expected);
}

#[test]
fn calls_types_and_trait_impls_cross_modules_at_runtime() {
    let main = "use pkg.geo.shapes\nuse pkg.geo.shapes.{Area}\nuse pkg.report\n\n\
                fn main(out: Stdout) -> Result<(), IoError> {\n    \
                let xs = [shapes.Shape.Circle(1), shapes.Shape.Square(2)]\n    \
                out.println(report.describe(xs))\n}\n";
    let shapes = "pub(pkg) enum Shape {\n    Circle(Int),\n    Square(Int),\n}\n\n\
                  pub(pkg) trait Area {\n    fn area(self) -> Int\n}\n\n\
                  impl Area for Shape {\n    fn area(self) -> Int {\n        \
                  match self {\n            Shape.Circle(r) => 3 * r * r\n            \
                  Shape.Square(s) => s * s\n        }\n    }\n}\n";
    let report = "use pkg.geo.shapes.{Area, Shape}\n\n\
                  pub(pkg) fn describe(xs: List<Shape>) -> String {\n    \
                  let n = total(xs)\n    \"total {n}\"\n}\n\n\
                  fn total<T: Area>(xs: List<T>) -> Int {\n    var sum = 0\n    \
                  for x in xs {\n        sum = sum + x.area()\n    }\n    sum\n}\n";
    assert_prints(
        &[("main", main), ("geo.shapes", shapes), ("report", report)],
        "total 7\n",
    );
}

#[test]
fn a_panic_in_the_entry_module_spans_its_own_file_whatever_the_module_order() {
    let main = "use pkg.a\n\nfn main() {\n    let n = a.zero()\n    let q = 1 / n\n}\n";
    let outcome = run_modules(&[
        ("a", "pub(pkg) fn zero() -> Int {\n    0\n}\n"),
        ("main", main),
    ]);
    let panic = outcome.panic.expect("a panic");
    assert_eq!(&main[panic.span.start..panic.span.end], "1 / n");
}

#[test]
fn each_module_calls_its_own_private_item_of_a_shared_name() {
    let main = "use pkg.util\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    \
                let a = helper()\n    let b = util.call()\n    out.println(\"{a} {b}\")\n}\n\n\
                fn helper() -> String {\n    \"main\"\n}\n";
    let util = "pub(pkg) fn call() -> String {\n    helper()\n}\n\n\
                fn helper() -> String {\n    \"util\"\n}\n";
    assert_prints(&[("main", main), ("util", util)], "main util\n");
}
