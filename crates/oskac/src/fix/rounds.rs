//! The rounds of `oskac fix`, in memory: check, apply the first fix that
//! may be applied, check again, and repeat, until no such fix is left, a
//! fix leaves its own diagnostic in place, or the cap on rounds is
//! reached. Then the fixed program's authority is compared with the
//! starting program's, and any widening refuses the whole run.
//!
//! The checker is a parameter, so the rules are the same whatever checks
//! the program: `oskac fix` passes `oska_check::check_package`.

use oska_check::authority_diff::{diff, AuthorityRecord};
use oska_check::{CheckReport, Package, MANIFEST_PATH};
use oska_syntax::{Applicability, Diagnostic, Span};

use super::edits::{self, EditError};

/// The most rounds one run makes. Each round applies one fix and checks
/// the whole program again, so a faulty fix that keeps making new
/// diagnostics cannot loop forever.
pub const MAX_ROUNDS: usize = 100;

/// Which fixes a run applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Fixes whose `applicability` is `placeholders` too, such as
    /// `E0253`'s `todo()` arms. `maybe` fixes never are.
    pub placeholders: bool,
}

/// A fix applied in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The diagnostic it fixed, located in the program as it was then.
    pub diagnostic: Diagnostic,
    /// The file its edits are in, as the report names it.
    pub path: String,
    pub message: String,
    pub applicability: Applicability,
}

/// Why a run stopped before every fix it may apply was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The fix of this diagnostic left it in place, so the fix was undone.
    Stuck(Diagnostic),
    /// [`MAX_ROUNDS`] rounds were made.
    Cap,
}

/// Why nothing may be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The fix of this diagnostic edits a file that is not one of the
    /// checked program's: `path`.
    Outside {
        diagnostic: Diagnostic,
        path: String,
    },
    /// The fix of this diagnostic has edits that cannot be applied.
    Edits {
        diagnostic: Diagnostic,
        error: EditError,
    },
    /// The fixed program can do more than the starting one: each entry
    /// says what it gained.
    Widening(Vec<String>),
}

/// What a run did.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The fixes applied, in order. Empty when the run was refused.
    pub applied: Vec<Applied>,
    /// The program after them: the starting one when the run was refused.
    pub program: Package,
    /// The check of `program`.
    pub report: CheckReport,
    /// How many fixes were applied in memory and checked, including one
    /// that was undone.
    pub rounds: usize,
    pub stop: Option<Stop>,
    pub refusal: Option<Refusal>,
}

