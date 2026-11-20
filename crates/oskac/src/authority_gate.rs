//! The authority gate: what a program's authority report is compared
//! with, or written to, before `oskac` lets it pass
//! (`oska_check::authority_diff` owns the comparison).
//!
//! - `oskac check --update-baseline=<file>` writes the program's
//!   **authority baseline**, and `--baseline=<file>` compares with one.
//! - `oskac check --grant=<file>` and `oskac run --grant=<file>` compare
//!   with an **authority grant**, by the same rules: only the words
//!   differ, and a grant is never written.
//!
//! Exit codes, which [`Verdict`] decides with the JSON `verdict`: 1 for
//! diagnostics (no diff is computed: a refused program's report is
//! incomplete); 3 when the program checks but the gate fails; 0
//! otherwise. An unreadable or malformed baseline or grant, or a baseline
//! that cannot be written, is an `error:` and exit code 1, with no verdict
//! and, with `--format=json`, no check object. There is no flag that lets
//! a widening pass: approving one means updating the baseline in the same
//! change, and a grant only its embedder changes.

use std::process::ExitCode;

use oska_check::authority_diff::{diff_against, Against, AuthorityDiff, AuthorityRecord};
use oska_check::CheckReport;
use oska_syntax::json_escape;

use crate::package::{self, Files};
use crate::verdict::{check_json, Verdict};

