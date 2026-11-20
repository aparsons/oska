//! Snapshot assertions: `t.assert_snapshot(value)`.
//!
//! A test does not read or write a snapshot: it has only stand-in
//! capabilities. Each backend records what each snapshot assertion was
//! given, rendered as `dbg` renders it, with the assertion's span
//! ([`Snapshot`], in [`crate::TestResult::snapshots`]). After the tests
//! have run, the test runner ([`settle`]) compares each with its file,
//! `snapshots/<test-file>/<test-name>.snap`, under the package's root or
//! the directory of a single file:
//!
//! - The same value passes.
//! - A missing or different snapshot fails the test with `P0014`, whose
//!   message shows the difference and names `oskac test --update`.
//! - With `--update`, a missing or different snapshot is written instead,
//!   and listed in the report ([`Written`]). Nothing is ever deleted.
//!
//! A test with more than one snapshot assertion numbers them in source
//! order, `<test-name>-1.snap` and so on ([`numbered`]). So that the
//! number of each is fixed by the source, each assertion must be written
//! directly in its test's body and run once; one in a function the test
//! calls, or one run more than once, fails the test with `P0014`.

use std::io;
use std::ops::ControlFlow;
use std::path::Path;

use oska_check::program::{walk, Callee, Test, Visit};
use oska_syntax::{Diagnostic, Frame, Frames, Span};

use crate::panics::codes;
use crate::testing::{TestReport, TestStatus};

/// The intrinsic a snapshot assertion calls.
pub const ASSERT_SNAPSHOT: &str = "TestContext.assert_snapshot";

/// What `--update` names in each failure.
const UPDATE: &str = "oskac test --update";

/// What one snapshot assertion of a test was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The assertion's call.
    pub span: Span,
    /// Its number among its test's snapshot assertions, from 1 in source
    /// order, or `None` when the test has only the one.
    pub number: Option<usize>,
    /// The value, as `dbg` renders it.
    pub value: String,
}

/// A snapshot `--update` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Its path from the root, `/`-separated:
    /// `snapshots/<test-file>/<test-name>.snap`.
    pub path: String,
    /// Whether there was none before; otherwise it changed.
    pub new: bool,
}

impl Written {
    /// `new` or `changed`, as the report says it.
    pub fn status(&self) -> &'static str {
        if self.new {
            "new"
        } else {
            "changed"
        }
    }
}

/// The snapshot assertions written in `test`'s body, in source order.
pub fn sites(test: &Test) -> Vec<Span> {
    let mut found = Vec::new();
    let _ = walk(&test.body, &mut |visit| {
        if let Visit::Callee(Callee::Intrinsic(name), span) = visit {
            if name == ASSERT_SNAPSHOT {
                found.push(span);
            }
        }
        ControlFlow::<()>::Continue(())
    });
    found.sort_by_key(|span| (span.start, span.end));
    found.dedup();
    found
}

/// The snapshots `test` recorded, each its call's span and its value in
/// the order they ran, numbered by [`sites`]: or the `P0014` failure of
/// one that is not written directly in the test's body, or that ran more
/// than once.
pub fn numbered(test: &Test, recorded: Vec<(Span, String)>) -> Result<Vec<Snapshot>, Diagnostic> {
    let sites = sites(test);
    let mut seen = vec![false; sites.len()];
    let mut snapshots = Vec::with_capacity(recorded.len());
    for (span, value) in recorded {
        let Some(index) = sites.iter().position(|site| *site == span) else {
            return Err(Diagnostic::new(
                codes::SNAPSHOT_FAILED,
                "a snapshot assertion must be written directly in the test's body, \
                 where its place numbers its snapshot; this one is in a function the test calls",
                span,
            ));
        };
        if std::mem::replace(&mut seen[index], true) {
            return Err(Diagnostic::new(
                codes::SNAPSHOT_FAILED,
                "a snapshot assertion must run once, where its place numbers its snapshot; \
                 this one ran more than once",
                span,
            ));
        }
        snapshots.push(Snapshot {
            span,
            number: (sites.len() > 1).then_some(index + 1),
            value,
        });
    }
    Ok(snapshots)
}

/// The file name of the snapshot of the test named `test` with `number`:
/// `<test-name>.snap`, or `<test-name>-<number>.snap`. Each character a
/// file name cannot hold on some platform (`/ \ : * ? " < > |` and
/// control characters) becomes `_`, and so does a `.` or space that
/// starts or ends the name, so a name such as `../x` stays in its
/// directory.
pub fn file_name(test: &str, number: Option<usize>) -> String {
    let mut name: Vec<char> = test
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if name.is_empty() {
        name.push('_');
    }
    for at in [0, name.len() - 1] {
        if matches!(name[at], '.' | ' ') {
            name[at] = '_';
        }
    }
    let name: String = name.into_iter().collect();
    match number {
        Some(number) => format!("{name}-{number}.snap"),
        None => format!("{name}.snap"),
    }
}