/// Fix `start` with `check`, applying the fixes `options` allows.
pub fn fix(
    start: Package,
    options: Options,
    mut check: impl FnMut(&Package) -> CheckReport,
) -> Outcome {
    let start_report = check(&start);
    // The authority the fixes are compared with: the report of the first
    // program in the run whose every module parses. A module that does not
    // parse reports no functions, and only its parse error, so the fixes
    // made until it parses are syntax fixes; comparing with its empty
    // report would count every function as new.
    let mut baseline = parses(&start).then(|| start_report.clone());
    let mut program = start.clone();
    let mut report = start_report.clone();
    let mut applied = Vec::new();
    let mut rounds = 0;
    let mut stop = None;
    let refused = |refusal: Refusal, rounds: usize| Outcome {
        applied: Vec::new(),
        program: start.clone(),
        report: start_report.clone(),
        rounds,
        stop: None,
        refusal: Some(refusal),
    };
    while let Some(diagnostic) = first_applicable(&report, options) {
        let Some(fix) = &diagnostic.fix else {
            break;
        };
        if rounds == MAX_ROUNDS {
            stop = Some(Stop::Cap);
            break;
        }
        rounds += 1;
        let own_path = diagnostic.location.as_ref().map(|l| l.path.clone());
        let path = fix.path.clone().or_else(|| own_path.clone());
        let Some((path, text)) = path.and_then(|p| text_of(&program, &p).map(|t| (p, t))) else {
            let path = fix.path.clone().or(own_path).unwrap_or_default();
            return refused(
                Refusal::Outside {
                    diagnostic: diagnostic.clone(),
                    path,
                },
                rounds,
            );
        };
        let fixed = match edits::apply(text, &fix.edits) {
            Ok(fixed) => fixed,
            Err(error) => {
                let diagnostic = diagnostic.clone();
                return refused(Refusal::Edits { diagnostic, error }, rounds);
            }
        };
        let mut next = program.clone();
        set_text(&mut next, &path, fixed);
        let next_report = check(&next);
        let moved = if own_path.as_deref() == Some(path.as_str()) {
            Span {
                start: edits::moved(diagnostic.span.start, &fix.edits),
                end: edits::moved(diagnostic.span.end, &fix.edits),
            }
        } else {
            diagnostic.span
        };
        if next_report.diagnostics.iter().any(|d| {
            d.code == diagnostic.code
                && d.span == moved
                && d.location.as_ref().map(|l| &l.path) == own_path.as_ref()
        }) {
            stop = Some(Stop::Stuck(diagnostic.clone()));
            break;
        }
        applied.push(Applied {
            diagnostic: diagnostic.clone(),
            path,
            message: fix.message.clone(),
            applicability: fix.applicability,
        });
        program = next;
        report = next_report;
        if baseline.is_none() && parses(&program) {
            baseline = Some(report.clone());
        }
    }
    if let Some(baseline) = &baseline {
        // Two functions of one name could hide a widening: that refuses
        // the run too.
        let widened = match diff(
            &AuthorityRecord::of(baseline),
            &AuthorityRecord::of(&report),
        ) {
            Ok(difference) => difference.widenings(),
            Err(message) => vec![message],
        };
        if !widened.is_empty() {
            return refused(Refusal::Widening(widened), rounds);
        }
    }
    Outcome {
        applied,
        program,
        report,
        rounds,
        stop,
        refusal: None,
    }
}

/// The first diagnostic, in the order the check reports them, whose fix
/// `options` allows.
fn first_applicable(report: &CheckReport, options: Options) -> Option<Diagnostic> {
    report
        .diagnostics
        .iter()
        .find(|d| {
            d.fix.as_ref().is_some_and(|fix| match fix.applicability {
                Applicability::Machine => true,
                Applicability::Placeholders => options.placeholders,
                Applicability::Maybe => false,
            })
        })
        .cloned()
}

/// Every module of `program` parses.
fn parses(program: &Package) -> bool {
    program
        .modules
        .iter()
        .all(|module| oska_syntax::parse_module(&module.source).is_ok())
}

/// The text of the file of `program` at `path`, as the report names it:
/// a module's file, or the manifest of a package that has one.
pub fn text_of<'p>(program: &'p Package, path: &str) -> Option<&'p str> {
    if path == MANIFEST_PATH {
        if let Some(manifest) = &program.manifest {
            return Some(manifest);
        }
    }
    program
        .modules
        .iter()
        .find(|module| module.path == path)
        .map(|module| module.source.as_str())
}

/// Replace the text of the file at `path`, which [`text_of`] found.
fn set_text(program: &mut Package, path: &str, text: String) {
    if path == MANIFEST_PATH && program.manifest.is_some() {
        program.manifest = Some(text);
    } else if let Some(module) = program.modules.iter_mut().find(|m| m.path == path) {
        module.source = text;
    }
}

#[cfg(test)]
mod tests {
    use super::{fix, Options, Refusal, Stop, MAX_ROUNDS};
    use crate::fix::edits::EditError;
    use oska_check::{CheckReport, Package};
    use oska_syntax::{Diagnostic, Edit, Span};

