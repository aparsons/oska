// SPDX-License-Identifier: Apache-2.0
//! `oskac test`: run a program's tests and settle their snapshots.

use crate::cache;
use crate::check::print_diagnostics;
use crate::cli::Flags;
use crate::package;
use crate::places::Places;
use crate::run::{refuse_unsupported, Gated};
use crate::source::read_source;
use crate::{native, native_test, DNS_TEST_HOOKS};
use oska_syntax::{Diagnostic, Span};
use std::process::ExitCode;

/// `oskac test`: run the file's `test` blocks, the `#[integration]`
/// ones too, on the real OS, with `--integration`. The report goes to
/// stdout: one line per test, the failures, and a summary, or one JSON
/// object with `--format=json`. A refused file's diagnostics go to stderr
/// as `oskac check` prints them, or into the JSON report. The exit code
/// is 1 when a test failed (see `docs/runtime.md`).
pub(crate) fn test_file(
    path: &str,
    flags: Flags,
    filter: Option<&str>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let Flags {
        json, integration, ..
    } = flags;
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let cached = cache::Cached::new(store, cache::file_sources(path, &src));
    let options = oska_run::TestOptions {
        integration,
        filter,
    };
    let places = Places::File { path, src: &src };
    if let Some(target) = native::native_target(flags) {
        let checked = oska_check::check_module(&src);
        if let Some(program) = checked
            .program
            .as_ref()
            .filter(|_| checked.diagnostics.is_empty())
        {
            match native_test::test_program(program, target, options, json, &cached) {
                native_test::Tested::Report(mut report) => {
                    settle_snapshots(&mut report, path, &places, flags);
                    return finish_test(report, json, &places);
                }
                native_test::Tested::Ended(code) => return code,
                native_test::Tested::Refused(refusal) => {
                    if let Some(refusal) =
                        native::gate(refusal, oska_run::test_refusal(program), &places)
                    {
                        return refuse_unsupported(Gated::Test, refusal, json);
                    }
                }
            }
        }
    }
    let mut report = oska_run::test(
        &src,
        &mut oska_run::RealHost::new().with_dns_test_hooks(DNS_TEST_HOOKS),
        options,
    );
    settle_snapshots(&mut report, path, &places, flags);
    finish_test(report, json, &places)
}

/// Compare the snapshots the tests of `report` recorded with their files,
/// or with `--update` write the new and changed ones
/// (`oska_run::snapshots::settle`). `target` is the single file or the
/// package directory as given. The human report names each snapshot
/// written from the current directory.
pub(crate) fn settle_snapshots(
    report: &mut oska_run::TestReport,
    target: &str,
    places: &Places,
    flags: Flags,
) {
    let dir = match places {
        Places::Package { locator, .. } => {
            let file_of = |span| package_test_file(locator, span);
            settle_in(report, target, &file_of, flags.update);
            target.to_string()
        }
        _ => {
            let (dir, stem) = file_snapshots(target);
            settle_in(report, &dir, &|_| Some(stem.clone()), flags.update);
            dir
        }
    };
    if !flags.json {
        for written in report.snapshots.iter_mut().flatten() {
            written.path = package::from_here(&dir, &written.path);
        }
    }
}

/// [`oska_run::snapshots::settle`] under the directory `dir`, the current
/// one when it is empty.
pub(crate) fn settle_in(
    report: &mut oska_run::TestReport,
    dir: &str,
    file_of: &dyn Fn(Span) -> Option<String>,
    update: bool,
) {
    let dir = if dir.is_empty() { "." } else { dir };
    oska_run::snapshots::settle(report, std::path::Path::new(dir), file_of, update);
}

/// Where the snapshots of the single file `path` go: its directory, as
/// `path` names it, and its `<test-file>`, its name without `.osk`.
pub(crate) fn file_snapshots(path: &str) -> (String, String) {
    let path = std::path::Path::new(path);
    let dir = path
        .parent()
        .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned());
    let stem = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    (dir, stem)
}

/// The `<test-file>` of a package's snapshots for the test at `span`: its
/// file under `src/` without `.osk`, such as `lib` or `util/text`, or
/// `None` when the span is in no file of the package under `src/`.
pub(crate) fn package_test_file(locator: &oska_check::Locator, span: Span) -> Option<String> {
    let located = locator.locate(Diagnostic::new(oska_run::codes::SNAPSHOT_FAILED, "", span));
    let path = located.location?.path;
    let module = path.strip_prefix("src/")?.strip_suffix(".osk")?;
    (!module.is_empty()).then(|| module.to_string())
}

/// Print a test report and return its exit code. `places` names the file
/// of each failure and refusal, as for [`finish_run`], and each test's
/// module in a package. A `--filter` that left out every test is an
/// error, so a typo is never a passing run.
pub(crate) fn finish_test(
    mut report: oska_run::TestReport,
    json: bool,
    places: &Places,
) -> ExitCode {
    if let (Some(filtered), true) = (&report.filtered, report.tests.is_empty()) {
        return no_test_matches(filtered, json);
    }
    for test in &mut report.tests {
        test.module = places.module_of(test.span);
        test.failure = test.failure.take().map(|d| places.locate(d));
    }
    report.diagnostics = report
        .diagnostics
        .drain(..)
        .map(|d| places.locate(d))
        .collect();
    if json {
        outln!("{}", report.to_json());
    } else if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics);
    } else {
        out!("{}", report.to_human(""));
    }
    ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(u8::MAX))
}

/// The error of a `--filter` that matched none of the program's tests,
/// as text on stderr or as JSON on stdout. A program with no tests at
/// all is told apart, so the filter is not blamed for it.
pub(crate) fn no_test_matches(filtered: &oska_run::Filtered, json: bool) -> ExitCode {
    let oska_run::Filtered {
        substring,
        left_out,
    } = filtered;
    let shown = oska_run::escape_control(substring);
    let (code, message) = if *left_out == 0 {
        (
            "no_tests",
            format!("the program has no tests, so the filter `{shown}` matches none"),
        )
    } else {
        (
            "no_test_matches",
            format!("no test name contains `{shown}`; {left_out} tests filtered out"),
        )
    };
    if json {
        outln!(
            "{{\"ok\":false,\"error\":\"{code}\",\"message\":\"{}\",\"filter\":\"{}\",\"filtered\":{left_out}}}",
            oska_syntax::json_escape(&message),
            oska_syntax::json_escape(substring)
        );
    } else {
        eprintln!("error: {message}");
    }
    ExitCode::FAILURE
}
