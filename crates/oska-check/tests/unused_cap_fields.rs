// SPDX-License-Identifier: Apache-2.0
//! Unused authority in `cap struct` fields (ticket #369, ADR 0016): the
//! release check rejects a capability-bearing field of a `cap struct` that
//! no code in the program reads, with `E0297`. Any read counts: a field
//! access, a destructuring `match`, passing the field on. An ordinary
//! check says nothing.

mod common;

use common::assert_ok;
use oska_check::{
    check_file, check_module_with, check_package, CheckOptions, ModuleSource, Package,
};
use oska_syntax::Applicability;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `(code, spanned text)` for every diagnostic of the release check.
fn release_errors(src: &str) -> Vec<(&'static str, &str)> {
    check_module_with(src, CheckOptions { release: true })
        .diagnostics
        .iter()
        .map(|d| (d.code, &src[d.span.start..d.span.end]))
        .collect()
}

/// Assert that the release check accepts `src`.
fn assert_release_ok(src: &str) {
    assert_eq!(release_errors(src), [], "{src}");
}

/// A `cap struct` with two capability fields, and `main` building one.
/// `body` is the rest of the program, after the struct.
fn program(body: &str) -> String {
    format!("cap struct Tools {{\n    out: Stdout,\n    clock: Clock,\n}}\n\n{body}")
}

// ---- Release check only -------------------------------------------------------------

#[test]
fn an_unread_capability_field_is_unused_authority_in_the_release_check() {
    let src = program(
        "fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.out.println(\"hi\")\n}",
    );
    assert_eq!(release_errors(&src), [("E0297", "clock: Clock")]);
}

#[test]
fn an_ordinary_check_does_not_report_an_unread_field() {
    assert_ok(&program(
        "fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.out.println(\"hi\")\n}",
    ));
}

#[test]
fn a_cap_struct_whose_every_capability_field_is_read_passes() {
    assert_release_ok(&program(
        "fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.out.println(\"hi\")\n    let _ = tools.clock.now_millis()\n}",
    ));
}

// ---- Any read anywhere in the program counts ----------------------------------------

/// `main` builds `Tools` and hands it to `work`; `work` is the rest.
fn handed_on(work: &str) -> String {
    program(&format!(
        "{work}\n\nfn main(out: Stdout, clock: Clock) {{\n    \
         work(Tools(out: out, clock: clock))\n}}"
    ))
}

#[test]
fn passing_a_field_on_is_a_read() {
    assert_release_ok(&handed_on(
        "fn tick(clock: Clock) -> Int {\n    clock.now_millis()\n}\n\n\
         fn work(tools: Tools) {\n    let _ = tools.out.println(\"hi\")\n    \
         let _ = tick(tools.clock)\n}",
    ));
}

#[test]
fn destructuring_a_field_is_a_read() {
    assert_release_ok(&handed_on(
        "fn work(tools: Tools) {\n    match tools {\n        \
         Tools(out: o, clock: c) => {\n            let n = c.now_millis()\n            \
         let _ = o.println(\"{n}\")\n        \
         }\n    }\n}",
    ));
}

#[test]
fn a_field_discarded_with_a_wildcard_is_not_read() {
    assert_eq!(
        release_errors(&handed_on(
            "fn work(tools: Tools) {\n    match tools {\n        \
             Tools(out: o, clock: _) => {\n            let _ = o.println(\"hi\")\n        \
             }\n    }\n}",
        )),
        [("E0297", "clock: Clock")]
    );
}

#[test]
fn discarding_a_field_with_let_underscore_is_not_a_read() {
    assert_eq!(
        release_errors(&handed_on(
            "fn work(tools: Tools) {\n    let _ = tools.out.println(\"hi\")\n    \
             let _ = tools.clock\n}",
        )),
        [("E0297", "clock: Clock")]
    );
}

#[test]
fn a_pattern_binding_that_is_never_used_is_not_a_read() {
    assert_eq!(
        release_errors(&handed_on(
            "fn work(tools: Tools) {\n    match tools {\n        \
             Tools(out: o, clock: _c) => {\n            let _ = o.println(\"hi\")\n        \
             }\n    }\n}",
        )),
        [("E0297", "clock: Clock")]
    );
}

#[test]
fn a_leading_underscore_does_not_silence_an_unread_field() {
    assert_eq!(
        release_errors(
            "cap struct Tools {\n    out: Stdout,\n    _clock: Clock,\n}\n\n\
             fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, _clock: clock)\n    \
             let _ = tools.out.println(\"hi\")\n}"
        ),
        [("E0297", "_clock: Clock")]
    );
}

