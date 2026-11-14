//! `oskac fix [path] [--dry-run] [--placeholders] [--format=json]`: apply
//! the fixes that are safe to apply automatically to a source file or a
//! package, one at a time with a check after each (`fix/rounds.rs`), then
//! write each changed file once, replacing it in one step.
//!
//! Nothing is written unless every rule holds: every fix edits one of the
//! checked program's own files with edits that apply, the fixed program
//! is no wider in authority than the starting one, no file changed on
//! disk while `oskac fix` ran, and no file is reached through a symlink.
//! Files are read and replaced beneath a handle for the package's (or the
//! file's) directory without following a symlink (`package::Files`).

mod diff;
mod edits;
mod rounds;

use std::path::Path;
use std::process::ExitCode;

use oska_check::{CheckOptions, CheckReport, Package, MANIFEST_PATH};
use oska_syntax::{json_escape, Diagnostic};

use crate::package::{self, Files};
use crate::Flags;

/// What `oskac fix` was given, read from disk.
struct Target {
    /// The program as read, and as the check names its files.
    program: Package,
    /// The directory its files are read and replaced beneath.
    files: Files,
    /// The package directory as given, for a package: the human output's
    /// paths start from it.
    dir: Option<String>,
    /// The symlinks found in a package, refused (`E0411`): every check
    /// reports them first.
    refused: Vec<Diagnostic>,
}

/// `oskac fix [path]`.
pub fn command(path: Option<&str>, flags: Flags) -> ExitCode {
    let target = match package::target(path) {
        package::Target::File(file) => {
            let code = fix_target(read_file(&file), flags);
            crate::hint_package(&file, "fix");
            return code;
        }
        package::Target::Dir(dir) => read_package(&dir),
    };
    fix_target(target, flags)
}

/// How reading the target went.
enum Read {
    Target(Box<Target>),
    /// The package was refused before it could be checked: these say why.
    Refused(Vec<Diagnostic>, String),
    /// An error, already printed.
    Failed,
}

/// The single file at `path`. A symlink is refused, even one to a file
/// in the same directory: a fix never writes through one.
fn read_file(path: &str) -> Read {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            eprintln!(
                "error: refusing to fix `{path}`: it is a symlink, and `oskac fix` never \
                 writes through one; nothing was written"
            );
            return Read::Failed;
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            return Read::Failed;
        }
    }
    let as_path = Path::new(path);
    let (Some(parent), Some(name)) = (as_path.parent(), as_path.file_name()) else {
        eprintln!("error: cannot read {path}: it names no file");
        return Read::Failed;
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let name = name.to_string_lossy();
    let read = Files::open(parent).and_then(|files| {
        let bytes = files.read(&[&name])?;
        let text = String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok((files, text))
    });
    match read {
        Ok((files, text)) => Read::Target(Box::new(Target {
            program: Package::single_file(path, &text),
            files,
            dir: None,
            refused: Vec::new(),
        })),
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            Read::Failed
        }
    }
}

/// The package in `dir`.
fn read_package(dir: &str) -> Read {
    let (program, refused) = match package::load(dir, "fix") {
        package::Loaded::Package { package, refused } => (package, refused),
        package::Loaded::Refused(diagnostics) => {
            return Read::Refused(diagnostics, dir.to_string());
        }
        package::Loaded::Unreadable(message) => {
            eprintln!("error: {message}");
            return Read::Failed;
        }
    };
    match Files::open(Path::new(dir)) {
        Ok(files) => Read::Target(Box::new(Target {
            program,
            files,
            dir: Some(dir.to_string()),
            refused,
        })),
        Err(err) => {
            eprintln!("error: cannot read {dir}: {err}");
            Read::Failed
        }
    }
}

/// What a run printed and wrote, for the output.
struct Report<'r> {
    applied: &'r [rounds::Applied],
    /// The diagnostics left: of the fixed program, or of the program on
    /// disk when nothing was written.
    diagnostics: &'r [Diagnostic],
    /// The files written, or that would be with `--dry-run`, with their
    /// diff.
    files: Vec<(String, String)>,
    rounds: usize,
    written: bool,
    dir: Option<&'r str>,
}

