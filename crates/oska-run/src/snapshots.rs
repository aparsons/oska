//! Snapshot assertions: `t.assert_snapshot(value)`.
//!
//! A test does not read or write a snapshot: it has only stand-in
//! capabilities. Each backend records what each snapshot assertion was
//! given, rendered as `dbg` renders it, with the assertion's span
//! ([`SnapshotRecord`], in [`crate::TestResult::snapshots`]). After the
//! tests have run, the test runner ([`settle`]) compares each with its
//! file, `snapshots/<test-file>/<test-name>.snap`, under the package's
//! root or the directory of a single file:
//!
//! - The same value passes.
//! - A missing or different snapshot fails the test with `P0014`, whose
//!   message shows the difference and names `oskac test --update`.
//! - With `--update`, a missing or different snapshot is written instead,
//!   and listed in the report ([`Written`]). Nothing is ever deleted.
//! - A test that panicked fails with its panic: its snapshots are neither
//!   compared nor written, since it did not run to its end.
//!
//! A test that runs more than one snapshot assertion numbers them in the
//! order they run, `<test-name>-1.snap` and so on ([`file_name`]): source
//! order for straight-line code, and the same on every run, since a test
//! runs on stand-ins. An assertion in a helper the test calls, or in a
//! loop, counts each time it runs.
//!
//! The runner follows no symlink beneath the root: a `snapshots/`
//! directory, a directory in it, or a snapshot file that is a symlink
//! fails the test instead, so no read or write leaves the root.

use std::io;
use std::path::Path;

use oska_syntax::{Diagnostic, Frame, Frames, SnapshotRecord, Span};

use crate::panics::codes;
use crate::testing::{TestReport, TestResult, TestStatus};

/// The intrinsic a snapshot assertion calls.
pub const ASSERT_SNAPSHOT: &str = "TestContext.assert_snapshot";

/// What `--update` names in each failure.
const UPDATE: &str = "oskac test --update";

/// The most lines on either side that [`diff`] lines up after trimming
/// what the two share at the start and the end; past it, it shows both
/// sides whole.
const DIFF_LINES: usize = 1000;

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

/// The names Windows keeps for devices, whatever their case and
/// extension: a file named `CON` or `nul.txt` is no file there.
const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// The file name of snapshot `number` of `count`, the snapshot assertions
/// the test named `test` ran, counted from 1: `<test-name>.snap` for the
/// only one, `<test-name>-<number>.snap` otherwise. Each character a file
/// name cannot hold on some platform (`/ \ : * ? " < > |` and control
/// characters) becomes `_`, and so does a `.` or space that starts or
/// ends the name, so a name such as `../x` stays in its directory. A
/// name Windows keeps for a device, such as `CON` or `com1.x`, gets a `_`
/// before it.
pub fn file_name(test: &str, number: usize, count: usize) -> String {
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
    let mut name: String = name.into_iter().collect();
    if is_device(&name) {
        name.insert(0, '_');
    }
    if count == 1 {
        format!("{name}.snap")
    } else {
        format!("{name}-{number}.snap")
    }
}

/// Whether Windows reads `name`, up to its first `.`, as a device:
/// `CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9` or `LPT1` to `LPT9`, in
/// any case.
fn is_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if DEVICES.contains(&stem.as_str()) {
        return true;
    }
    match stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
    {
        Some(digit) => digit.len() == 1 && matches!(digit.as_bytes()[0], b'1'..=b'9'),
        None => false,
    }
}

/// The difference between a `stored` snapshot (`None` when there is
/// none) and `value`, a line each: `- ` a stored line only, `+ ` a line
/// of the value only, two spaces one in both. The lines the two share at
/// the start and the end are trimmed first; when either side still has
/// more than [`DIFF_LINES`] lines, both are shown whole, the stored lines
/// then the value's, instead of lined up.
pub fn diff(stored: Option<&str>, value: &str) -> String {
    let old: Vec<&str> = stored.map_or_else(Vec::new, |s| s.split('\n').collect());
    let new: Vec<&str> = value.split('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_mid, new_mid) = (
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
    );
    let mut lines: Vec<String> = old[..prefix].iter().map(|l| format!("  {l}")).collect();
    if old_mid.len() > DIFF_LINES || new_mid.len() > DIFF_LINES {
        lines.extend(old_mid.iter().map(|l| format!("- {l}")));
        lines.extend(new_mid.iter().map(|l| format!("+ {l}")));
    } else {
        lines.extend(lined_up(old_mid, new_mid));
    }
    lines.extend(old[old.len() - suffix..].iter().map(|l| format!("  {l}")));
    lines.join("\n")
}