#[test]
fn a_read_in_a_method_through_self_counts() {
    assert_release_ok(&handed_on(
        "impl Tools {\n    fn stamp(self) -> Int {\n        self.clock.now_millis()\n    }\n}\n\n\
         fn work(tools: Tools) {\n    let n = tools.stamp()\n    let _ = tools.out.println(\"{n}\")\n}",
    ));
}

#[test]
fn a_read_in_a_function_nothing_calls_counts() {
    assert_release_ok(&handed_on(
        "fn work(tools: Tools) {\n    let _ = tools.out.println(\"hi\")\n}\n\n\
         fn stamp(tools: Tools) -> Int {\n    tools.clock.now_millis()\n}",
    ));
}

#[test]
fn a_read_in_another_module_of_the_package_counts() {
    let package = Package {
        manifest: None,
        modules: vec![
            ModuleSource::in_package(
                "main",
                "use pkg.tools.{Tools}\n\n\
                 fn main(out: Stdout, clock: Clock) {\n    \
                 let tools = Tools(out: out, clock: clock)\n    \
                 let n = tools.clock.now_millis()\n    \
                 let _ = tools.out.println(\"{n}\")\n}\n",
            ),
            ModuleSource::in_package(
                "tools",
                "pub(pkg) cap struct Tools {\n    pub out: Stdout,\n    pub clock: Clock,\n}\n",
            ),
        ],
    };
    let report = check_package(&package, CheckOptions { release: true });
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

// ---- What is judged -----------------------------------------------------------------

#[test]
fn a_field_that_is_not_capability_bearing_is_not_judged() {
    assert_release_ok(
        "cap struct Tagged {\n    out: Stdout,\n    tag: String,\n}\n\n\
         fn main(out: Stdout) {\n    let t = Tagged(out: out, tag: \"x\")\n    \
         let _ = t.out.println(\"hi\")\n}",
    );
}

#[test]
fn a_field_typed_by_the_structs_type_parameter_is_not_judged() {
    assert_release_ok(
        "cap struct Holder<T> {\n    out: Stdout,\n    value: T,\n}\n\n\
         fn main(out: Stdout, clock: Clock) {\n    let h = Holder(out: out, value: clock)\n    \
         let _ = h.out.println(\"hi\")\n}",
    );
}

#[test]
fn an_unread_field_typed_by_a_bounded_type_parameter_is_unused_authority() {
    assert_eq!(
        release_errors(
            "trait Tick {\n    fn tick(self) -> Int\n}\n\n\
             cap struct Holder<T: Tick> {\n    out: Stdout,\n    value: T,\n}\n\n\
             fn say<T: Tick>(h: Holder<T>) {\n    let _ = h.out.println(\"hi\")\n}\n\n\
             fn main() {}"
        ),
        [("E0297", "value: T")]
    );
}

#[test]
fn an_unread_field_holding_a_cap_struct_is_unused_authority() {
    assert_eq!(
        release_errors(&handed_on(
            "cap struct Outer {\n    tools: Tools,\n}\n\n\
             fn work(tools: Tools) {\n    let n = tools.clock.now_millis()\n    \
             let _ = tools.out.println(\"{n}\")\n    let _ = Outer(tools: tools)\n}",
        )),
        [("E0297", "tools: Tools")]
    );
}

#[test]
fn an_unread_positional_field_is_unused_authority() {
    assert_eq!(
        release_errors(
            "cap struct Pair(Stdout, Clock)\n\n\
             fn main(out: Stdout, clock: Clock) {\n    let p = Pair(out, clock)\n    \
             match p {\n        Pair(o, _) => {\n            let _ = o.println(\"hi\")\n        }\n    }\n}",
        ),
        [("E0297", "Clock")]
    );
}

#[test]
fn a_program_with_another_error_is_not_judged_for_unread_fields() {
    // `missing` is unknown, so the body that might read `clock` failed to
    // check; only that error is reported.
    let codes: Vec<&str> = release_errors(&handed_on(
        "fn work(tools: Tools) {\n    let _ = tools.out.println(\"hi\")\n    \
         let _ = missing(tools)\n}",
    ))
    .into_iter()
    .map(|(code, _)| code)
    .collect();
    assert!(!codes.is_empty() && !codes.contains(&"E0297"), "{codes:?}");
}

#[test]
fn an_error_found_after_the_bodies_also_holds_back_unread_fields() {
    // A constant cycle (`E0236`) is found after every body is checked.
    let codes: Vec<&str> = release_errors(&program(
        "const A: Int = B\n\nconst B: Int = A\n\n\
         fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.out.println(\"hi\")\n}",
    ))
    .into_iter()
    .map(|(code, _)| code)
    .collect();
    assert!(!codes.is_empty() && !codes.contains(&"E0297"), "{codes:?}");
}

#[test]
fn the_release_codes_are_the_ones_the_catalog_marks_release_only() {
    let catalog = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/checker.md"),
    )
    .expect("should read docs/checker.md");
    let mut marked: Vec<&str> = catalog
        .lines()
        .filter(|line| line.contains("when checking with `--release`"))
        .filter_map(|line| {
            line.strip_prefix("| `")?
                .split_once('`')
                .map(|(code, _)| code)
        })
        .collect();
    marked.sort_unstable();
    let mut codes = oska_check::RELEASE_CODES.to_vec();
    codes.sort_unstable();
    assert_eq!(codes, marked);
}

