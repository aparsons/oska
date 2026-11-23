// SPDX-License-Identifier: Apache-2.0
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
//! gate writes no file, starts no program and reaches no real resource.
//! The only files it reads besides the source are the snapshots the tests'
//! snapshot assertions compare with, as `oskac test` does without
//! `--update` (`oska_run::snapshots`), and it reaches none of them through
//! a symlink: a symlinked `snapshots/`, directory in it, or snapshot fails
//! the test.
//!
//! Any diagnostic or failed test blocks, on every attempt, and so does a
//! target or directory that cannot be read: the gate never lets the agent
//! through on what it could not check. A test run that the interpreter
//! refuses (`oskac test` exit status 2) does not: it is a limit of the
//! toolchain, not of the program, so the gate lets the agent finish and
//! tells the user.

use std::path::{Path, PathBuf};

use oska_check::{CheckOptions, Locator};
use oska_run::{codes, FakeHost, TestOptions, TestReport, TestStatus};
use oska_syntax::{json_escape, Diagnostic};

use super::target::{self, json_list, line, one_line, Checked, Shown, SHOWN};
use super::{Finish, Format, Verdict};
use crate::package::{self, Target};

/// What the gate says about the agent finishing, written as `format` says.
pub fn verdict_for(finish: &Finish, named: &[&str], format: Format) -> Verdict {
    let root = finish.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut unreadable = Vec::new();
    let targets = targets(&root, named, finish.cwd.is_some(), &mut unreadable);
    let mut outcomes = Vec::new();
    for target in targets {
        let shown = Shown::new(target, Some(&root));
        match run(&shown) {
            Ok(outcome) => outcomes.push((shown.name, outcome)),
            Err(reason) => unreadable.push(reason),
        }
    }
    verdict(&outcomes, &unreadable, format)
}

// ---- Targets ----------------------------------------------------------------------------------

/// The targets to run: `named`, each a path from `root` (the project's
/// directory, when the input gave one), or what is found beneath `root`.
/// What could not be read while finding them is added to `unreadable`.
fn targets(root: &Path, named: &[&str], rooted: bool, unreadable: &mut Vec<String>) -> Vec<Target> {
    if named.is_empty() {
        return discover(root, unreadable);
    }
    named
        .iter()
        .map(|name| {
            let path = if rooted {
                root.join(name)
            } else {
                PathBuf::from(name)
            };
            package::target(Some(&path.to_string_lossy()))
        })
        .collect()
}

/// Whether `dir` is a package: it holds an `oska.toml` that is a regular
/// file, never a symlink.
fn is_package(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(oska_check::MANIFEST_PATH)).is_ok_and(|m| m.is_file())
}

/// The package in `root`, if it is one; otherwise every package and every
/// `.osk` file outside a package's `src/` beneath it, in name order. What
/// cannot be read is added to `unreadable` and the walk goes on, so every
/// other target is still checked, and the gate blocks.
fn discover(root: &Path, unreadable: &mut Vec<String>) -> Vec<Target> {
    let mut found = Vec::new();
    if is_package(root) {
        found.push(Target::Dir(root.to_string_lossy().into_owned()));
    } else {
        walk(root, &mut found, unreadable);
    }
    found
}

/// Collect what is beneath `dir`, skipping hidden entries and symlinks,
/// and the `src/` of a package, which its package covers.
fn walk(dir: &Path, found: &mut Vec<Target>, unreadable: &mut Vec<String>) {
    let cannot_read =
        |path: &Path, e: std::io::Error| format!("cannot read {}: {e}", path.display());
    let listed = std::fs::read_dir(dir).and_then(|entries| {
        entries
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, _>>()
    });
    let mut names = match listed {
        Ok(names) => names,
        Err(e) => return unreadable.push(cannot_read(dir, e)),
    };
    names.sort();
    let in_package = is_package(dir);
    for name in names {
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = dir.join(&name);
        let kind = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta.file_type(),
            Err(e) => {
                unreadable.push(cannot_read(&path, e));
                continue;
            }
        };
        if kind.is_dir() {
            if is_package(&path) {
                found.push(Target::Dir(path.to_string_lossy().into_owned()));
            }
            if !(in_package && name == "src") {
                walk(&path, found, unreadable);
            }
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "osk") {
            found.push(Target::File(path.to_string_lossy().into_owned()));
        }
    }
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

    /// The outcome of a release check that reported `check`.
    fn checked(check: Vec<Diagnostic>) -> Outcome {
        Outcome {
            check,
            ..Outcome::default()
        }
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
    let tests = TestOptions {
        integration: false,
        filter: None,
    };
    // Stand-ins only: the host given to a test run is used for an
    // `#[integration]` test, which does not run, and for nothing else.
    let mut host = FakeHost::new();
    match target::check(shown, CheckOptions { release: true })? {
        Checked::Refused(check) => Ok(Outcome::checked(check)),
        Checked::File { report, .. } if !report.diagnostics.is_empty() => {
            Ok(Outcome::checked(report.diagnostics))
        }
        Checked::File { source, .. } => {
            let locate = |d| target::locate(&shown.name, &source, d);
            let mut report = oska_run::test(&source, &mut host, tests);
            let (dir, stem) = crate::file_snapshots(path_of(shown));
            crate::settle_in(&mut report, &dir, &|_| Some(stem.clone()), false);
            for test in &mut report.tests {
                test.failure = test.failure.take().map(locate);
            }
            report.diagnostics = report.diagnostics.into_iter().map(locate).collect();
            Ok(Outcome::tested(report))
        }
        Checked::Package { package, report } => {
            let program = match report.program {
                Some(program) if report.diagnostics.is_empty() => program,
                _ => return Ok(Outcome::checked(report.diagnostics)),
            };
            let locator = Locator::new(&package);
            let locate = |d| crate::from_here(&shown.name, locator.locate(d));
            let mut report = oska_run::test_program(&program, &mut host, tests);
            let file_of = |span| crate::package_test_file(&locator, span);
            crate::settle_in(&mut report, path_of(shown), &file_of, false);
            for test in &mut report.tests {
                test.module = locator.module_of(test.span).map(str::to_string);
                test.failure = test.failure.take().map(locate);
            }
            report.diagnostics = report.diagnostics.into_iter().map(locate).collect();
            Ok(Outcome::tested(report))
        }
    }
}