fn fix_target(read: Read, flags: Flags) -> ExitCode {
    let target = match read {
        Read::Target(target) => target,
        Read::Refused(diagnostics, dir) => {
            let report = Report {
                applied: &[],
                diagnostics: &diagnostics,
                files: Vec::new(),
                rounds: 0,
                written: false,
                dir: Some(&dir),
            };
            print(&report, flags);
            return ExitCode::FAILURE;
        }
        Read::Failed => return ExitCode::FAILURE,
    };
    let refused = target.refused.clone();
    let check = |program: &Package| -> CheckReport {
        let mut report = oska_check::check_package(program, CheckOptions::default());
        if !refused.is_empty() {
            report.diagnostics.splice(0..0, refused.iter().cloned());
            report.program = None;
        }
        report
    };
    let options = rounds::Options {
        placeholders: flags.placeholders,
    };
    let outcome = rounds::fix(target.program.clone(), options, check);
    let dir = target.dir.as_deref();
    let mut report = Report {
        applied: &outcome.applied,
        diagnostics: &outcome.report.diagnostics,
        files: Vec::new(),
        rounds: outcome.rounds,
        written: false,
        dir,
    };
    if let Some(refusal) = &outcome.refusal {
        print(&report, flags);
        eprintln!("error: {}; nothing was written", describe(refusal, dir));
        return ExitCode::FAILURE;
    }
    let changed = changed_files(&target.program, &outcome.program);
    report.files = changed
        .iter()
        .map(|(path, old, new)| {
            let shown = shown_path(dir, path, flags.json);
            (path.to_string(), diff::unified(&shown, old, new))
        })
        .collect();
    if !flags.dry_run && !changed.is_empty() {
        match write(&target.files, &target.program, &changed) {
            Ok(()) => report.written = true,
            Err(refusal) => {
                // Nothing was written, so what remains is what was there.
                let start = check(&target.program);
                let report = Report {
                    applied: &[],
                    diagnostics: &start.diagnostics,
                    files: Vec::new(),
                    rounds: outcome.rounds,
                    written: false,
                    dir,
                };
                print(&report, flags);
                eprintln!("error: {refusal}");
                return ExitCode::FAILURE;
            }
        }
    }
    print(&report, flags);
    // Either way a diagnostic with a fix remains, so the exit code is 1.
    match &outcome.stop {
        Some(rounds::Stop::Stuck(diagnostic)) => eprintln!(
            "note: stopped at {}: the fix for {} did not remove its diagnostic, so it was not \
             applied",
            located(diagnostic, dir),
            diagnostic.code
        ),
        Some(rounds::Stop::Cap) => eprintln!(
            "note: stopped after {} rounds; run `oskac fix` again to go on",
            rounds::MAX_ROUNDS
        ),
        None => {}
    }
    if !outcome.report.diagnostics.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Each file of `fixed` whose text differs from `start`'s: its path, as
/// the report names it, and both texts. The manifest comes first, then the
/// modules in package order.
fn changed_files<'p>(start: &'p Package, fixed: &'p Package) -> Vec<(&'p str, &'p str, &'p str)> {
    let mut changed = Vec::new();
    if let (Some(old), Some(new)) = (&start.manifest, &fixed.manifest) {
        if old != new {
            changed.push((MANIFEST_PATH, old.as_str(), new.as_str()));
        }
    }
    for (old, new) in start.modules.iter().zip(&fixed.modules) {
        if old.source != new.source {
            changed.push((old.path.as_str(), old.source.as_str(), new.source.as_str()));
        }
    }
    changed
}

/// The names `path`, as the report names it, reaches beneath the
/// directory of `files`: the file's own name for a single file, whose
/// report path is as given on the command line.
fn names_of(program: &Package, path: &str) -> Vec<String> {
    if program.manifest.is_none() && program.modules.len() == 1 {
        let name = Path::new(path)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        return vec![name];
    }
    path.split('/').map(str::to_string).collect()
}

