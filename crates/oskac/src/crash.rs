// SPDX-License-Identifier: Apache-2.0
//! What `oskac` does when it has a bug: a Rust panic inside `oskac` itself,
//! never a panic of a program it runs (`P0038` and the other `P00xx` stops
//! are reported by `oskac` and do not come here).
//!
//! The `dist` profile (the shipped `oskac`, see `Cargo.toml`) sets
//! `panic = "abort"`, so a panic does not unwind the stack and no `Drop`
//! code runs. This module keeps what that code did for the one temporary
//! file that a long-lived `oskac` process holds, the native executable that
//! `oskac run` writes and runs: it is listed here while it exists, and the
//! hook below removes it before it ends the process. The staged file of a
//! package write (`package::Staged`) is not listed: nothing between its
//! creation and its commit can panic (each step returns an `io::Result`),
//! and a signal that kills the process would skip `Drop` as well.
//! The hook runs on every panic, before an abort, and also in builds that
//! unwind, so the message and the exit code are the same in both.
//!
//! The exit code is 101, the one a Rust panic ends the process with when it
//! unwinds, so a script that reads it sees no change.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The exit code of a crash.
const EXIT_CODE: i32 = 101;

/// Where to report a crash.
const REPORT: &str = "https://github.com/aparsons/oska/issues";

/// Files and directories to remove if `oskac` crashes: each file is listed
/// before the directory that holds it.
static TEMPS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Remove `path` on a crash, until [`untrack`] is called for it.
pub fn track(path: &Path) {
    if let Ok(mut temps) = TEMPS.lock() {
        temps.push(path.to_path_buf());
    }
}

/// Stop tracking `path`: its owner removes it.
pub fn untrack(path: &Path) {
    if let Ok(mut temps) = TEMPS.lock() {
        temps.retain(|tracked| tracked != path);
    }
}

/// Remove every tracked file, then every tracked directory. A lock that
/// another thread holds is not waited for: a crash must end.
fn clean_up() {
    if let Ok(temps) = TEMPS.try_lock() {
        remove_all(&temps);
    }
}

/// Remove each of `paths` that is a file, then each that is a directory.
fn remove_all(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
    for path in paths.iter().rev() {
        let _ = std::fs::remove_dir(path);
    }
}

/// The text of a panic's message, if it has one.
fn message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "no message".to_string()
    }
}

/// Whether the caller asked for a backtrace: `RUST_BACKTRACE` is set to
/// anything but `0`. The default report stays one line.
fn wants_backtrace() -> bool {
    std::env::var_os("RUST_BACKTRACE").is_some_and(|value| value != "0")
}

/// Report a crash in one line and a request, remove the temporary files,
/// and end the process with 101. Call it first in `main`.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map_or(String::new(), |at| {
            format!(" at {}:{}", at.file(), at.line())
        });
        let _ = writeln!(
            std::io::stderr(),
            "oskac: internal error: {}{place}\nThis is a bug in oskac, not in your program. Please report it: {REPORT}",
            message(info.payload())
        );
        if wants_backtrace() {
            let _ = writeln!(
                std::io::stderr(),
                "stack backtrace:\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }
        clean_up();
        std::process::exit(EXIT_CODE);
    }));
}

#[cfg(test)]
mod tests {
    use super::{message, remove_all, track, untrack, TEMPS};
    use std::path::{Path, PathBuf};

    fn is_tracked(path: &Path) -> bool {
        TEMPS
            .lock()
            .map(|temps| temps.iter().any(|tracked| tracked == path))
            .unwrap_or(false)
    }

    /// A file is removed, and then the directory that held it.
    #[test]
    fn remove_all_removes_files_and_then_their_directories() {
        let dir = std::env::temp_dir().join(format!("oskac-crash-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("should make the directory");
        let file = dir.join("program");
        std::fs::write(&file, b"x").expect("should write the file");
        remove_all(&[file.clone(), dir.clone()]);
        assert!(!file.exists(), "the file is removed");
        assert!(!dir.exists(), "the directory is removed after its file");
    }

    /// A path is tracked until its owner says it has removed it.
    #[test]
    fn a_path_is_tracked_until_it_is_untracked() {
        let path = PathBuf::from("/oskac-crash-unit/never-made");
        track(&path);
        assert!(is_tracked(&path));
        untrack(&path);
        assert!(!is_tracked(&path));
    }

    /// A native executable is tracked while it exists, so a crash removes
    /// it, and untracked when it is dropped.
    #[test]
    fn a_native_executable_is_tracked_while_it_exists() {
        let exe = crate::native::Executable::create(b"not run").expect("should write it");
        let path = exe.path().to_path_buf();
        let dir = path.parent().expect("it has a directory").to_path_buf();
        assert!(is_tracked(&path) && is_tracked(&dir));
        drop(exe);
        assert!(!is_tracked(&path) && !is_tracked(&dir));
        assert!(!path.exists() && !dir.exists());
    }

    /// The message is the text of a `&str` or `String` payload.
    #[test]
    fn the_message_is_the_text_of_the_payload() {
        assert_eq!(message(&"static text"), "static text");
        assert_eq!(message(&String::from("owned text")), "owned text");
        assert_eq!(message(&42_u32), "no message");
    }
}