// ---- The fix ------------------------------------------------------------------------

/// The source after applying the fix of the first `E0297`, and how safe
/// that fix is.
fn fixed(src: &str) -> (String, Applicability) {
    let report = check_module_with(src, CheckOptions { release: true });
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code == "E0297")
        .unwrap_or_else(|| panic!("{src:?} should report E0297: {:#?}", report.diagnostics));
    let fix = diagnostic.fix.as_ref().expect("E0297 should carry a fix");
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = src.to_string();
    for edit in edits {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    (out, fix.applicability)
}

#[test]
fn the_fix_removes_the_last_field_and_is_maybe() {
    let (src, applicability) = fixed(&program(
        "fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.out.println(\"hi\")\n}",
    ));
    assert_eq!(applicability, Applicability::Maybe);
    assert!(
        src.starts_with("cap struct Tools {\n    out: Stdout,\n}\n"),
        "{src}"
    );
}

#[test]
fn the_fix_removes_a_first_field_with_its_comma() {
    let (src, _) = fixed(&program(
        "fn main(out: Stdout, clock: Clock) {\n    let tools = Tools(out: out, clock: clock)\n    \
         let _ = tools.clock.now_millis()\n}",
    ));
    assert!(
        src.starts_with("cap struct Tools {\n    clock: Clock,\n}\n"),
        "{src}"
    );
}

// ---- The repository's own programs --------------------------------------------------

/// The repository root.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The tracked files under `dirs`, relative to the repository root.
fn tracked(dirs: &[&str]) -> Vec<String> {
    let output = Command::new("git")
        .arg("ls-files")
        .args(dirs)
        .current_dir(repo())
        .output()
        .expect("git should run");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8(output.stdout)
        .expect("paths should be UTF-8")
        .lines()
        .map(str::to_string)
        .collect()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("should read {path}: {e}"))
}

/// Every finished Oska program the repository ships passes the release
/// check, so none holds unused authority: the site's generator and link
/// checker, and the examples, each package checked as a package. The eval
/// tasks' solutions are release-checked by `scripts/eval-tasks`, and the
/// test fixtures fail on purpose.
#[test]
fn every_program_the_repository_ships_passes_the_release_check() {
    let files = tracked(&["site", "examples"]);
    let packages: Vec<&str> = files
        .iter()
        .filter_map(|f| f.strip_suffix("/oska.toml"))
        .collect();
    let release = CheckOptions { release: true };
    let mut failures = Vec::new();
    for dir in &packages {
        let prefix = format!("{dir}/src/");
        let modules = files
            .iter()
            .filter_map(|f| {
                f.strip_prefix(&prefix)?
                    .strip_suffix(".osk")
                    .map(|m| (f, m))
            })
            .map(|(file, module)| ModuleSource::in_package(&module.replace('/', "."), &read(file)))
            .collect();
        let package = Package {
            manifest: Some(read(&format!("{dir}/oska.toml"))),
            modules,
        };
        let report = check_package(&package, release);
        failures.extend(
            report
                .diagnostics
                .iter()
                .map(|d| format!("{dir}: {}", d.message)),
        );
    }
    let mut checked = packages.len();
    for file in files.iter().filter(|f| f.ends_with(".osk")) {
        if packages
            .iter()
            .any(|dir| file.starts_with(&format!("{dir}/")))
        {
            continue;
        }
        checked += 1;
        let report = check_file(file, &read(file), release);
        failures.extend(
            report
                .diagnostics
                .iter()
                .map(|d| format!("{file}: {}", d.message)),
        );
    }
    assert!(checked >= 5, "only {checked} programs found");
    assert!(failures.is_empty(), "{failures:#?}");
}
