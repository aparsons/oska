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
use std::path::Path;
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

/// The files of the package `name`, as (path in the package, contents),
/// in the order they are written.
fn files(name: &str, options: Options) -> Vec<(&'static str, String)> {
    let mut files = vec![(
        "oska.toml",
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )];
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

/// `oskac new <name>`: write the package, or refuse with nothing written.
pub fn command(name: &str, options: Options, json: bool) -> ExitCode {
    if let Err(refusal) = refusal(name) {
        return fail(json, refusal.0, &refusal.1);
    }
    let files = files(name, options);
    let dir = Path::new(name);
    if let Err(err) = write(dir, &files) {
        return fail(json, "io", &format!("cannot write `{name}`: {err}"));
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
        outln!("created the package `{name}` in `{name}`:");
        for path in &written {
            outln!("  {path}");
        }
    }
    ExitCode::SUCCESS
}

/// Why `oskac new` must not write the package `name`, as the error code
/// `--format=json` gives and the message, or nothing. The name is checked
/// as the manifest's `name` is (snake_case), so it is also a plain
/// directory name, with no separator and no `..`.
fn refusal(name: &str) -> Result<(), (&'static str, String)> {
    let manifest = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    if oska_check::parse_manifest("oska.toml", &manifest).is_err() {
        return Err((
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
        Ok(meta) if meta.is_dir() => match std::fs::read_dir(name).map(|mut e| e.next()) {
            Ok(None) => Ok(()),
            Ok(Some(_)) => Err((
                "not_empty",
                format!(
                    "the directory `{name}` is not empty; `oskac new` writes only into a new \
                     or empty directory, so nothing was written"
                ),
            )),
            Err(err) => Err(("io", format!("cannot read the directory `{name}`: {err}"))),
        },
        Ok(_) => Err((
            "exists",
            format!(
                "`{name}` already exists and is not a directory; `oskac new` writes only into \
                 a new or empty directory, so nothing was written"
            ),
        )),
        Err(err) => Err(("io", format!("cannot look at `{name}`: {err}"))),
    }
}

/// Create `dir` and the `files` in it, each one new.
fn write(dir: &Path, files: &[(&str, String)]) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    for (path, contents) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(contents.as_bytes())?;
    }
    Ok(())
}

/// A refusal: the error code under `--format=json`, and why. Exit code 1.
fn fail(json: bool, code: &str, message: &str) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":{},\"message\":{}}}",
            quote(code),
            quote(message)
        );
    } else {
        eprintln!("oskac new: {message}");
    }
    ExitCode::FAILURE
}
