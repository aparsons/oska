//! The **verdict** of `oskac check`: one value that says whether the
//! program may go on, and the exit code that says the same. With
//! `--format=json` it is the check object's last key, `verdict`, so an
//! embedder branches on one value.
//!
//! The verdict is decided here and only here: every way an authority
//! comparison fails (widened, became impure, new audit site, new kind)
//! is [`Verdict::Widened`], whatever the comparison is with.

use oska_check::authority_diff::AuthorityDiff;
use oska_check::CheckReport;

/// What `oskac check` concludes about a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// No diagnostics, and no authority comparison failed: exit code 0.
    Ok,
    /// The program has diagnostics: exit code 1. Nothing is compared.
    Diagnostics,
    /// The program checks, but its authority goes past what it is
    /// compared with: exit code 3.
    Widened,
}

impl Verdict {
    /// The verdict on `report`, compared with `difference` when there is
    /// one.
    pub(crate) fn of(report: &CheckReport, difference: Option<&AuthorityDiff>) -> Verdict {
        if !report.diagnostics.is_empty() {
            Verdict::Diagnostics
        } else if difference.is_some_and(AuthorityDiff::fails) {
            Verdict::Widened
        } else {
            Verdict::Ok
        }
    }

    /// The value of the `verdict` key.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Diagnostics => "diagnostics",
            Verdict::Widened => "widened",
        }
    }

    /// The exit code of `oskac check` with this verdict.
    pub(crate) fn exit_code(self) -> u8 {
        match self {
            Verdict::Ok => 0,
            Verdict::Diagnostics => 1,
            Verdict::Widened => 3,
        }
    }
}

/// The check object of `oskac check --format=json`: `report`, then
/// `authority_diff` when a comparison ran (its JSON), then the verdict,
/// always last.
pub(crate) fn check_json(
    report: &CheckReport,
    authority_diff: Option<&str>,
    verdict: Verdict,
) -> String {
    let verdict = format!("\"{}\"", verdict.name());
    let mut extra = Vec::with_capacity(2);
    if let Some(value) = authority_diff {
        extra.push(("authority_diff", value));
    }
    extra.push(("verdict", verdict.as_str()));
    report.to_json_with(&extra)
}
