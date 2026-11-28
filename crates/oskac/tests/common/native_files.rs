// SPDX-License-Identifier: Apache-2.0
//! Helpers shared by the native file-capability suites (`native_files.rs`
//! and `native_files_swaps.rs`), included by each with `#[path]` and not
//! declared in `mod.rs`: they need the suite's own `differential` module.

#![allow(dead_code)]

use crate::differential::{assert_same, Case, Outcome};
use std::path::Path;

/// The helpers every program has: what an operation gave, as one line.
pub const SHOWN: &str = r#"
pub fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

pub fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

pub fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

pub fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// A program whose `main` has `io`, `fs`, `read` (`fs.read_only()`),
/// `write` (`fs.write_only()`) and `args`, and runs `body`.
pub fn program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, fs: Fs, args: Args) -> Result<(), IoError> {{\n\
         \x20   let read = fs.read_only()\n\
         \x20   let write = fs.write_only()\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n{SHOWN}"
    )
}

/// The standard tree with `body` as the program.
pub fn case(name: &str, body: &str) -> Case {
    tree(Case::file(&format!("files_{name}"), &program(body)))
}

/// `case` with the standard tree's files.
pub fn tree(case: Case) -> Case {
    case.with_file("root/inside.txt", "inside text")
        .with_file("root/sub/nested.txt", "nested text")
        .with_file("outside/secret.txt", "outside text")
}

/// Run `case` on both backends, require the same outcome and a clean
/// exit, and return what it printed.
pub fn printed(case: &Case) -> String {
    let outcome: Outcome = assert_same(case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stderr, "", "{outcome:#?}");
    outcome.stdout
}

/// A symlink at `dir/at` to `target`, which is relative to the link's
/// directory or absolute. On Windows it is a directory symlink when
/// `target` is a directory, and its `/`s are backslashes.
pub fn link(dir: &Path, at: &str, target: impl AsRef<Path>) {
    let target = target.as_ref();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, dir.join(at));
    #[cfg(windows)]
    let made = {
        let at = dir.join(at);
        let target = std::path::PathBuf::from(target.to_string_lossy().replace('/', "\\"));
        let resolved = at.parent().expect("a parent").join(&target);
        match resolved.is_dir() {
            true => std::os::windows::fs::symlink_dir(&target, &at),
            false => std::os::windows::fs::symlink_file(&target, &at),
        }
    };
    made.expect("create a symlink");
}

/// `path`, a root's trusted path shown in a message, as this platform
/// shows it: with `\\` for `/` on Windows.
pub fn native(path: &str) -> String {
    match cfg!(windows) {
        true => path.replace('/', "\\"),
        false => path.to_string(),
    }
}

/// `expected` as this platform words it. Linux and macOS cannot tell a
/// symlink met where a directory is needed from anything else that is
/// not a directory; Windows can, and says it is a symlink.
pub fn worded(expected: &str) -> String {
    match cfg!(windows) {
        true => expected.replace(
            "is a symlink or not a directory, and confined access never follows a symlink",
            "is a symlink, and confined access never follows one",
        ),
        false => expected.to_string(),
    }
}

/// Every line of `seen` is refused, saying `why`, and there are `lines`.
#[track_caller]
pub fn assert_all_refused(seen: &str, lines: usize, why: &str) {
    assert_eq!(seen.lines().count(), lines, "{seen}");
    for line in seen.lines() {
        assert!(
            line.starts_with("REFUSED: ") && line.contains(why),
            "should be refused for `{why}`: {seen}"
        );
    }
}
