//! Fixes from the review of spec #115 (packages), at seam 1: packages
//! checked in memory. Each test is an attack or a case the review found.

mod common;

use common::{assert_modules_errors, assert_modules_ok, check, check_modules, path_of};
use oska_check::{check_package, CheckOptions, ModuleSource, Package};

const MAIN: &str = "fn main() {}\n";

/// The `E0410` diagnostics of the package of `modules`, by file.
fn bad_names(modules: &[(&str, &str)]) -> Vec<String> {
    check_modules(modules)
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0410")
        .map(|d| path_of(d.location.as_deref()).to_string())
        .collect()
}

#[test]
fn a_module_named_by_a_keyword_cannot_be_imported_so_it_is_refused() {
    let found = bad_names(&[
        ("main", MAIN),
        ("fn", "fn f() -> Int {\n    1\n}\n"),
        ("match.use", "fn g() -> Int {\n    1\n}\n"),
    ]);
    assert_eq!(found, ["src/fn.osk", "src/match/use.osk"]);
}

#[test]
fn pkg_std_and_main_are_reserved_as_the_first_name_of_a_module() {
    let found = bad_names(&[
        ("main", MAIN),
        ("pkg", "fn g() -> Int {\n    1\n}\n"),
        ("std", "fn f() -> Int {\n    1\n}\n"),
        ("main.x", "fn h() -> Int {\n    1\n}\n"),
        ("tools.std", "fn k() -> Int {\n    1\n}\n"),
    ]);
    assert_eq!(
        found,
        ["src/pkg.osk", "src/std.osk", "src/main/x.osk"],
        "only the first name is reserved: `tools.std` is fine"
    );
    let report = check_modules(&[("main", MAIN), ("pkg", "")]);
    let message = &report.diagnostics[0].message;
    assert!(message.contains("reserved"), "{message}");
}

#[test]
fn a_hidden_file_is_refused_even_when_its_text_does_not_parse() {
    // `oskac` names a module by its file: `src/.scratch.osk` is `.scratch`.
    let package = Package {
        manifest: None,
        modules: vec![
            ModuleSource::in_package("main", MAIN),
            ModuleSource {
                module: ".scratch".to_string(),
                path: "src/.scratch.osk".to_string(),
                source: "fn {\n".to_string(),
            },
        ],
    };
    let report = check_package(&package, CheckOptions::default());
    let found: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (path_of(d.location.as_deref()), d.code))
        .collect();
    assert_eq!(
        found,
        [("src/.scratch.osk", "E0410"), ("src/.scratch.osk", "E0101")]
    );
}

// ---- Imports are per module: splitting a file changes nothing ------------------

#[test]
fn an_entry_module_name_does_not_clash_with_another_modules_std_import_by_name() {
    let util = "use std.html.{text, Html}\n\npub(pkg) fn page(name: Untrusted<String>) -> Html {\n    text(name)\n}\n";
    let main = "use pkg.util\n\nfn text(s: Untrusted<String>) -> Int {\n    1\n}\n\nfn twice(s: Untrusted<String>) -> Int {\n    text(s) + text(s)\n}\n\nfn main() {}\n";
    assert_modules_ok(&[("main", main), ("util", util)]);
}

#[test]
fn an_entry_module_name_does_not_clash_with_another_modules_std_module_import() {
    let util = "use std.html\n\npub(pkg) fn page(name: Untrusted<String>) -> html.Html {\n    html.text(name)\n}\n";
    let main = "use pkg.util\n\nfn html() -> Int {\n    1\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    let n = html()\n    out.println(\"{n}\")\n}\n";
    assert_modules_ok(&[("main", main), ("util", util)]);
}

#[test]
fn the_entry_modules_items_are_not_seen_by_another_module_that_does_not_import_them() {
    let main = "fn helper() -> Int {\n    1\n}\n\nfn main() {}\n";
    let util = "fn f() -> Int {\n    helper()\n}\n";
    assert_modules_errors(
        &[("main", main), ("util", util)],
        &[("src/util.osk", "E0221", "helper")],
    );
}

#[test]
fn an_entry_module_item_named_like_a_prelude_item_is_still_a_clash() {
    let main = "fn todo() -> Int {\n    1\n}\n\nfn main() {}\n";
    let single = check(main);
    assert_eq!(
        single
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        ["E0202"],
        "a single file: {:#?}",
        single.diagnostics
    );
    assert_modules_errors(
        &[("main", main), ("util", "")],
        &[("src/main.osk", "E0202", "todo")],
    );
}

// ---- The authority report: unique names, never confused with std ---------------

