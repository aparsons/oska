//! Helpers that make a test behave the same on Linux, macOS and Windows:
//! a scratch directory, JSON escapes for paths, a home environment, and
//! paths that keep `..` literal. Any test that builds a path, writes a path
//! into JSON, or sets a home should use these (see `CODING_STANDARDS.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The platform's path separator as text.
pub const SEP: char = std::path::MAIN_SEPARATOR;

/// A scratch directory, canonical and empty, removed on drop. The name is
/// made unique per process; give each test its own `name`.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(prefix: &str, name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("{prefix}-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.canonicalize().expect("canonical scratch dir"))
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
        text.push(SEP);
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
