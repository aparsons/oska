// SPDX-License-Identifier: Apache-2.0
//! Helpers that make a test behave the same on Linux, macOS and Windows:
//! a scratch directory, JSON escapes for paths, a home environment, and
//! paths that keep `..` literal. Any test that builds a path, writes a path
//! into JSON, or sets a home should use these (see `CODING_STANDARDS.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch directory, canonical and empty, removed on drop. The name is
/// made unique per process; give each test its own `name`.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(prefix: &str, name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("{prefix}-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.canonicalize().expect("canonical scratch dir"))
    }
}

impl Scratch {
    /// The directory.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `text` as a JSON string literal, quotes included. Backslashes (Windows
/// paths) and quotes are escaped.
pub fn json_str(text: &str) -> String {
    format!("\"{}\"", json_escape(text))
}

/// `text` escaped for the inside of a JSON string literal.
pub fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `path` as a JSON string literal.
pub fn json_path(path: &Path) -> String {
    json_str(&path.to_string_lossy())
}

/// `base` followed by `parts`, joined with the platform separator as plain
/// text, so a `..` part stays in the path. `Path::join("..")` on a `\\?\`
/// path on Windows takes the step instead, leaving no `..` to test.
pub fn literal_path(base: &Path, parts: &[&str]) -> PathBuf {
    let mut text = base.to_string_lossy().into_owned();
    for part in parts {
        text.push_str(std::path::MAIN_SEPARATOR_STR);
        text.push_str(part);
    }
    PathBuf::from(text)
}

/// Make `home` the user's home for `command`: `HOME` and `USERPROFILE` (the
/// variable Windows reads) and `XDG_CONFIG_HOME` (`<home>/.config`).
pub fn set_home<'a>(command: &'a mut Command, home: &Path) -> &'a mut Command {
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
}

/// A path as `oskac` prints it: without the `\\?\` prefix of a Windows
/// canonical path.
pub fn shown(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// Run `command`, trying again while the kernel says the executable is
/// busy (`ETXTBSY`, 26): another test's `fork` can hold a fresh copy's
/// write handle open for an instant after the copy is made.
pub fn output_of(command: &mut Command) -> std::process::Output {
    for _ in 0..50 {
        match command.output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result.expect("the command should run"),
        }
    }
    command.output().expect("the command should run")
}

/// [`output_of`], with `input` written to the command's standard input,
/// which is then closed. A command that exits before it reads all of
/// `input` is not an error.
pub fn output_with_input(command: &mut Command, input: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for _ in 0..50 {
        match command.spawn() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => {
                let mut child = result.expect("the command should start");
                let mut stdin = child.stdin.take().expect("a stdin pipe");
                let _ = stdin.write_all(input);
                drop(stdin);
                return child.wait_with_output().expect("the command should finish");
            }
        }
    }
    panic!("the executable stayed busy");
}