/// Write each of `changed`, a file of `start` and its new text, once,
/// replacing it in one step: or nothing, saying which rule refused it.
fn write(files: &Files, start: &Package, changed: &[(&str, &str, &str)]) -> Result<(), String> {
    // A file that changed since it was read: the fixes were made to text
    // that is no longer there, and writing would lose the change.
    let every_file = start
        .manifest
        .iter()
        .map(|m| (MANIFEST_PATH, m.as_str()))
        .chain(
            start
                .modules
                .iter()
                .map(|m| (m.path.as_str(), m.source.as_str())),
        );
    for (path, text) in every_file {
        let names = names_of(start, path);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        match files.read(&names) {
            Ok(bytes) if bytes == text.as_bytes() => {}
            Ok(_) => {
                return Err(format!(
                    "refusing to write: `{path}` changed on disk while `oskac fix` ran; nothing \
                     was written"
                ));
            }
            Err(err) => {
                return Err(format!(
                    "refusing to write: cannot read `{path}` again: {err}; nothing was written"
                ));
            }
        }
    }
    let mut staged = Vec::new();
    for (path, _, new) in changed {
        let names = names_of(start, path);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        match files.stage(&names, new.as_bytes()) {
            Ok(file) => staged.push(file),
            Err(err) => {
                return Err(format!(
                    "refusing to write `{path}`: {err}; nothing was written"
                ));
            }
        }
    }
    let mut written = Vec::new();
    for (file, (path, _, _)) in staged.into_iter().zip(changed) {
        if let Err(err) = file.commit() {
            let before = if written.is_empty() {
                "nothing was written".to_string()
            } else {
                format!("only {} were written", written.join(", "))
            };
            return Err(format!("cannot write `{path}`: {err}; {before}"));
        }
        written.push(format!("`{path}`"));
    }
    Ok(())
}

/// A refusal of the run, said with the rule it broke.
fn describe(refusal: &rounds::Refusal, dir: Option<&str>) -> String {
    match refusal {
        rounds::Refusal::Outside { diagnostic, path } => format!(
            "refusing the fix for {} at {}: it edits `{path}`, which is not a file of the checked \
             program",
            diagnostic.code,
            located(diagnostic, dir)
        ),
        rounds::Refusal::Edits { diagnostic, error } => format!(
            "refusing the fix for {} at {}: {}",
            diagnostic.code,
            located(diagnostic, dir),
            error.describe()
        ),
        rounds::Refusal::Widening(widenings) => format!(
            "refusing to write: the fixed program's authority is wider than before: {}",
            widenings.join("; ")
        ),
    }
}

/// Where `diagnostic` is, as `path:line:column` from the current
/// directory.
fn located(diagnostic: &Diagnostic, dir: Option<&str>) -> String {
    let diagnostic = match dir {
        Some(dir) => crate::from_here(dir, diagnostic.clone()),
        None => diagnostic.clone(),
    };
    diagnostic
        .location
        .map_or_else(|| diagnostic.code.to_string(), |l| l.to_string())
}

/// `path`, as the report names it, as the output shows it: package-relative
/// in JSON, from the current directory otherwise.
fn shown_path(dir: Option<&str>, path: &str, json: bool) -> String {
    match dir {
        Some(dir) if !json => package::from_here(dir, path),
        _ => path.to_string(),
    }
}

/// Print `report`: one line per applied fix on stdout, then the diff with
/// `--dry-run`, and the remaining diagnostics on stderr as `oskac check`
/// prints them; or one JSON object on stdout with `--format=json`.
fn print(report: &Report, flags: Flags) {
    if flags.json {
        out!("{}", to_json(report, flags.dry_run));
        return;
    }
    for applied in report.applied {
        outln!(
            "{}: fixed {}: {}",
            located(&applied.diagnostic, report.dir),
            applied.diagnostic.code,
            applied.message
        );
    }
    if flags.dry_run {
        for (_, diff) in &report.files {
            out!("{diff}");
        }
    }
    let remaining: Vec<Diagnostic> = report
        .diagnostics
        .iter()
        .map(|d| match report.dir {
            Some(dir) => crate::from_here(dir, d.clone()),
            None => d.clone(),
        })
        .collect();
    crate::print_diagnostics(&remaining);
}

