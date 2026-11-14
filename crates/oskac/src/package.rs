//! Packages on disk: what a path given to `oskac check`, `run` or `test`
//! names, the discovery of a package's modules, and the reading and
//! replacing of its files that `oskac fix` does.
//!
//! A package is a directory with an `oska.toml` and a `src/` directory.
//! Every `.osk` file under `src/` is a module whose path is the file's
//! path under `src/` with `/` written as `.` and no `.osk` extension.
//! Only regular files and directories count: a symlinked `oska.toml`, and
//! a symlink under `src/`, are refused, and nothing is followed. Entries
//! are told apart with `symlink_metadata`, never `metadata`, and on Unix
//! every file is then opened beneath the package directory one component
//! at a time with `O_NOFOLLOW` (`oska_sys::Dir`), so a symlink swapped in
//! after the check makes the open fail instead of reading through it: a
//! package's source is what is in its tree (ADR 0008). The rules about
//! names and `main` are the checker's (`oska_check::check_package`);
//! finding a symlink needs the file system, so it is found here.

use std::path::{Component, Path, PathBuf};

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
    /// Not a package that can be checked: the directory has no manifest
    /// or no modules, or its manifest is a symlink or not UTF-8. The
    /// diagnostics say which.
    Refused(Vec<Diagnostic>),
    /// A file could not be read.
    Unreadable(String),
}

/// Read the package in `dir`: its manifest's text and every module.
/// `command` names the command in a hint.
pub fn load(dir: &str, command: &str) -> Loaded {
    let root = Path::new(dir);
    let files = match Files::open(root) {
        Ok(files) => files,
        Err(err) => return Loaded::Unreadable(format!("cannot read {dir}: {err}")),
    };
    match std::fs::symlink_metadata(root.join(MANIFEST_PATH)) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let above = package_above(dir);
            return Loaded::Refused(vec![oska_check::not_a_package(
                dir,
                &format!("it has no `{MANIFEST_PATH}`"),
                above.as_deref().map(|above| (command, above)),
            )]);
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            return Loaded::Refused(vec![oska_check::symlink_refused(MANIFEST_PATH)]);
        }
        _ => {}
    }
    let manifest = match files.read(&[MANIFEST_PATH]) {
        Ok(bytes) => match oska_check::decode_manifest(MANIFEST_PATH, &bytes) {
            Ok(text) => text,
            Err(refusal) => return Loaded::Refused(vec![refusal]),
        },
        Err(err) => {
            return Loaded::Unreadable(format!("cannot read {dir}/{MANIFEST_PATH}: {err}"));
        }
    };
    let src = root.join("src");
    let mut refused = Vec::new();
    let mut modules = Vec::new();
    match std::fs::symlink_metadata(&src) {
        Err(_) => {
            return Loaded::Refused(vec![oska_check::not_a_package(
                dir,
                "it has no `src/` directory",
                None,
            )]);
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            refused.push(oska_check::symlink_refused("src"));
        }
        Ok(meta) if !meta.is_dir() => {
            return Loaded::Refused(vec![oska_check::not_a_package(
                dir,
                "`src` is not a directory",
                None,
            )]);
        }
        Ok(_) => {
            let mut walk = Walk {
                files: &files,
                modules: &mut modules,
                refused: &mut refused,
            };
            if let Err(message) = walk.dir(&src, &["src".to_string()]) {
                return Loaded::Unreadable(message);
            }
        }
    }
    if modules.is_empty() && refused.is_empty() {
        return Loaded::Refused(vec![oska_check::not_a_package(
            dir,
            "its `src/` holds no `.osk` file, so it has no modules",
            None,
        )]);
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

/// The modules found so far, and the symlinks refused.
struct Walk<'w> {
    files: &'w Files,
    modules: &'w mut Vec<ModuleSource>,
    refused: &'w mut Vec<Diagnostic>,
}

