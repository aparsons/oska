//! What the events share: a target with the name it is shown by, and the
//! one line a diagnostic is printed as.

use std::path::Path;

use oska_syntax::Diagnostic;

use crate::package::Target;

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
