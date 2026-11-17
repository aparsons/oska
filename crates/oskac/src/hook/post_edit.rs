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
use oska_syntax::{json_escape, Applicability, Diagnostic};

use super::target::{self, json_list, line, Shown, SHOWN};
use super::{Edited, Format, Verdict};
use crate::package::{self, Target};

/// What the hook says about `edited`, written as `format` says.
pub fn verdict_for(edited: &Edited, format: Format) -> Verdict {
    let Some(target) = target_of(&edited.file) else {
        return Verdict::quiet();
    };
    let shown = Shown::new(target, edited.cwd.as_deref());
    match target::check(&shown, CheckOptions { release: false }) {
        Ok(checked) => verdict(&shown.name, &checked.diagnostics(), format),
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

/// What to print for the target `name`, and the exit status, given its
/// diagnostics: nothing for none. Otherwise, as text, one line each, at
/// most [`SHOWN`], then `and N more`, then how many have a `machine` fix
/// and the command that applies them; as JSON, one object saying the
/// same, with each diagnostic in its full shape.
fn verdict(name: &str, diagnostics: &[Diagnostic], format: Format) -> Verdict {
    if diagnostics.is_empty() {
        return Verdict::quiet();
    }
    let more = diagnostics.len().saturating_sub(SHOWN);
    // The fixes `oskac fix` applies: no fix that would widen authority is
    // `machine`, so each one counted here is one it will apply.
    let machine = diagnostics
        .iter()
        .filter(|d| {
            d.fix
                .as_ref()
                .is_some_and(|fix| fix.applicability == Applicability::Machine)
        })
        .count();
    let command = format!("oskac fix {name}");
    let out = match format {
        Format::Json => {
            let command = if machine > 0 {
                format!("\"{}\"", json_escape(&command))
            } else {
                "null".to_string()
            };
            format!(
                "{{\"event\":\"post-edit\",\"target\":\"{}\",\"diagnostics\":{},\"more\":{more},\
                 \"machine_fixes\":{machine},\"fix_command\":{command}}}\n",
                json_escape(name),
                json_list(diagnostics)
            )
        }
        Format::Text => {
            let mut out = String::new();
            for diagnostic in diagnostics.iter().take(SHOWN) {
                out.push_str(&line(name, diagnostic));
                out.push('\n');
            }
            if more > 0 {
                out.push_str(&format!("and {more} more\n"));
            }
            if machine > 0 {
                out.push_str(&format!(
                    "{machine} of {} diagnostics have a `machine` fix: run `{command}`\n",
                    diagnostics.len()
                ));
            }
            out
        }
    };
    Verdict::feedback(out)
}