/// The difference between a `stored` snapshot (`None` when there is
/// none) and `value`, a line each: `- ` a stored line only, `+ ` a line
/// of the value only, two spaces one in both.
pub fn diff(stored: Option<&str>, value: &str) -> String {
    let old: Vec<&str> = stored.map_or_else(Vec::new, |s| s.split('\n').collect());
    let new: Vec<&str> = value.split('\n').collect();
    // The longest common subsequence, from the end.
    let mut common = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i][j] = if old[i] == new[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            lines.push(format!("  {}", old[i]));
            i += 1;
            j += 1;
        } else if i < old.len() && (j == new.len() || common[i + 1][j] >= common[i][j + 1]) {
            lines.push(format!("- {}", old[i]));
            i += 1;
        } else {
            lines.push(format!("+ {}", new[j]));
            j += 1;
        }
    }
    lines.join("\n")
}

/// The text of a stored snapshot file, as compared with a value: line
/// breaks as `\n`, without the one that ends the file.
fn stored_value(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    match text.strip_suffix('\n') {
        Some(value) => value.to_string(),
        None => text,
    }
}

/// Compare the snapshots each test of `report` recorded with their files
/// under `root`, the package's root or a single file's directory, where
/// `file_of` names the `<test-file>` of the test at a span: fail each test
/// with a missing or different snapshot, or with `update`, write those
/// instead and list them in the report. A snapshot failure is the test's
/// first, so it takes the place of a panic that came after it.
pub fn settle(
    report: &mut TestReport,
    root: &Path,
    file_of: &dyn Fn(Span) -> String,
    update: bool,
) {
    let mut written = Vec::new();
    for test in &mut report.tests {
        let mut failure = None;
        for snapshot in &test.snapshots {
            let path = format!(
                "snapshots/{}/{}",
                file_of(test.span),
                file_name(&test.name, snapshot.number)
            );
            let file = path.split('/').fold(root.to_path_buf(), |at, c| at.join(c));
            let stored = match std::fs::read_to_string(&file) {
                Ok(text) => Some(stored_value(&text)),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => {
                    failure.get_or_insert_with(|| {
                        fail(
                            test,
                            snapshot,
                            format!("cannot read snapshot `{path}`: {err}"),
                        )
                    });
                    continue;
                }
            };
            if stored.as_deref() == Some(snapshot.value.as_str()) {
                continue;
            }
            if update {
                match write(&file, &snapshot.value) {
                    Ok(()) => written.push(Written {
                        path,
                        new: stored.is_none(),
                    }),
                    Err(err) => {
                        failure.get_or_insert_with(|| {
                            fail(
                                test,
                                snapshot,
                                format!("cannot write snapshot `{path}`: {err}"),
                            )
                        });
                    }
                }
                continue;
            }
            let why = match &stored {
                None => format!("no snapshot `{path}` yet; run `{UPDATE}` to write it"),
                Some(_) => {
                    format!("the value differs from snapshot `{path}`; run `{UPDATE}` to accept it")
                }
            };
            let message = format!("{why}\n{}", diff(stored.as_deref(), &snapshot.value));
            failure.get_or_insert_with(|| fail(test, snapshot, message));
        }
        if let Some(failure) = failure {
            test.status = TestStatus::Failed;
            test.failure = Some(failure);
        }
    }
    if update {
        report.snapshots = Some(written);
    }
}

/// The `P0014` failure of `test` at `snapshot`, with `message`.
fn fail(test: &crate::TestResult, snapshot: &Snapshot, message: String) -> Diagnostic {
    let frames = Frames {
        frames: vec![Frame::new(Frame::test(&test.name), snapshot.span)],
        omitted: 0,
    };
    Diagnostic::new(codes::SNAPSHOT_FAILED, message, snapshot.span).with_frames(frames)
}

/// Write `value` to the snapshot `file`, with its directories.
fn write(file: &Path, value: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, format!("{value}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_marks_the_lines_only_one_side_has() {
        assert_eq!(diff(Some("a\nb\nc"), "a\nc\nd"), "  a\n- b\n  c\n+ d");
        assert_eq!(diff(None, "x"), "+ x");
    }

    #[test]
    fn a_file_name_keeps_to_its_directory() {
        assert_eq!(file_name("adds", None), "adds.snap");
        assert_eq!(file_name("adds", Some(2)), "adds-2.snap");
        assert_eq!(file_name("a/b:c", None), "a_b_c.snap");
        assert_eq!(file_name("..", None), "__.snap");
        assert_eq!(file_name("", None), "_.snap");
    }
}