/// `old` and `new` lined up by their longest common subsequence.
fn lined_up(old: &[&str], new: &[&str]) -> Vec<String> {
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
    lines
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

/// Compare the snapshots each test of `report` that ran to its end
/// recorded with their files under `root`, the package's root or a
/// single file's directory, where `file_of` names the `<test-file>` of
/// the test at a span (`None` when it cannot, which fails the test): fail
/// each test with a missing or different snapshot, or with `update`,
/// write those instead and list them in the report. A test that
/// panicked keeps its panic as its failure.
pub fn settle(
    report: &mut TestReport,
    root: &Path,
    file_of: &dyn Fn(Span) -> Option<String>,
    update: bool,
) {
    let mut written = Vec::new();
    for test in &mut report.tests {
        if test.failure.is_some() || test.snapshots.is_empty() {
            continue;
        }
        if let Some(failure) = settle_test(test, root, file_of, update, &mut written) {
            test.status = TestStatus::Failed;
            test.failure = Some(failure);
        }
    }
    if update {
        report.snapshots = Some(written);
    }
}

/// [`settle`] for one `test`, which ran to its end: its first failure,
/// if any. What `--update` writes is added to `written`.
fn settle_test(
    test: &TestResult,
    root: &Path,
    file_of: &dyn Fn(Span) -> Option<String>,
    update: bool,
    written: &mut Vec<Written>,
) -> Option<Diagnostic> {
    let count = test.snapshots.len();
    let mut first: Option<Diagnostic> = None;
    let mut fail = |record: &SnapshotRecord, message: String| {
        first.get_or_insert_with(|| failure(test, record, message));
    };
    let Some(file) = file_of(test.span) else {
        let why = "cannot name the test's file, which its snapshots are under";
        return Some(failure(test, &test.snapshots[0], why.to_string()));
    };
    for (index, record) in test.snapshots.iter().enumerate() {
        let path = format!(
            "snapshots/{file}/{}",
            file_name(&test.name, index + 1, count)
        );
        let stored = match read(root, &path) {
            Ok(stored) => stored.map(|text| stored_value(&text)),
            Err(err) => {
                fail(record, format!("cannot read snapshot `{path}`: {err}"));
                continue;
            }
        };
        if stored.as_deref() == Some(record.value.as_str()) {
            continue;
        }
        if update {
            match write(root, &path, &record.value) {
                Ok(()) => written.push(Written {
                    path,
                    new: stored.is_none(),
                }),
                Err(err) => fail(record, format!("cannot write snapshot `{path}`: {err}")),
            }
            continue;
        }
        let why = match &stored {
            None => format!("no snapshot `{path}` yet; run `{UPDATE}` to write it"),
            Some(_) => {
                format!("the value differs from snapshot `{path}`; run `{UPDATE}` to accept it")
            }
        };
        let difference = diff(stored.as_deref(), &record.value);
        fail(record, format!("{why}\n{difference}"));
    }
    drop(fail);
    first
}

/// The `P0014` failure of `test` at `record`, with `message`.
fn failure(test: &TestResult, record: &SnapshotRecord, message: String) -> Diagnostic {
    let frames = Frames {
        frames: vec![Frame::new(Frame::test(&test.name), record.span)],
        omitted: 0,
    };
    Diagnostic::new(codes::SNAPSHOT_FAILED, message, record.span).with_frames(frames)
}

/// Whether `at`, beneath `root`, exists, refusing it when it is a
/// symlink.
fn exists_not_linked(root: &Path, at: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(at) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::other(format!(
            "`{}` is a symlink, which the test runner does not follow",
            shown(root, at)
        ))),
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// `at`, beneath `root`, as a `/`-separated path from it.
fn shown(root: &Path, at: &Path) -> String {
    let relative = at.strip_prefix(root).unwrap_or(at);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The text of the snapshot at the `/`-separated `path` beneath `root`,
/// or `None` when there is none. No component of it may be a symlink.
fn read(root: &Path, path: &str) -> io::Result<Option<String>> {
    let mut at = root.to_path_buf();
    for name in path.split('/') {
        at.push(name);
        if !exists_not_linked(root, &at)? {
            return Ok(None);
        }
    }
    std::fs::read_to_string(at).map(Some)
}

/// Write `value` and a line break to the snapshot at the `/`-separated
/// `path` beneath `root`, making its directories one at a time. No
/// component of it may be a symlink.
fn write(root: &Path, path: &str, value: &str) -> io::Result<()> {
    let names: Vec<&str> = path.split('/').collect();
    let (file, dirs) = names.split_last().expect("a path has a file name");
    let mut at = root.to_path_buf();
    for name in dirs {
        at.push(name);
        if !exists_not_linked(root, &at)? {
            std::fs::create_dir(&at)?;
        }
    }
    at.push(file);
    exists_not_linked(root, &at)?;
    std::fs::write(&at, format!("{value}\n"))
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
    fn a_long_diff_shows_both_sides_whole_between_what_they_share() {
        let old: Vec<String> = (0..=DIFF_LINES).map(|n| format!("o{n}")).collect();
        let new: Vec<String> = (0..=DIFF_LINES).map(|n| format!("n{n}")).collect();
        let stored = format!("same\n{}\nend", old.join("\n"));
        let value = format!("same\n{}\nend", new.join("\n"));
        let shown = diff(Some(&stored), &value);
        let lines: Vec<&str> = shown.lines().collect();
        assert_eq!(lines.len(), 2 * (DIFF_LINES + 1) + 2);
        assert_eq!(lines[0], "  same");
        assert_eq!(lines[1], "- o0");
        assert_eq!(lines[DIFF_LINES + 2], "+ n0");
        assert_eq!(lines[lines.len() - 1], "  end");
    }

    #[test]
    fn a_file_name_keeps_to_its_directory() {
        assert_eq!(file_name("adds", 1, 1), "adds.snap");
        assert_eq!(file_name("adds", 2, 3), "adds-2.snap");
        assert_eq!(file_name("a/b:c", 1, 1), "a_b_c.snap");
        assert_eq!(file_name("..", 1, 1), "__.snap");
        assert_eq!(file_name("", 1, 1), "_.snap");
    }

    #[test]
    fn a_name_windows_keeps_for_a_device_is_not_a_file_name() {
        for device in ["CON", "con", "Nul", "aux.txt", "COM1", "lpt9.x", "prn"] {
            assert_eq!(
                file_name(device, 1, 1),
                format!("_{device}.snap"),
                "{device}"
            );
        }
        for plain in ["COM0", "COM10", "console", "LPT", "nullish"] {
            assert_eq!(file_name(plain, 1, 1), format!("{plain}.snap"), "{plain}");
        }
    }
}
