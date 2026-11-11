//! A package is checked as one program of several modules (spec #115).
//! Until cross-module `use` exists, its modules are independent: each is
//! checked as it would be alone, and every report entry names its file.

mod common;

use common::{
    assert_modules_errors, assert_modules_fix, assert_modules_ok, check_modules, path_of,
};

#[test]
fn each_diagnostic_names_the_file_it_is_in() {
    assert_modules_errors(
        &[
            (
                "main",
                "fn main() {}\nfn helper(n: Strin) -> Int {\n    1\n}\n",
            ),
            ("util.text", "fn shout(s: String) -> Strng {\n    s\n}\n"),
        ],
        &[
            ("src/main.osk", "E0200", "Strin"),
            ("src/util/text.osk", "E0200", "Strng"),
        ],
    );
}

#[test]
fn a_fix_edits_the_file_its_diagnostic_is_in() {
    assert_modules_fix(
        &[
            ("main", "fn main() {}\n"),
            ("util.text", "fn shout(s: String) -> Strng {\n    s\n}\n"),
        ],
        "src/util/text.osk",
        "E0200",
        "fn shout(s: String) -> String {\n    s\n}\n",
    );
}

#[test]
fn modules_are_independent_so_each_may_declare_the_same_name() {
    assert_modules_ok(&[
        ("main", "fn main() {}\nfn helper() -> Int {\n    1\n}\n"),
        ("a", "fn helper() -> Int {\n    2\n}\n"),
        ("a.b", "struct Helper {\n    n: Int\n}\n"),
    ]);
}

#[test]
fn audit_sites_and_attenuations_name_their_files_with_spans_in_that_file() {
    let main = "fn main() {}\n";
    let trust = "fn vouch(name: Untrusted<String>) -> String {\n    name.assume_trusted()\n}\n";
    let fs = "fn docs(read: FsRead) -> DirRead {\n    read.restrict_to(\"docs\")\n}\n";
    let report = check_modules(&[("main", main), ("trust", trust), ("io.fs", fs)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let audit: Vec<(&str, &str)> = report
        .audit
        .iter()
        .map(|site| {
            (
                path_of(site.location.as_ref()),
                &trust[site.span.start..site.span.end],
            )
        })
        .collect();
    assert_eq!(audit, [("src/trust.osk", "name.assume_trusted()")]);
    let attenuations: Vec<(&str, &str)> = report
        .attenuations
        .iter()
        .map(|a| (path_of(a.location.as_ref()), &fs[a.span.start..a.span.end]))
        .collect();
    assert_eq!(
        attenuations,
        [("src/io/fs.osk", "read.restrict_to(\"docs\")")]
    );
}

#[test]
fn the_json_report_carries_each_entry_s_path_before_its_span() {
    let fs = "fn docs(read: FsRead) -> DirRead {\n    read.restrict_to(\"docs\")\n}\n\
              fn vouch(name: Untrusted<String>) -> String {\n    name.assume_trusted()\n}\n";
    let json =
        check_modules(&[("main", "fn main() {}\nfn f() -> Strng {}\n"), ("io", fs)]).to_json();
    for expected in [
        r#"{"code":"E0200","message":"unknown type `Strng`; did you mean `String`?","path":"src/main.osk","span":"#,
        r#"{"kind":"assume_trusted","path":"src/io.osk","span":"#,
        r#"{"function":"docs","method":"restrict_to","path":"src/io.osk","span":"#,
    ] {
        assert!(json.contains(expected), "no {expected} in {json}");
    }
}

#[test]
fn the_human_report_starts_audit_and_attenuation_lines_with_path_line_and_column() {
    let fs = "fn docs(read: FsRead) -> DirRead {\n    read.restrict_to(\"docs\")\n}\n\
              fn vouch(name: Untrusted<String>) -> String {\n    name.assume_trusted()\n}\n";
    let human =
        check_modules(&[("main", "fn main() {}\n"), ("io", fs)]).authority_to_human("ignored");
    assert!(
        human.contains(
            "\n  src/io.osk:2:5: FsRead -> DirRead: restrict_to \"docs\" (bytes 39..63)\n"
        ),
        "{human}"
    );
    assert!(
        human.ends_with("src/io.osk:5:5: audit: assume_trusted (bytes 116..137)\n"),
        "{human}"
    );
}

#[test]
fn a_single_file_is_a_package_of_one_module_named_by_the_path_given() {
    let src = "fn main() {}\nfn f(x: Strng) -> Int {\n    1\n}\n";
    let report = oska_check::check_file("dir/prog.osk", src, oska_check::CheckOptions::default());
    let location = report.diagnostics[0].location.clone();
    assert_eq!(
        location.map(|l| l.to_string()),
        Some("dir/prog.osk:2:9".to_string())
    );
    let alone = oska_check::check_module(src);
    let unlocated: Vec<_> = report
        .diagnostics
        .into_iter()
        .map(|mut d| {
            d.location = None;
            d
        })
        .collect();
    assert_eq!(unlocated, alone.diagnostics);
}

#[test]
fn a_clean_single_file_carries_its_checked_program() {
    let report = oska_check::check_file(
        "prog.osk",
        "fn main() {}\n",
        oska_check::CheckOptions::default(),
    );
    assert!(report.program.is_some());
}
