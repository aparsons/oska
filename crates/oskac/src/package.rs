//! Packages on disk: what a path given to `oskac check`, `run` or `test`
//! names, and the discovery of a package's modules.
//!
//! A package is a directory with an `oska.toml` and a `src/` directory.
//! Every `.osk` file under `src/` is a module whose path is the file's
//! path under `src/` with `/` written as `.` and no `.osk` extension.
//! Only regular files and directories count: a symlink under `src/` is
//! refused, and nothing is followed (`symlink_metadata`, never
//! `metadata`), so a package's source is what is in its tree (ADR 0008).
//! The rules about names and `main` are the checker's
//! (`oska_check::check_package`); finding a symlink needs the file
//! system, so it is found here.

use std::path::Path;

use oska_check::{ModuleSource, Package, MANIFEST_PATH};
use oska_syntax::Diagnostic;

/// What a path argument names.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A source file, checked alone as before.
    File(String),
    /// A package directory: the argument, or `.` when there is none.
    Dir(String),
}

/// What `arg` names: nothing means the current directory, a directory is
/// a package, and anything else is a file (which may not exist: the
/// command reports that as it always did).
pub fn target(arg: Option<&str>) -> Target {
    match arg {
        None => Target::Dir(".".to_string()),
        Some(path) if Path::new(path).is_dir() => Target::Dir(path.to_string()),
        Some(path) => Target::File(path.to_string()),
    }
}

/// A directory read as a package.
pub enum Loaded {
    /// A package, with the symlinks found under `src/`, refused.
    Package {
        package: Package,
        refused: Vec<Diagnostic>,
    },
    /// Not a package: the refusal says why, with a hint.
    NotAPackage(Diagnostic),
    /// A file could not be read.
    Unreadable(String),
}

/// Read the package in `dir`: its manifest's text and every module.
pub fn load(dir: &str) -> Loaded {
    let root = Path::new(dir);
    let manifest = match std::fs::read_to_string(root.join(MANIFEST_PATH)) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Loaded::NotAPackage(oska_check::not_a_package(
                dir,
                &format!("it has no `{MANIFEST_PATH}`"),
            ));
        }
        Err(err) => {
            return Loaded::Unreadable(format!("cannot read {dir}/{MANIFEST_PATH}: {err}"));
        }
    };
    let src = root.join("src");
    let mut refused = Vec::new();
    let mut modules = Vec::new();
    match std::fs::symlink_metadata(&src) {
        Err(_) => {
            return Loaded::NotAPackage(oska_check::not_a_package(
                dir,
                "it has no `src/` directory",
            ));
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            refused.push(oska_check::symlink_refused("src"));
        }
        Ok(meta) if !meta.is_dir() => {
            return Loaded::NotAPackage(oska_check::not_a_package(dir, "`src` is not a directory"));
        }
        Ok(_) => {
            if let Err(message) = walk(&src, "src", &mut modules, &mut refused) {
                return Loaded::Unreadable(message);
            }
        }
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    Loaded::Package {
        package: Package {
            manifest: Some(manifest),
            modules,
        },
        refused,
    }
}

/// Collect the modules under `dir`, whose package-relative path is
/// `relative`, in name order, and refuse every symlink.
fn walk(
    dir: &Path,
    relative: &str,
    modules: &mut Vec<ModuleSource>,
    refused: &mut Vec<Diagnostic>,
) -> Result<(), String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let name = name.to_string_lossy();
        let child = format!("{relative}/{name}");
        let kind = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?
            .file_type();
        if kind.is_symlink() {
            refused.push(oska_check::symlink_refused(&child));
        } else if kind.is_dir() {
            walk(&path, &child, modules, refused)?;
        } else if kind.is_file() {
            if let Some(stem) = child.strip_suffix(".osk") {
                let source = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let module = stem.trim_start_matches("src/").replace('/', ".");
                modules.push(ModuleSource {
                    module,
                    path: child,
                    source,
                });
            }
        }
    }
    Ok(())
}

/// The package a source file belongs to: the nearest directory above it
/// that has an `oska.toml` and holds the file under `src/`. It is as
/// `file` is written, so `oskac check <dir>` works from where the file
/// path does.
pub fn package_containing(file: &str) -> Option<String> {
    let file = Path::new(file);
    file.ancestors().skip(1).find_map(|ancestor| {
        let manifest = ancestor.join(MANIFEST_PATH);
        let is_manifest = std::fs::symlink_metadata(manifest).is_ok_and(|m| m.is_file());
        (is_manifest && file.strip_prefix(ancestor.join("src")).is_ok()).then(|| {
            if ancestor.as_os_str().is_empty() {
                ".".to_string()
            } else {
                ancestor.to_string_lossy().into_owned()
            }
        })
    })
}