/// The path of `shown`'s file or package directory, which its snapshots
/// are found from (`oska_run::snapshots`).
fn path_of(shown: &Shown) -> &str {
    match &shown.target {
        Target::File(path) | Target::Dir(path) => path,
    }
}

// ---- The verdict ------------------------------------------------------------------------------

/// What to print, and the exit status, given each target's outcome, by
/// the name it is shown by, and what could not be read. Any diagnostic,
/// failed test, or target or directory that could not be read blocks,
/// with what blocks, as text or JSON as `format` says: a gate that could
/// not check everything has not shown that the work is done, so it never
/// lets the agent through. With none, a target the interpreter cannot run
/// is told to the user only.
fn verdict(outcomes: &[(String, Outcome)], unreadable: &[String], format: Format) -> Verdict {
    let failed = outcomes.iter().any(|(_, outcome)| outcome.blocks());
    if failed || !unreadable.is_empty() {
        let blocking: Vec<&(String, Outcome)> =
            outcomes.iter().filter(|(_, o)| o.blocks()).collect();
        return Verdict::feedback(match format {
            Format::Text => blocked_text(&blocking, unreadable, failed),
            Format::Json => blocked_json(&blocking, unreadable),
        });
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

/// The blocking message as text: a line per diagnostic and failed test
/// (at most [`SHOWN`] of each kind per target, then a count), a line per
/// target that could not be read, and what to do.
fn blocked_text(blocking: &[&(String, Outcome)], unreadable: &[String], failed: bool) -> String {
    let mut out = String::new();
    for (name, outcome) in blocking {
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
    if failed {
        out.push_str("the release check or a test failed: fix it before you finish\n");
    }
    if !unreadable.is_empty() {
        out.push_str(
            "the stop gate could not check every target: make it readable, or name the \
             targets in the hook's settings, before you finish\n",
        );
    }
    out
}

/// The blocking message as one JSON object: each blocking target with its
/// diagnostics and failed tests (at most [`SHOWN`] of each, then a count),
/// each diagnostic in the shape `oskac check --format=json` gives it, and
/// what could not be read.
fn blocked_json(blocking: &[&(String, Outcome)], unreadable: &[String]) -> String {
    let targets: Vec<String> = blocking
        .iter()
        .map(|(name, outcome)| {
            let failed: Vec<String> = outcome
                .failed
                .iter()
                .take(SHOWN)
                .map(|(label, failure)| {
                    format!(
                        "{{\"name\":\"{}\",\"failure\":{}}}",
                        json_escape(label),
                        failure.to_json()
                    )
                })
                .collect();
            format!(
                "{{\"target\":\"{}\",\"diagnostics\":{},\"more\":{},\"failed_tests\":[{}],\
                 \"more_failed_tests\":{}}}",
                json_escape(name),
                json_list(&outcome.check),
                outcome.check.len().saturating_sub(SHOWN),
                failed.join(","),
                outcome.failed.len().saturating_sub(SHOWN)
            )
        })
        .collect();
    let errors: Vec<String> = unreadable
        .iter()
        .map(|reason| format!("\"{}\"", json_escape(reason)))
        .collect();
    format!(
        "{{\"event\":\"stop\",\"targets\":[{}],\"errors\":[{}]}}\n",
        targets.join(","),
        errors.join(",")
    )
}
