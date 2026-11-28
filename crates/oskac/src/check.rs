// SPDX-License-Identifier: Apache-2.0
//! `oskac check`: report diagnostics, authority and audit sites.

use crate::authority_gate::{self, Gate};
use crate::cache;
use crate::cli::Flags;
use crate::source::read_source;
use crate::verdict;
use oska_syntax::{Applicability, Diagnostic};
use std::process::ExitCode;

/// `oskac check`: print every diagnostic, each function's authority with
/// its attenuations, and every audit site, as `{"diagnostics": [...],
/// "functions": [...], "audit": [...], "attenuations": [...]}` with
/// `--format=json`.
/// The exit code is non-zero when there are diagnostics.
/// A result with no diagnostics is reused from the build cache, `store`,
/// when its inputs are unchanged; with a baseline or a grant it is always checked.
pub(crate) fn check_file(
    path: &str,
    flags: Flags,
    gate: Option<Gate>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    check_source(path, &src, flags, gate, store)
}

/// [`check_file`] on `src`, the text of the module `path` names.
pub(crate) fn check_source(
    path: &str,
    src: &str,
    Flags { json, release, .. }: Flags,
    gate: Option<Gate>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let check = || oska_check::check_file(path, src, oska_check::CheckOptions { release });
    match gate {
        Some(_) => finish_check(&check(), json, path, gate),
        None => cache::Cached::new(store, cache::file_sources(path, src))
            .check(&check_flags(json, release), || {
                render_report(&check(), json, path)
            })
            .emit(),
    }
}

/// The flags that change what `oskac check` prints, for its cache key.
pub(crate) fn check_flags(json: bool, release: bool) -> [&'static [u8]; 2] {
    [
        if json { b"json" } else { b"text" },
        if release { b"release" } else { b"debug" },
    ]
}

/// Print a check report, then compare it with a baseline or write one
/// when `gate` says so, and return the exit code.
pub(crate) fn finish_check(
    report: &oska_check::CheckReport,
    json: bool,
    path: &str,
    gate: Option<Gate>,
) -> ExitCode {
    match gate {
        Some(gate) => authority_gate::check(report, json, path, gate),
        None => print_report(report, json, path),
    }
}

/// Print a check report as `oskac check` does, and return its exit code.
/// `path` names the unlocated entries' file in the human report.
pub(crate) fn print_report(report: &oska_check::CheckReport, json: bool, path: &str) -> ExitCode {
    render_report(report, json, path).emit()
}

/// What [`print_report`] prints, and the exit code.
pub(crate) fn render_report(
    report: &oska_check::CheckReport,
    json: bool,
    path: &str,
) -> cache::Printed {
    let verdict = verdict::Verdict::of(report, None);
    let (stdout, stderr) = if json {
        (verdict::check_json(report, None, verdict), String::new())
    } else {
        (
            report.authority_to_human(path),
            diagnostics_text(&report.diagnostics),
        )
    };
    cache::Printed {
        stdout,
        stderr,
        code: verdict.exit_code(),
    }
}

/// Print diagnostics to stderr, each after its location,
/// `path:line:col`, and with its fix, as `oskac check` does.
pub(crate) fn print_diagnostics(diagnostics: &[Diagnostic]) {
    eprint!("{}", diagnostics_text(diagnostics));
}

/// What [`print_diagnostics`] prints.
pub(crate) fn diagnostics_text(diagnostics: &[Diagnostic]) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for diagnostic in diagnostics {
        let _ = match &diagnostic.location {
            Some(location) => writeln!(text, "{location}: {diagnostic}"),
            None => writeln!(text, "{diagnostic}"),
        };
        if let Some(fix) = &diagnostic.fix {
            let _ = match fix.applicability {
                Applicability::Machine => writeln!(text, "    fix: {}", fix.message),
                other => writeln!(text, "    fix ({}): {}", other.name(), fix.message),
            };
        }
        // A panic's call stack, under its line.
        if let Some(frames) = &diagnostic.frames {
            text.push_str(&frames.human());
        }
    }
    text
}