/// What a program's authority is compared with, or written to, and the
/// file that holds it. `oskac check` takes any one; `oskac run` only a
/// grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate<'a> {
    /// `--baseline=<file>`: compare with the baseline.
    Baseline(&'a str),
    /// `--update-baseline=<file>`: write the baseline.
    UpdateBaseline(&'a str),
    /// `--grant=<file>`: compare with the grant.
    Grant(&'a str),
}

/// Print `report` as `oskac check` does, then apply `gate`, and return
/// the exit code. `path` names the unlocated entries' file in the human
/// report.
pub(crate) fn check(report: &CheckReport, json: bool, path: &str, gate: Gate) -> ExitCode {
    if !report.diagnostics.is_empty() {
        let code = crate::print_report(report, json, path);
        if let Gate::UpdateBaseline(file) = gate {
            eprintln!("error: the baseline {file} was not written: the program has diagnostics");
        }
        return code;
    }
    match gate {
        Gate::UpdateBaseline(file) => match write(report, file) {
            Ok(()) => crate::print_report(report, json, path),
            Err(message) => {
                // As for a baseline that cannot be read: the error
                // decides, so `--format=json` prints no check object,
                // whose verdict would be `ok`.
                if !json {
                    let _ = crate::print_report(report, json, path);
                }
                eprintln!("error: cannot write the baseline {file}: {message}");
                ExitCode::FAILURE
            }
        },
        Gate::Baseline(file) => compare(report, json, path, Against::Baseline, file),
        Gate::Grant(file) => compare(report, json, path, Against::Grant, file),
    }
}

/// Write the baseline of `report` to `file` as `oskac fix` writes a file:
/// to a new file beside it, which then replaces it in one step, so a
/// reader never sees half a baseline; and never through a symlink at
/// `file`. The directories on the way are the user's choice, and followed.
fn write(report: &CheckReport, file: &str) -> Result<(), String> {
    let text = AuthorityRecord::of(report).to_baseline(env!("CARGO_PKG_VERSION"))?;
    package::parent_and_name(file)
        .and_then(|(parent, name)| Files::open(parent)?.replace(&[name], text.as_bytes()))
        .map_err(|err| err.to_string())
}

/// Compare `report`, which has no diagnostics, with the baseline or
/// grant (`kind`) in `file`; an error when the file cannot be read or
/// the diff cannot be made.
fn compared(report: &CheckReport, kind: Against, file: &str) -> Result<AuthorityDiff, String> {
    let word = kind.word();
    std::fs::read_to_string(file)
        .map_err(|err| format!("cannot read the {word} {file}: {err}"))
        .and_then(|text| {
            AuthorityRecord::read(&text).map_err(|err| format!("the {word} {file}: {err}"))
        })
        .and_then(|against| diff_against(kind, &against, &AuthorityRecord::of(report)))
}

/// Why a program whose authority goes past the baseline or grant (`kind`)
/// in `file` fails the gate, without `error: `.
fn widened_message(kind: Against, file: &str) -> String {
    let what = "a function gained authority or became impure, an audit site was added, or \
                the program reaches a new kind";
    match kind {
        Against::Baseline => format!(
            "authority widened against the baseline {file}: {what}; approving this means \
             updating the baseline, a change for the user to review"
        ),
        Against::Grant => format!(
            "authority goes past the grant {file}: {what}; a grant is set by whoever runs \
             the code, and oskac never updates it"
        ),
    }
}

/// The diff, as `oskac` shows it for the baseline or grant (`kind`) in
/// `file`: the JSON value of `authority_diff` with `json`, else the
/// human report's `baseline:` or `grant:` section.
fn shown_diff(difference: &AuthorityDiff, json: bool, kind: Against, file: &str) -> String {
    if json {
        difference.to_json(kind, file)
    } else {
        difference.to_human(kind, file, env!("CARGO_PKG_VERSION"))
    }
}

/// `oskac run --grant=<file>`: compare `report`, the one check of the
/// program about to run, with the grant in `file`. `None` when it may
/// run. Otherwise the exit code that ends the run before it starts: 1
/// for a report with diagnostics (the caller reports those before it
/// asks) or a grant that cannot be read or compared; 3, with the diff on
/// standard error, for a program past its grant.
pub(crate) fn refuse_run(report: &CheckReport, json: bool, file: &str) -> Option<ExitCode> {
    // Fail closed: nothing with diagnostics passes a grant.
    if !report.diagnostics.is_empty() {
        return Some(ExitCode::FAILURE);
    }
    let difference = match compared(report, Against::Grant, file) {
        Ok(difference) => difference,
        Err(message) => {
            eprintln!("error: {message}");
            return Some(ExitCode::FAILURE);
        }
    };
    let verdict = Verdict::of(report, Some(&difference));
    if verdict != Verdict::Widened {
        return None;
    }
    let shown = shown_diff(&difference, json, Against::Grant, file);
    let message = format!(
        "{}; the program was not run",
        widened_message(Against::Grant, file)
    );
    if json {
        // One JSON object on one line, as `run --format=json` prints
        // every object on standard error. The diff's layout is only line
        // breaks and indents: its strings escape every line break.
        let value: String = shown.lines().map(str::trim_start).collect();
        eprintln!(
            "{{\"authority_diff\":{value},\"verdict\":\"{}\",\"error\":\"{}\"}}",
            verdict.name(),
            json_escape(&message)
        );
    } else {
        eprint!("{shown}");
        eprintln!("error: {message}");
    }
    Some(ExitCode::from(verdict.exit_code()))
}

/// Compare `report` with the baseline or grant (`kind`) in `file`: the
/// report gains the diff, and the exit code is 3 when the gate fails.
fn compare(report: &CheckReport, json: bool, path: &str, kind: Against, file: &str) -> ExitCode {
    let difference = match compared(report, kind, file) {
        Ok(difference) => difference,
        Err(message) => {
            // Nothing was compared, so there is no verdict, and with
            // `--format=json` no check object: the error decides. The
            // human report still lists the program's authority.
            if !json {
                let _ = crate::print_report(report, json, path);
            }
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };
    let verdict = Verdict::of(report, Some(&difference));
    let shown = shown_diff(&difference, json, kind, file);
    if json {
        out!("{}", check_json(report, Some(&shown), verdict));
    } else {
        let _ = crate::print_report(report, json, path);
        out!("{shown}");
    }
    if verdict == Verdict::Widened {
        eprintln!("error: {}", widened_message(kind, file));
    }
    ExitCode::from(verdict.exit_code())
}
