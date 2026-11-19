//! `oskac check --update-baseline=<file>` and `--baseline=<file>`: write
//! a program's **authority baseline**, or compare its authority report
//! with one and fail when authority widened (`oska_check::authority_diff`
//! owns the comparison). `--grant=<file>`, on `check` and `run`, compares
//! with an **authority grant** the same way: only the words differ, and a
//! grant is never written.
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

use crate::package::{self, Files};
use crate::verdict::{check_json, Verdict};

/// What `oskac check` does with a baseline or a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate<'a> {
    /// `--baseline=<file>`: compare with it.
    Compare(&'a str),
    /// `--update-baseline=<file>`: write it.
    Update(&'a str),
    /// `--grant=<file>`: compare with it, as with a baseline.
    Grant(&'a str),
}

/// Print `report` as `oskac check` does, then apply `gate`, and return
/// the exit code. `path` names the unlocated entries' file in the human
/// report.
pub(crate) fn check(report: &CheckReport, json: bool, path: &str, gate: Gate) -> ExitCode {
    if !report.diagnostics.is_empty() {
        let code = crate::print_report(report, json, path);
        if let Gate::Update(file) = gate {
            eprintln!("error: the baseline {file} was not written: the program has diagnostics");
        }
        return code;
    }
    match gate {
        Gate::Update(file) => match write(report, file) {
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
        Gate::Compare(file) => compare(report, json, path, Against::Baseline, file),
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
/// grant (`against`) in `file`; an error when the file cannot be read or
/// the diff cannot be made.
fn compared(report: &CheckReport, against: Against, file: &str) -> Result<AuthorityDiff, String> {
    let word = against.word();
    std::fs::read_to_string(file)
        .map_err(|err| format!("cannot read the {word} {file}: {err}"))
        .and_then(|text| {
            AuthorityRecord::read(&text).map_err(|err| format!("the {word} {file}: {err}"))
        })
        .and_then(|record| diff_against(against, &record, &AuthorityRecord::of(report)))
}

/// The `error:` line for a program whose authority goes past `against`
/// in `file`.
fn widened_error(against: Against, file: &str) -> String {
    let what = "a function gained authority or became impure, an audit site was added, or \
                the program reaches a new kind";
    match against {
        Against::Baseline => format!(
            "error: authority widened against the baseline {file}: {what}; approving this \
             means updating the baseline, a change for the user to review"
        ),
        Against::Grant => format!(
            "error: authority goes past the grant {file}: {what}; a grant is set by whoever \
             runs the code, and oskac never updates it"
        ),
    }
}

/// `oskac run --grant=<file>`: compare `report`, the checked program
/// about to run, with the grant in `file`. `None` when it may run: within
/// the grant, or with diagnostics, which the run reports as it does
/// without a grant (nothing is compared). Otherwise the exit code that
/// ends the run before it starts: 1 for a grant that cannot be read or
/// compared, 3, with the diff on standard error, for a program past it.
pub(crate) fn refuse_run(report: &CheckReport, json: bool, file: &str) -> Option<ExitCode> {
    if !report.diagnostics.is_empty() {
        return None;
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
    if json {
        // One JSON object per line, as `run --format=json` prints every
        // object on standard error: the diff's layout is only line breaks
        // and indents, since its strings escape every line break.
        let value = difference.to_json(Against::Grant, file);
        let line: String = value.lines().map(str::trim_start).collect();
        eprintln!(
            "{{\"authority_diff\":{line},\"verdict\":\"{}\"}}",
            verdict.name()
        );
    } else {
        eprint!(
            "{}",
            difference.to_human(Against::Grant, file, env!("CARGO_PKG_VERSION"))
        );
    }
    eprintln!(
        "{}; the program was not run",
        widened_error(Against::Grant, file)
    );
    Some(ExitCode::from(verdict.exit_code()))
}

/// Compare `report` with the baseline or grant (`against`) in `file`: the
/// report gains the diff, and the exit code is 3 when the gate fails.
fn compare(report: &CheckReport, json: bool, path: &str, against: Against, file: &str) -> ExitCode {
    let difference = match compared(report, against, file) {
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
    if json {
        let value = difference.to_json(against, file);
        out!("{}", check_json(report, Some(&value), verdict));
    } else {
        let _ = crate::print_report(report, json, path);
        out!(
            "{}",
            difference.to_human(against, file, env!("CARGO_PKG_VERSION"))
        );
    }
    if verdict == Verdict::Widened {
        eprintln!("{}", widened_error(against, file));
    }
    ExitCode::from(verdict.exit_code())
}
