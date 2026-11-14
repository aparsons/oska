//! The post-edit event: after the agent edited a `.osk` file or an
//! `oska.toml`, check what it belongs to and hand back only the
//! diagnostics, compactly.
//!
//! The target is the one [`package::discover`] finds: a module of a
//! package is checked with its package, since checked alone it reports
//! errors the package does not have; any other `.osk` file is checked
//! alone. The check is `oskac check` without `--release`. Any other file
//! is none of the hook's business: nothing is checked and nothing said.

use std::path::Path;

use oska_check::CheckOptions;
use oska_syntax::{Applicability, Diagnostic};

use super::target::{line, Shown, SHOWN};
use super::{Edit, Verdict};
use crate::package::{self, Loaded, Target};

/// What the hook says about `edit`.
pub fn verdict_for(edit: &Edit) -> Verdict {
    let Some(target) = target_of(&edit.file) else {
        return Verdict::quiet();
    };
    let target = Shown::new(target, edit.cwd.as_deref());
    match check(&target) {
        Ok(diagnostics) => verdict(&target.name, &diagnostics),
        Err(reason) => Verdict::failed(&reason),
    }
}

/// The target an edit of `file` asks to check, or `None` for a file that
/// is neither a `.osk` file nor an `oska.toml`, or an `oska.toml` that is
/// not a regular file and so is no package's manifest.
fn target_of(file: &Path) -> Option<Target> {
    let is_manifest = file.file_name().is_some_and(|n| n == "oska.toml");
    let is_source = file.extension().is_some_and(|e| e == "osk");
    if !is_manifest && !is_source {
        return None;
    }
    match package::discover(&file.to_string_lossy()) {
        Target::File(_) if is_manifest => None,
        target => Some(target),
    }
}

/// Every diagnostic of `oskac check` on the target, located, with its
/// path written from the project's directory. An error says what could
/// not be read.
fn check(shown: &Shown) -> Result<Vec<Diagnostic>, String> {
    let options = CheckOptions { release: false };
    match &shown.target {
        Target::File(path) => {
            let src = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", shown.name))?;
            let report = oska_check::check_file(&shown.name, &src, options);
            Ok(report
                .diagnostics
                .into_iter()
                .map(|d| match d.location {
                    Some(_) => d,
                    None => d.locate(&shown.name, &src),
                })
                .collect())
        }
        Target::Dir(dir) => {
            let diagnostics = match package::load(dir, "check") {
                Loaded::Package { package, refused } => {
                    let mut report = oska_check::check_package(&package, options);
                    report.diagnostics.splice(0..0, refused);
                    report.diagnostics
                }
                Loaded::Refused(diagnostics) => diagnostics,
                Loaded::Unreadable(message) => return Err(message),
            };
            Ok(diagnostics
                .into_iter()
                .map(|mut d| {
                    if let Some(location) = &mut d.location {
                        location.path = package::from_here(&shown.name, &location.path);
                    }
                    d
                })
                .collect())
        }
    }
}

/// What to print for the target `name`, and the exit status, given its
/// diagnostics: nothing for none; otherwise one line each, at most
/// [`SHOWN`], then `and N more`, then how many have a `machine` fix and
/// the command that applies them.
fn verdict(name: &str, diagnostics: &[Diagnostic]) -> Verdict {
    if diagnostics.is_empty() {
        return Verdict::quiet();
    }
    let mut out = String::new();
    for diagnostic in diagnostics.iter().take(SHOWN) {
        out.push_str(&line(name, diagnostic));
        out.push('\n');
    }
    if diagnostics.len() > SHOWN {
        out.push_str(&format!("and {} more\n", diagnostics.len() - SHOWN));
    }
    let machine = diagnostics
        .iter()
        .filter(|d| {
            d.fix
                .as_ref()
                .is_some_and(|fix| fix.applicability == Applicability::Machine)
        })
        .count();
    if machine > 0 {
        out.push_str(&format!(
            "{machine} of {} diagnostics have a `machine` fix: run `oskac fix {name}`\n",
            diagnostics.len()
        ));
    }
    Verdict::feedback(out)
}
