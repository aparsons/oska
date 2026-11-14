//! What the events share: a target with the name it is shown by, its
//! check, and how a diagnostic is printed, as a line or as JSON.

use std::path::Path;

use oska_check::{CheckOptions, CheckReport, Package};
use oska_syntax::Diagnostic;

use crate::package::{self, Loaded, Target};

/// The most diagnostics (or failed tests) shown for a target; the rest
/// are counted.
pub const SHOWN: usize = 20;

/// A target, with the name it is shown by: written from the project's
/// directory when it is under it, as the checker's own paths are.
pub struct Shown {
    pub target: Target,
    pub name: String,
}

impl Shown {
    pub fn new(target: Target, cwd: Option<&Path>) -> Shown {
        let path = match &target {
            Target::File(path) | Target::Dir(path) => path,
        };
        let name = match cwd.and_then(|cwd| Path::new(path).strip_prefix(cwd).ok()) {
            Some(relative) if relative.as_os_str().is_empty() => ".".to_string(),
            Some(relative) => relative.to_string_lossy().into_owned(),
            None => path.clone(),
        };
        Shown { target, name }
    }
}

/// What checking a target found. Every diagnostic is located, with its
/// path written from the project's directory.
pub enum Checked {
    /// A file alone: its text, which its tests run on, and its check.
    File { source: String, report: CheckReport },
    /// A package, and its check.
    Package {
        package: Package,
        report: CheckReport,
    },
    /// Refused before it could be checked: a symlink, or a directory that
    /// is not a package.
    Refused(Vec<Diagnostic>),
}

impl Checked {
    /// Every diagnostic it found.
    pub fn diagnostics(self) -> Vec<Diagnostic> {
        match self {
            Checked::File { report, .. } | Checked::Package { report, .. } => report.diagnostics,
            Checked::Refused(diagnostics) => diagnostics,
        }
    }
}

/// Check the target as `oskac check` does, with `options`. Its files are
/// read as `oskac fix` reads them, never through a symlink: a file alone
/// that is a symlink is refused with `E0411`, as one in a package is, and
/// what it points at is never read. An error says what could not be read.
pub fn check(shown: &Shown, options: CheckOptions) -> Result<Checked, String> {
    match &shown.target {
        Target::File(path) => {
            if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                let refused = oska_check::symlink_refused(&shown.name);
                return Ok(Checked::Refused(vec![refused]));
            }
            let (_, source) = package::read_alone(path)
                .map_err(|e| format!("cannot read {}: {e}", shown.name))?;
            let mut report = oska_check::check_file(&shown.name, &source, options);
            report.diagnostics = report
                .diagnostics
                .into_iter()
                .map(|d| locate(&shown.name, &source, d))
                .collect();
            Ok(Checked::File { source, report })
        }
        Target::Dir(dir) => match package::load(dir, "check") {
            Loaded::Package { package, refused } => {
                let mut report = package::check(&package, &refused, options);
                report.diagnostics = report
                    .diagnostics
                    .into_iter()
                    .map(|d| crate::from_here(&shown.name, d))
                    .collect();
                Ok(Checked::Package { package, report })
            }
            Loaded::Refused(diagnostics) => Ok(Checked::Refused(
                diagnostics
                    .into_iter()
                    .map(|d| crate::from_here(&shown.name, d))
                    .collect(),
            )),
            Loaded::Unreadable(message) => Err(message),
        },
    }
}

/// `diagnostic`, reported for the file `name` whose text is `source`, with
/// its place.
pub fn locate(name: &str, source: &str, diagnostic: Diagnostic) -> Diagnostic {
    match diagnostic.location {
        Some(_) => diagnostic,
        None => diagnostic.locate(name, source),
    }
}

/// `text` on one line: a control character, a line break included,
/// becomes a space, so what a program writes cannot forge a line of the
/// hook's report.
pub fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `path:line:column: error[CODE]: message`, as `oskac check` prints a
/// diagnostic, on one line. A diagnostic with no place is the target's,
/// named `name`.
pub fn line(name: &str, diagnostic: &Diagnostic) -> String {
    let place = match &diagnostic.location {
        Some(location) => location.to_string(),
        None => name.to_string(),
    };
    one_line(&format!(
        "{place}: {}[{}]: {}",
        diagnostic.severity.name(),
        diagnostic.code,
        diagnostic.message
    ))
}

/// The first [`SHOWN`] of `diagnostics` as a JSON array, each in the shape
/// `oskac check --format=json` gives it.
pub fn json_list(diagnostics: &[Diagnostic]) -> String {
    let items: Vec<String> = diagnostics
        .iter()
        .take(SHOWN)
        .map(Diagnostic::to_json)
        .collect();
    format!("[{}]", items.join(","))
}
