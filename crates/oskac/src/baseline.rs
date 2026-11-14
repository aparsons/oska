//! `oskac check --update-baseline=<file>` and `--baseline=<file>`: write
//! a program's **authority baseline**, or compare its authority report
//! with one and fail when authority widened (`oska_check::authority_diff`
//! owns the comparison).
//!
//! Exit codes: 1 for diagnostics (no diff is computed: a refused
//! program's report is incomplete), an unreadable or malformed baseline,
//! or a baseline that cannot be written; 3 when the program checks but
//! the gate fails; 0 otherwise. There is no flag that lets a widening
//! pass: approving one means updating the baseline in the same change.

use std::process::ExitCode;

use oska_check::authority_diff::{diff, AuthorityRecord};
use oska_check::CheckReport;

/// The exit code of a check whose authority widened against its baseline.
const WIDENED: u8 = 3;

/// What `oskac check` does with a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate<'a> {
    /// `--baseline=<file>`: compare with it.
    Compare(&'a str),
    /// `--update-baseline=<file>`: write it.
    Update(&'a str),
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
        Gate::Update(file) => {
            let code = crate::print_report(report, json, path);
            match write(report, file) {
                Ok(()) => code,
                Err(message) => {
                    eprintln!("error: cannot write the baseline {file}: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Gate::Compare(file) => compare(report, json, path, file),
    }
}

fn write(report: &CheckReport, file: &str) -> Result<(), String> {
    let text = AuthorityRecord::of(report).to_baseline(env!("CARGO_PKG_VERSION"))?;
    std::fs::write(file, text).map_err(|err| err.to_string())
}

/// Compare `report` with the baseline in `file`: the report gains the
/// diff, and the exit code is 3 when the gate fails.
fn compare(report: &CheckReport, json: bool, path: &str, file: &str) -> ExitCode {
    let compared = std::fs::read_to_string(file)
        .map_err(|err| format!("cannot read the baseline {file}: {err}"))
        .and_then(|text| {
            AuthorityRecord::read(&text).map_err(|err| format!("the baseline {file}: {err}"))
        })
        .and_then(|baseline| diff(&baseline, &AuthorityRecord::of(report)));
    let difference = match compared {
        Ok(difference) => difference,
        Err(message) => {
            // The report has no diagnostics, so this exit code is 0: the
            // unreadable baseline decides it.
            let _ = crate::print_report(report, json, path);
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };
    if json {
        let value = difference.to_json(file);
        out!("{}", report.to_json_with(Some(("authority_diff", &value))));
    } else {
        let _ = crate::print_report(report, json, path);
        out!("{}", difference.to_human(file, env!("CARGO_PKG_VERSION")));
    }
    if difference.fails() {
        eprintln!(
            "error: authority widened against the baseline {file}: a function gained \
             authority or became impure, an audit site was added, or the program reaches a \
             new kind; approving this means updating the baseline, a change for the user \
             to review"
        );
        ExitCode::from(WIDENED)
    } else {
        ExitCode::SUCCESS
    }
}
