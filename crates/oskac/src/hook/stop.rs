//! The Stop event: when the agent tries to finish, run the release check
//! and then the tests on each target, and block it while any fails.
//!
//! The targets are the ones named on the command line, as paths from the
//! project's directory, or, with none, the ones found beneath it: its
//! package if it is one; otherwise every package and every `.osk` file
//! outside a package's `src/`, skipping hidden directories and following
//! no symlink. The check is `oskac check --release`; the tests are
//! `oskac test` without `--integration`, run on the interpreter, in this
//! process, each on stand-in capabilities and a fresh in-memory world: the
//! gate touches no file, starts no program and reaches no real resource.
//!
//! Any diagnostic or failed test blocks, on every attempt. A test run that
//! the interpreter refuses (`oskac test` exit status 2) does not: it is a
//! limit of the toolchain, not of the program, so the gate lets the agent
//! finish and tells the user.

use std::path::{Path, PathBuf};

use oska_check::{CheckOptions, Locator};
use oska_run::{codes, FakeHost, TestOptions, TestReport, TestStatus};
use oska_syntax::Diagnostic;

use super::target::{line, one_line, Shown, SHOWN};
use super::{Finish, Verdict};
use crate::package::{self, Loaded, Target};

/// What the gate says about the agent finishing.
pub fn verdict_for(finish: &Finish, named: &[&str]) -> Verdict {
    let root = finish.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
    let targets = match targets(&root, named, finish.cwd.is_some()) {
        Ok(targets) => targets,
        Err(reason) => return Verdict::failed(&reason),
    };
    let mut outcomes = Vec::new();
    let mut unreadable = Vec::new();
    for target in targets {
        let shown = Shown::new(target, Some(&root));
        match run(&shown) {
            Ok(outcome) => outcomes.push((shown.name, outcome)),
            Err(reason) => unreadable.push(reason),
        }
    }
    verdict(&outcomes, &unreadable)
}

// ---- Targets ----------------------------------------------------------------------------------

/// The targets to run: `named`, each a path from `root` (the project's
/// directory, when the input gave one), or what is found beneath `root`.
fn targets(root: &Path, named: &[&str], rooted: bool) -> Result<Vec<Target>, String> {
    if named.is_empty() {
        return discover(root);
    }
    Ok(named
        .iter()
        .map(|name| {
            let path = if rooted {
                root.join(name)
            } else {
                PathBuf::from(name)
            };
            package::target(Some(&path.to_string_lossy()))
        })
        .collect())
}

/// Whether `dir` is a package: it holds an `oska.toml` that is a regular
/// file, never a symlink.
fn is_package(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(oska_check::MANIFEST_PATH)).is_ok_and(|m| m.is_file())
}

/// The package in `root`, if it is one; otherwise every package and every
/// `.osk` file outside a package's `src/` beneath it, in name order.
fn discover(root: &Path) -> Result<Vec<Target>, String> {
    let mut found = Vec::new();
    if is_package(root) {
        found.push(Target::Dir(root.to_string_lossy().into_owned()));
    } else {
        walk(root, &mut found)?;
    }
    Ok(found)
}

/// Collect what is beneath `dir`, skipping hidden entries and symlinks,
/// and the `src/` of a package, which its package covers.
fn walk(dir: &Path, found: &mut Vec<Target>) -> Result<(), String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    names.sort();
    let in_package = is_package(dir);
    for name in names {
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = dir.join(&name);
        let kind = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?
            .file_type();
        if kind.is_dir() {
            if is_package(&path) {
                found.push(Target::Dir(path.to_string_lossy().into_owned()));
            }
            if !(in_package && name == "src") {
                walk(&path, found)?;
            }
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "osk") {
            found.push(Target::File(path.to_string_lossy().into_owned()));
        }
    }
    Ok(())
}

// ---- Running a target -------------------------------------------------------------------------

/// What running one target found.
#[derive(Default)]
struct Outcome {
    /// What the release check reported. When there is any, the tests did
    /// not run: there is no program to run them on.
    check: Vec<Diagnostic>,
    /// The tests that failed, by name, with their failures.
    failed: Vec<(String, Diagnostic)>,
    /// What the interpreter refused to run, so no test did.
    unrunnable: Vec<Diagnostic>,
}

impl Outcome {
    fn blocks(&self) -> bool {
        !self.check.is_empty() || !self.failed.is_empty()
    }

    /// The outcome of `report`, whose failures are located already.
    fn tested(report: TestReport) -> Outcome {
        let mut outcome = Outcome::default();
        for test in &report.tests {
            if test.status == TestStatus::Failed {
                if let Some(failure) = &test.failure {
                    outcome.failed.push((test.label(), failure.clone()));
                }
            }
        }
        for diagnostic in report.diagnostics {
            if codes::ALL.contains(&diagnostic.code) {
                outcome.unrunnable.push(diagnostic);
            } else {
                outcome.check.push(diagnostic);
            }
        }
        outcome
    }
}