impl Walk<'_> {
    /// Collect the modules under `dir`, whose package-relative path is
    /// the names `relative`, in name order, and refuse every symlink.
    fn dir(&mut self, dir: &Path, relative: &[String]) -> Result<(), String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<Result<_, _>>()
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        names.sort();
        for name in names {
            let path = dir.join(&name);
            let mut names = relative.to_vec();
            names.push(name.to_string_lossy().into_owned());
            let child = names.join("/");
            let kind = std::fs::symlink_metadata(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?
                .file_type();
            if kind.is_symlink() {
                self.refused.push(oska_check::symlink_refused(&child));
            } else if kind.is_dir() {
                self.dir(&path, &names)?;
            } else if kind.is_file() {
                let Some(stem) = child.strip_suffix(".osk") else {
                    continue;
                };
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let source = self
                    .files
                    .read(&names)
                    .and_then(|bytes| {
                        String::from_utf8(bytes)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    })
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                // `src/src/util.osk` is the module `src.util`: only the
                // first `src/` is the source directory.
                let module = stem.strip_prefix("src/").unwrap_or(stem).replace('/', ".");
                self.modules.push(ModuleSource {
                    module,
                    path: child,
                    source,
                });
            }
        }
        Ok(())
    }
}

/// The files of a package, or the directory of a single file, read and
/// replaced without following a symlink. On Unix, beneath a handle for
/// the directory, with `O_NOFOLLOW` at every component (`oska_sys::Dir`).
/// Elsewhere `oska-sys` offers nothing, and a `symlink_metadata` check
/// before each read and write is all there is.
pub struct Files {
    #[cfg(unix)]
    root: oska_sys::Dir,
    #[cfg(not(unix))]
    root: PathBuf,
}

impl Files {
    /// The directory at `root`, a path the user chose: its own
    /// components are followed, as any path given on the command line is.
    pub fn open(root: &Path) -> std::io::Result<Files> {
        #[cfg(unix)]
        let root = oska_sys::Dir::open(root)?;
        #[cfg(not(unix))]
        let root = root.to_path_buf();
        Ok(Files { root })
    }

    /// The bytes of the regular file reached by `names` beneath the
    /// directory, none of them a symlink.
    pub fn read(&self, names: &[&str]) -> std::io::Result<Vec<u8>> {
        #[cfg(unix)]
        {
            use std::io::Read;
            let mut bytes = Vec::new();
            self.root.open_file(names)?.read_to_end(&mut bytes)?;
            Ok(bytes)
        }
        #[cfg(not(unix))]
        {
            let path = self.path(names);
            refuse_symlink(&path)?;
            std::fs::read(path)
        }
    }

    /// Get ready to replace the regular file reached by `names`, none of
    /// them a symlink, with `bytes`: write them to a new file beside it,
    /// with its permissions. [`Staged::commit`] then renames the new file
    /// over the old one, in one step; dropping it uncommitted removes the
    /// new file.
    pub fn stage(&self, names: &[&str], bytes: &[u8]) -> std::io::Result<Staged> {
        use std::io::Write;
        let Some((last, parents)) = names.split_last() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no file name to write",
            ));
        };
        // Not `.osk`, so a new file left behind by a crash is no module.
        let temp = format!(".{last}.oskac-fix");
        #[cfg(unix)]
        let (dir, mut file, permissions) = {
            let dir = self.root.open_dir(parents)?;
            let permissions = dir.open_file(&[last])?.metadata()?.permissions();
            let file = dir.create_file(&[&temp])?;
            (dir, file, permissions)
        };
        #[cfg(not(unix))]
        let (dir, mut file, permissions) = {
            let dir = self.path(parents);
            let path = dir.join(last);
            refuse_symlink(&path)?;
            refuse_symlink(&dir.join(&temp))?;
            let permissions = std::fs::metadata(&path)?.permissions();
            let file = std::fs::File::create(dir.join(&temp))?;
            (dir, file, permissions)
        };
        let staged = Staged {
            dir,
            temp,
            name: (*last).to_string(),
            committed: false,
        };
        file.write_all(bytes)?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        Ok(staged)
    }

    /// The path `names` reach beneath the directory.
    #[cfg(not(unix))]
    fn path(&self, names: &[&str]) -> PathBuf {
        names.iter().fold(self.root.clone(), |p, n| p.join(n))
    }
}