/// The report names of the package of `modules`, asserting no two are
/// the same.
fn report_names(modules: &[(&str, &str)]) -> Vec<String> {
    let report = check_modules(modules);
    let names: Vec<String> = report.functions.iter().map(|f| f.name.clone()).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "a name is reported twice: {names:?}"
    );
    names
}

#[test]
fn a_method_is_named_by_its_types_module_wherever_its_impl_is() {
    let main = "use pkg.util\n\nimpl util.Box {\n    fn leak(self, fs: Fs) -> Int {\n        1\n    }\n}\n\nfn main(fs: Fs) {\n    let n = util.make().leak(fs)\n}\n";
    let util = "pub(pkg) struct Box {\n    pub v: Int\n}\n\npub(pkg) fn make() -> Box {\n    Box(v: 1)\n}\n";
    let tools = "struct Box {\n    pub v: Int\n}\n\nimpl Box {\n    fn leak(self) -> Int {\n        2\n    }\n}\n";
    let modules = [("main", main), ("util", util), ("tools.util", tools)];
    let names = report_names(&modules);
    assert_eq!(
        names,
        [
            "pkg.util.Box.leak",
            "pkg.main.main",
            "pkg.util.make",
            "pkg.tools.util.Box.leak"
        ]
    );
    let report = check_modules(&modules);
    assert_eq!(
        report.functions[0].authority,
        ["Fs"],
        "{:#?}",
        report.functions
    );
}

#[test]
fn a_package_function_is_named_with_pkg_so_it_never_reads_as_std() {
    let html = "pub(pkg) fn text(fs: Fs) -> Int {\n    1\n}\n";
    let names = report_names(&[("main", MAIN), ("html", html)]);
    assert_eq!(names, ["pkg.main.main", "pkg.html.text"]);
}

#[test]
fn no_two_entries_of_the_report_share_a_name() {
    let main = "use pkg.util\n\nfn util_f() -> Int {\n    1\n}\n\nimpl util.Box {\n    fn f(self) -> Int {\n        1\n    }\n}\n\nfn main() {}\n";
    let util = "pub(pkg) struct Box {\n    pub v: Int\n}\n\nfn f() -> Int {\n    1\n}\n\ntrait Named {\n    fn f(self) -> Int {\n        1\n    }\n}\n";
    let inner = "struct Box {\n    pub v: Int\n}\n\nimpl Box {\n    fn g(self) -> Int {\n        1\n    }\n}\n";
    report_names(&[("main", main), ("util", util), ("util.box", inner)]);
}

// ---- Internal names never reach a message --------------------------------------

/// Every diagnostic message and fix message of the package of `modules`.
fn messages(modules: &[(&str, &str)]) -> Vec<String> {
    check_modules(modules)
        .diagnostics
        .iter()
        .flat_map(|d| {
            std::iter::once(d.message.clone()).chain(d.fix.iter().map(|f| f.message.clone()))
        })
        .collect()
}

#[test]
fn a_type_of_another_module_is_named_as_code_names_it() {
    let main = "use pkg.util\n\nfn main() {\n    let b: Int = util.make()\n}\n";
    let util = "pub(pkg) struct Box {\n    pub v: Int\n}\n\npub(pkg) fn make() -> Box {\n    Box(v: 1)\n}\n";
    assert_eq!(
        messages(&[("main", main), ("util", util)]),
        ["expected `Int`, found `util.Box`"]
    );
}

#[test]
fn an_item_of_the_diagnostics_own_module_is_named_plainly() {
    let util = "fn f() -> Int {\n    1\n}\n\nfn f() -> Int {\n    2\n}\n";
    let main = "fn helper() -> Int {\n    1\n}\n\nfn helper() -> Int {\n    2\n}\n\nfn main() {}\n";
    let found = messages(&[("main", main), ("a.b.c", util)]);
    assert_eq!(found.len(), 2, "{found:#?}");
    for message in &found {
        assert!(!message.contains("pkg."), "{message}");
    }
    assert!(found[0].contains("`helper`"), "{}", found[0]);
    assert!(found[1].contains("`f`"), "{}", found[1]);
}

#[test]
fn a_pkg_path_written_as_a_type_is_reported_as_written() {
    let main = "use pkg.util\n\nfn peek(h: pkg.util.Hidden) -> Int {\n    0\n}\n\nfn main() {}\n";
    let util = "struct Hidden {\n    v: Int\n}\n";
    let modules = [("main", main), ("util", util)];
    assert_modules_errors(&modules, &[("src/main.osk", "E0200", "pkg.util.Hidden")]);
    let found = messages(&modules);
    assert!(
        found[0].contains("`pkg.util.Hidden`") && found[0].contains("`util.Hidden`"),
        "{}",
        found[0]
    );
}
