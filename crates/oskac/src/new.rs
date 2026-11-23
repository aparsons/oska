// SPDX-License-Identifier: Apache-2.0
//! `oskac new <name>`: lay out a package (spec #367, Q9), so that no agent
//! writes `oska.toml` and `src/` from memory. It writes the directory
//! `<name>` with `oska.toml`, `src/main.osk` (or `src/lib.osk` for a
//! library) and the agent-setup `AGENTS.md` and `CLAUDE.md`, which are
//! built into `oskac` from `site/agent-setup/`, so it works offline and
//! writes the published templates byte for byte.
//!
//! Everything is decided before anything is written: a name that is not a
//! valid package name, and a path that is a file or a directory that is
//! not empty, are refused with nothing written. Each file is created new,
//! never opened over one that is there. The package passes `oskac check
//! --release` and `oskac test` as written: its `main` uses the one
//! capability it takes, so it has no unused authority.

use crate::json::quote;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The agent-setup templates, as the site publishes them.
const AGENTS_TEMPLATE: &str = include_str!("../../../site/agent-setup/AGENTS.template.md");
const CLAUDE_TEMPLATE: &str = include_str!("../../../site/agent-setup/CLAUDE.template.md");

/// The entry module of a new program.
const MAIN_SOURCE: &str = "\
// The entry module. `main` receives only the capabilities it declares,
// so this program can write to standard output and nothing else.

fn greeting(name: String) -> String {
    \"Hello, {name}!\"
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(greeting(\"world\"))
}

test \"greets by name\" {
    t.assert_eq(greeting(\"Ada\"), \"Hello, Ada!\")
}

test \"main prints the greeting\" {
    let _ = main(t.stdout)
    t.assert_eq(t.printed().len(), 1)
}
";

/// The root module of a new library. A library has no `main`.
const LIB_SOURCE: &str = "\
// The root module of the library. An item marked `pub` is part of its
// API; a library has no `main`, so it is checked and tested, not run.

pub fn greeting(name: String) -> String {
    \"Hello, {name}!\"
}

test \"greets by name\" {
    t.assert_eq(greeting(\"Ada\"), \"Hello, Ada!\")
}
";

/// What `oskac new` writes: whether the package is a library, and whether
/// the agent files are left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// `--lib`: write `src/lib.osk` and no `main`.
    pub lib: bool,
    /// `--no-agent-files`: write neither `AGENTS.md` nor `CLAUDE.md`.
    pub no_agent_files: bool,
}

/// The manifest of the package `name`.
fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// The files of the package `name`, as (path in the package, contents),
/// in the order they are written.
fn files(name: &str, options: Options) -> Vec<(&'static str, String)> {
    let mut files = vec![("oska.toml", manifest(name))];
    if options.lib {
        files.push(("src/lib.osk", LIB_SOURCE.to_string()));
    } else {
        files.push(("src/main.osk", MAIN_SOURCE.to_string()));
    }
    if !options.no_agent_files {
        files.push(("AGENTS.md", AGENTS_TEMPLATE.to_string()));
        files.push(("CLAUDE.md", CLAUDE_TEMPLATE.to_string()));
    }
    files
}

/// Why `oskac new` writes nothing: the error code `--format=json` gives,
/// and the message.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    code: &'static str,
    message: String,
}

impl Refusal {
    fn new(code: &'static str, message: String) -> Refusal {
        Refusal { code, message }
    }

    /// The directory `name` is there and holds something.
    fn not_empty(name: &str) -> Refusal {
        Refusal::new(
            "not_empty",
            format!(
                "the directory `{name}` is not empty; `oskac new` writes only into a new or \
                 empty directory, so nothing was written"
            ),
        )
    }
}

/// `oskac new <name>`: write the package, or refuse with nothing written.
pub fn command(name: &str, options: Options, json: bool) -> ExitCode {
    if let Err(refusal) = check(name) {
        return fail(json, &refusal);
    }
    let files = files(name, options);
    if let Err(refusal) = write(name, &files) {
        return fail(json, &refusal);
    }
    let written: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
    if json {
        let list: Vec<String> = written.iter().map(|p| quote(p)).collect();
        outln!(
            "{{\"ok\":true,\"directory\":{},\"files\":[{}]}}",
            quote(name),
            list.join(",")
        );
    } else {
        outln!("created the package `{name}`, in the directory `{name}`:");
        for path in &written {
            outln!("  {path}");
        }
    }
    ExitCode::SUCCESS
}

/// Why `oskac new` must not write the package `name`, if it must not. The
/// name is checked as the manifest's `name` is (snake_case), so it is also
/// a plain directory name, with no separator and no `..`.
fn check(name: &str) -> Result<(), Refusal> {
    if oska_check::parse_manifest("oska.toml", &manifest(name)).is_err() {
        return Err(Refusal::new(
            "invalid_name",
            format!(
                "`{name}` is not a valid package name: a name is snake_case, lowercase \
                 letters and digits in words joined by single underscores, starting with a \
                 letter, as in `my_app`"
            ),
        ));
    }
    match std::fs::symlink_metadata(name) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(meta) if meta.is_dir() => {
            if is_empty(Path::new(name)).map_err(|err| io(name, &err))? {
                Ok(())
            } else {
                Err(Refusal::not_empty(name))
            }
        }
        Ok(_) => Err(Refusal::new(
            "exists",
            format!(
                "`{name}` already exists and is not a directory; `oskac new` writes only into \
                 a new or empty directory, so nothing was written"
            ),
        )),
        Err(err) => Err(io(name, &err)),
    }
}