/// An error when `path` is a symlink, which is never read or written
/// through.
#[cfg(not(unix))]
fn refuse_symlink(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("`{}` is a symlink, which is never followed", path.display()),
        )),
        _ => Ok(()),
    }
}

/// A file's new bytes, written beside it, waiting to replace it.
pub struct Staged {
    #[cfg(unix)]
    dir: oska_sys::Dir,
    #[cfg(not(unix))]
    dir: PathBuf,
    /// The new file's name, in `dir`.
    temp: String,
    /// The name of the file it replaces, in `dir`.
    name: String,
    committed: bool,
}

impl Staged {
    /// Replace the file with the new one, in one step.
    pub fn commit(mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        self.dir.rename(self.temp.as_ref(), self.name.as_ref())?;
        #[cfg(not(unix))]
        std::fs::rename(self.dir.join(&self.temp), self.dir.join(&self.name))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            #[cfg(unix)]
            let _ = self.dir.remove_file(self.temp.as_ref());
            #[cfg(not(unix))]
            let _ = std::fs::remove_file(self.dir.join(&self.temp));
        }
    }
}

/// The nearest directory above `dir` with an `oska.toml`, written from
/// `dir` as `..`, `../..` and so on: the package `dir` is inside, when it
/// is in one.
fn package_above(dir: &str) -> Option<String> {
    let absolute = std::fs::canonicalize(dir).ok()?;
    let up = absolute
        .ancestors()
        .skip(1)
        .position(|ancestor| ancestor.join(MANIFEST_PATH).is_file())?
        + 1;
    let mut above = PathBuf::from(dir);
    if above == Path::new(".") {
        above = PathBuf::new();
    }
    for _ in 0..up {
        above.push("..");
    }
    Some(above.to_string_lossy().into_owned())
}

/// `path`, package-relative, as a path from the current directory when
/// the package is `dir`, as given on the command line: `forge/src/main.osk`
/// for `src/main.osk` of `forge`. The current directory itself adds
/// nothing.
pub fn from_here(dir: &str, path: &str) -> String {
    let dir = Path::new(dir);
    if dir.components().all(|c| c == Component::CurDir) {
        return path.to_string();
    }
    // `/` on every platform: `path` already uses `/`, and joining with
    // the platform separator would print `forge\src/main.osk` on Windows.
    let dir = dir.to_string_lossy();
    format!("{}/{path}", dir.trim_end_matches(['/', '\\']))
}

/// The package a path belongs to: the nearest directory above it that
/// has a regular-file `oska.toml` (never a symlink) and holds the path
/// under `src/`, or whose `oska.toml` is the path itself. It is as `path`
/// is written, so `oskac check <dir>` works from where the path does.
///
/// This is the one rule for finding the package of a file: the hint after
/// `oskac check <file>`, the check-on-edit hook, and any later tool that
/// follows an edit use it.
pub fn package_containing(path: &str) -> Option<String> {
    let path = Path::new(path);
    path.ancestors().skip(1).find_map(|ancestor| {
        let manifest = ancestor.join(MANIFEST_PATH);
        let is_manifest = std::fs::symlink_metadata(&manifest).is_ok_and(|m| m.is_file());
        let belongs = path == manifest || path.strip_prefix(ancestor.join("src")).is_ok();
        (is_manifest && belongs).then(|| {
            if ancestor.as_os_str().is_empty() {
                ".".to_string()
            } else {
                ancestor.to_string_lossy().into_owned()
            }
        })
    })
}

/// What a tool that follows an edit of `path` checks: its package, found
/// as [`package_containing`] finds it, or the file alone.
pub fn discover(path: &str) -> Target {
    match package_containing(path) {
        Some(dir) => Target::Dir(dir),
        None => Target::File(path.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::from_here;

    #[test]
    fn a_path_from_here_joins_with_a_forward_slash_on_every_platform() {
        assert_eq!(from_here("forge", "src/main.osk"), "forge/src/main.osk");
        assert_eq!(from_here("forge/", "src/main.osk"), "forge/src/main.osk");
        assert_eq!(from_here("a/b", "src/x.osk"), "a/b/src/x.osk");
        assert_eq!(from_here(".", "src/main.osk"), "src/main.osk");
    }
}