    /// A stand-in check that reports the first `;` of the file, with a
    /// fix made by `fix_of` from the `;`'s offset: the file it names, if
    /// not the diagnostic's own, and its edits.
    fn semicolons(
        fix_of: impl Fn(usize) -> (Option<&'static str>, Vec<Edit>),
    ) -> impl FnMut(&Package) -> CheckReport {
        move |program: &Package| {
            let module = &program.modules[0];
            let diagnostics = module
                .source
                .find(';')
                .map(|at| {
                    let (path, edits) = fix_of(at);
                    let span = Span {
                        start: at,
                        end: at + 1,
                    };
                    let mut d = Diagnostic::new("E0005", "a `;`", span)
                        .with_fix("remove `;`", edits)
                        .locate(&module.path, &module.source);
                    if let Some(fix) = &mut d.fix {
                        fix.path = path.map(str::to_string);
                    }
                    d
                })
                .into_iter()
                .collect();
            CheckReport {
                diagnostics,
                ..CheckReport::default()
            }
        }
    }

    fn replace(at: usize, end: usize, text: &str) -> Edit {
        Edit {
            span: Span { start: at, end },
            replacement: text.to_string(),
        }
    }

    fn program(source: &str) -> Package {
        Package::single_file("app.osk", source)
    }

    #[test]
    fn fixes_apply_one_at_a_time_until_none_is_left() {
        let outcome = fix(
            program("a;b;c"),
            Options::default(),
            semicolons(|at| (None, vec![replace(at, at + 1, "")])),
        );
        assert_eq!(outcome.program.modules[0].source, "abc");
        assert_eq!(outcome.applied.len(), 2);
        assert_eq!(outcome.rounds, 2);
        assert_eq!((outcome.stop, outcome.refusal), (None, None));
    }

    #[test]
    fn a_fix_that_leaves_its_diagnostic_in_place_is_undone_and_stops_the_run() {
        // The edit replaces the `;` with itself.
        let outcome = fix(
            program("a;b"),
            Options::default(),
            semicolons(|at| (None, vec![replace(at, at + 1, ";")])),
        );
        assert!(matches!(outcome.stop, Some(Stop::Stuck(ref d)) if d.code == "E0005"));
        assert!(outcome.applied.is_empty());
        assert_eq!(outcome.rounds, 1);
        assert_eq!(outcome.program.modules[0].source, "a;b");
    }

    #[test]
    fn a_diagnostic_moved_by_its_own_fix_is_still_the_same_one() {
        // Inserting before the `;` only moves it: the diagnostic after the
        // edit is the same one, so the run stops.
        let outcome = fix(
            program("a;"),
            Options::default(),
            semicolons(|at| (None, vec![replace(at, at, "x")])),
        );
        assert!(matches!(outcome.stop, Some(Stop::Stuck(_))));
        assert_eq!(outcome.program.modules[0].source, "a;");
    }

    #[test]
    fn a_run_stops_at_the_cap_on_rounds() {
        // Each fix adds a `;` after the one it removes, so there is always
        // one more, somewhere new.
        let outcome = fix(
            program("a;"),
            Options::default(),
            semicolons(|at| (None, vec![replace(at, at + 1, "a;")])),
        );
        assert_eq!(outcome.stop, Some(Stop::Cap));
        assert_eq!(outcome.rounds, MAX_ROUNDS);
        assert_eq!(outcome.applied.len(), MAX_ROUNDS);
    }

    #[test]
    fn a_fix_of_a_file_outside_the_program_refuses_the_run() {
        let outcome = fix(
            program("a;b;"),
            Options::default(),
            semicolons(|at| (Some("../elsewhere.osk"), vec![replace(at, at + 1, "")])),
        );
        assert!(matches!(
            outcome.refusal,
            Some(Refusal::Outside { ref path, .. }) if path == "../elsewhere.osk"
        ));
        assert!(outcome.applied.is_empty());
        assert_eq!(outcome.program.modules[0].source, "a;b;");
    }

    #[test]
    fn a_fix_whose_edits_overlap_or_leave_the_file_refuses_the_run() {
        let outcome = fix(
            program("a;b;"),
            Options::default(),
            semicolons(|at| (None, vec![replace(at, at + 1, ""), replace(0, 2, "")])),
        );
        assert!(matches!(
            outcome.refusal,
            Some(Refusal::Edits {
                error: EditError::Overlap,
                ..
            })
        ));
        let outcome = fix(
            program("a;"),
            Options::default(),
            semicolons(|_| (None, vec![replace(1, 9, "")])),
        );
        assert!(matches!(
            outcome.refusal,
            Some(Refusal::Edits {
                error: EditError::OutOfRange,
                ..
            })
        ));
        assert_eq!(outcome.program.modules[0].source, "a;");
    }
}