/// The JSON object `oskac fix --format=json` prints.
fn to_json(report: &Report, dry_run: bool) -> String {
    let applied: Vec<String> = report
        .applied
        .iter()
        .map(|a| {
            format!(
                "{{\"code\":\"{}\",\"message\":\"{}\",\"path\":\"{}\",\"applicability\":\"{}\"}}",
                a.diagnostic.code,
                json_escape(&a.message),
                json_escape(&a.path),
                a.applicability.name()
            )
        })
        .collect();
    let files: Vec<String> = report
        .files
        .iter()
        .map(|(path, _)| format!("\"{}\"", json_escape(path)))
        .collect();
    let diagnostics: Vec<String> = report.diagnostics.iter().map(Diagnostic::to_json).collect();
    let mut out = format!(
        "{{\n  \"applied\": {},\n  \"files\": [{}],\n  \"diagnostics\": {},\n  \"rounds\": {},\n  \"written\": {}",
        json_lines(&applied),
        files.join(","),
        json_lines(&diagnostics),
        report.rounds,
        report.written
    );
    if dry_run {
        let diffs: Vec<String> = report
            .files
            .iter()
            .map(|(path, diff)| {
                format!(
                    "{{\"path\":\"{}\",\"diff\":\"{}\"}}",
                    json_escape(path),
                    json_escape(diff)
                )
            })
            .collect();
        out.push_str(&format!(",\n  \"diff\": {}", json_lines(&diffs)));
    }
    out.push_str("\n}\n");
    out
}

/// A JSON array of `items`, one per line, as `oskac check` prints its
/// report's arrays.
fn json_lines(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n    {}\n  ]", items.join(",\n    "))
    }
}

#[cfg(test)]
mod tests {
    use super::write;
    use crate::package::Files;
    use oska_check::Package;
    use std::path::PathBuf;

    /// A fresh directory with `app.osk` holding `text`, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str, text: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("oskac-fix-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            std::fs::write(dir.join("app.osk"), text).expect("the file");
            Scratch(dir)
        }

        fn app(&self) -> String {
            std::fs::read_to_string(self.0.join("app.osk")).expect("the file")
        }

        /// The names of the directory's entries, sorted.
        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .expect("the directory")
                .map(|e| {
                    e.expect("an entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_file_is_replaced_with_its_new_text_and_nothing_else_is_left() {
        let dir = Scratch::new("written", "a;");
        let files = Files::open(&dir.0).expect("the directory");
        let start = Package::single_file("app.osk", "a;");
        assert_eq!(write(&files, &start, &[("app.osk", "a;", "a")]), Ok(()));
        assert_eq!(dir.app(), "a");
        assert_eq!(dir.entries(), ["app.osk"]);
    }

    #[test]
    fn a_file_that_changed_on_disk_meanwhile_is_not_overwritten() {
        let dir = Scratch::new("changed", "a; // edited meanwhile");
        let files = Files::open(&dir.0).expect("the directory");
        let start = Package::single_file("app.osk", "a;");
        let refused = write(&files, &start, &[("app.osk", "a;", "a")]);
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.contains("changed on disk")),
            "{refused:?}"
        );
        assert_eq!(dir.app(), "a; // edited meanwhile");
        assert_eq!(dir.entries(), ["app.osk"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_swapped_in_meanwhile_is_not_written_through() {
        let dir = Scratch::new("swapped", "a;");
        let files = Files::open(&dir.0).expect("the directory");
        let start = Package::single_file("app.osk", "a;");
        std::fs::rename(dir.0.join("app.osk"), dir.0.join("real.osk")).expect("a rename");
        std::os::unix::fs::symlink("real.osk", dir.0.join("app.osk")).expect("a symlink");
        let refused = write(&files, &start, &[("app.osk", "a;", "a")]);
        assert!(
            refused.as_ref().is_err_and(|e| e.contains("symlink")),
            "{refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.0.join("real.osk")).expect("the target"),
            "a;"
        );
        assert!(std::fs::symlink_metadata(dir.0.join("app.osk"))
            .expect("the link")
            .file_type()
            .is_symlink());
    }
}