/// Check the target for release and, when that is clean, run its tests.
/// An error says what could not be read.
fn run(shown: &Shown) -> Result<Outcome, String> {
    let options = CheckOptions { release: true };
    let tests = TestOptions { integration: false };
    // Stand-ins only: the host given to a test run is used for an
    // `#[integration]` test, which does not run, and for nothing else.
    let mut host = FakeHost::new();
    match &shown.target {
        Target::File(path) => {
            let src = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", shown.name))?;
            let locate = |d: Diagnostic| match d.location {
                Some(_) => d,
                None => d.locate(&shown.name, &src),
            };
            let report = oska_check::check_file(&shown.name, &src, options);
            if !report.diagnostics.is_empty() {
                let check = report.diagnostics.into_iter().map(locate).collect();
                return Ok(Outcome {
                    check,
                    ..Outcome::default()
                });
            }
            let mut report = oska_run::test(&src, &mut host, tests);
            for test in &mut report.tests {
                test.failure = test.failure.take().map(locate);
            }
            report.diagnostics = report.diagnostics.into_iter().map(locate).collect();
            Ok(Outcome::tested(report))
        }
        Target::Dir(dir) => {
            let from_here = |mut d: Diagnostic| {
                if let Some(location) = &mut d.location {
                    location.path = package::from_here(&shown.name, &location.path);
                }
                d
            };
            let (package, refused) = match package::load(dir, "check") {
                Loaded::Package { package, refused } => (package, refused),
                Loaded::Refused(diagnostics) => {
                    let check = diagnostics.into_iter().map(from_here).collect();
                    return Ok(Outcome {
                        check,
                        ..Outcome::default()
                    });
                }
                Loaded::Unreadable(message) => return Err(message),
            };
            let mut report = oska_check::check_package(&package, options);
            report.diagnostics.splice(0..0, refused);
            let program = match report.program {
                Some(program) if report.diagnostics.is_empty() => program,
                _ => {
                    let check = report.diagnostics.into_iter().map(from_here).collect();
                    return Ok(Outcome {
                        check,
                        ..Outcome::default()
                    });
                }
            };
            let locator = Locator::new(&package);
            let mut report = oska_run::test_program(&program, &mut host, tests);
            for test in &mut report.tests {
                test.module = locator.module_of(test.span).map(str::to_string);
                test.failure = test.failure.take().map(|d| from_here(locator.locate(d)));
            }
            report.diagnostics = report
                .diagnostics
                .into_iter()
                .map(|d| from_here(locator.locate(d)))
                .collect();
            Ok(Outcome::tested(report))
        }
    }
}

// ---- The verdict ------------------------------------------------------------------------------

/// What to print, and the exit status, given each target's outcome, by
/// the name it is shown by, and what could not be read. Any diagnostic or
/// failed test blocks, with a line each (at most [`SHOWN`] of each kind
/// per target, then a count). With none, a target that could not be read
/// ends the hook with one line and a status that blocks nothing, and a
/// target the interpreter cannot run is told to the user only.
fn verdict(outcomes: &[(String, Outcome)], unreadable: &[String]) -> Verdict {
    if outcomes.iter().any(|(_, outcome)| outcome.blocks()) {
        let mut out = String::new();
        for (name, outcome) in outcomes.iter().filter(|(_, o)| o.blocks()) {
            for diagnostic in outcome.check.iter().take(SHOWN) {
                out.push_str(&line(name, diagnostic));
                out.push('\n');
            }
            if outcome.check.len() > SHOWN {
                out.push_str(&format!("and {} more\n", outcome.check.len() - SHOWN));
            }
            for (label, failure) in outcome.failed.iter().take(SHOWN) {
                out.push_str(&one_line(&format!("test {label}: {}", line(name, failure))));
                out.push('\n');
            }
            if outcome.failed.len() > SHOWN {
                out.push_str(&format!(
                    "and {} more failed tests\n",
                    outcome.failed.len() - SHOWN
                ));
            }
        }
        for reason in unreadable {
            out.push_str(&format!("error: {}\n", one_line(reason)));
        }
        out.push_str("the release check or a test failed: fix it before you finish\n");
        return Verdict::feedback(out);
    }
    if let Some(reason) = unreadable.first() {
        return Verdict::failed(reason);
    }
    let unrunnable: Vec<String> = outcomes
        .iter()
        .flat_map(|(name, outcome)| {
            outcome
                .unrunnable
                .iter()
                .map(move |d| one_line(&format!("{name}: [{}] {}", d.code, d.message)))
        })
        .collect();
    if unrunnable.is_empty() {
        return Verdict::quiet();
    }
    Verdict::noting(format!(
        "oskac stop gate: tests not run, the interpreter cannot run them yet ({})",
        unrunnable.join("; ")
    ))
}