fn is_empty(dir: &Path) -> std::io::Result<bool> {
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

/// A refusal by the file system, about `name`.
fn io(name: &str, err: &std::io::Error) -> Refusal {
    Refusal::new("io", format!("cannot write `{name}`: {err}"))
}

/// Write the `files` into the directory `name`: made here when it is not
/// there, else looked at again, just before the first file, to be still an
/// empty directory and not a symlink. On any failure, what this run made is
/// removed again, and nothing that was there before is touched.
fn write(name: &str, files: &[(&str, String)]) -> Result<(), Refusal> {
    let dir = Path::new(name);
    let mut created = Vec::new();
    match std::fs::create_dir(dir) {
        Ok(()) => created.push(dir.to_path_buf()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(dir).map_err(|err| io(name, &err))?;
            if !meta.is_dir() || !is_empty(dir).map_err(|err| io(name, &err))? {
                return Err(Refusal::not_empty(name));
            }
        }
        Err(err) => return Err(io(name, &err)),
    }
    let filled = fill(dir, files, &mut created);
    if let Err(err) = filled {
        undo(&created);
        return Err(io(name, &err));
    }
    Ok(())
}

/// Write each of `files` new into `dir`, making the directories they need,
/// and add each path it makes to `created`, in the order made.
fn fill(dir: &Path, files: &[(&str, String)], created: &mut Vec<PathBuf>) -> std::io::Result<()> {
    use std::io::Write;
    for (path, contents) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent().filter(|p| !p.exists()) {
            std::fs::create_dir(parent)?;
            created.push(parent.to_path_buf());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        created.push(path);
        file.write_all(contents.as_bytes())?;
    }
    Ok(())
}

/// Remove the paths in `created`, the newest first: files, and directories
/// only when empty, so nothing another process put there is lost.
fn undo(created: &[PathBuf]) {
    for path in created.iter().rev() {
        if path.is_dir() {
            let _ = std::fs::remove_dir(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A refusal: printed, as JSON under `--format=json`. Exit code 1.
fn fail(json: bool, refusal: &Refusal) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":{},\"message\":{}}}",
            quote(refusal.code),
            quote(&refusal.message)
        );
    } else {
        eprintln!("oskac new: {}", refusal.message);
    }
    ExitCode::FAILURE
}

/// `oskac new` with no name, or more than one, or a flag it does not take:
/// a usage error, exit code 2, as JSON under `--format=json`.
pub fn usage(json: bool) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":\"usage\",\"message\":{}}}",
            quote("give one package name: `oskac new <name> [--lib] [--no-agent-files] [--format=json]`")
        );
    } else {
        eprint!("{}", crate::USAGE);
    }
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::{fill, undo};
    use std::path::PathBuf;

    /// A failure partway through leaves only what was there before: the
    /// file in the way of the third file stops the run, and undoing it
    /// removes the files and the directory it made, and keeps that file.
    #[test]
    fn a_failure_partway_removes_what_the_run_made_and_keeps_what_was_there() {
        let dir = std::env::temp_dir().join(format!("oskac-new-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).expect("mkdir");
        std::fs::write(dir.join("AGENTS.md"), "mine\n").expect("write");
        let files = [
            ("oska.toml", "manifest\n".to_string()),
            ("src/main.osk", "main\n".to_string()),
            ("AGENTS.md", "template\n".to_string()),
            ("CLAUDE.md", "template\n".to_string()),
        ];
        let mut created: Vec<PathBuf> = Vec::new();
        assert!(fill(&dir, &files, &mut created).is_err());
        assert_eq!(
            created,
            [
                dir.join("oska.toml"),
                dir.join("src"),
                dir.join("src/main.osk")
            ]
        );
        undo(&created);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .expect("read")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["AGENTS.md"]);
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).expect("read"),
            "mine\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
