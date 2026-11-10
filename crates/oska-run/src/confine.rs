//! Filesystem confinement: how a restricted `Fs` keeps every path inside
//! its root.
//!
//! A restricted filesystem holds its root as a canonical path: absolute,
//! with every symlink followed. [`resolve`] turns a program's path into
//! the real path to open, or refuses it:
//!
//! 1. An empty path is refused, and so is an absolute one, even if it
//!    names a file inside the root: paths are relative to the root.
//! 2. The path is joined to the root and canonicalized by the host, which
//!    resolves `..` and follows every symlink, in every component.
//! 3. The result must be the root or lie under it, compared component by
//!    component, or the path is refused.
//!
//! A write names its file by a canonical parent directory and a plain
//! last component, and never follows a symlink in that component:
//!
//! 1. The path as written must end in a plain name: not in a separator,
//!    `.`, or `..`. `Path::components` drops a trailing `/` and `.`, so
//!    `dang/` would otherwise name `dang`, and a dangling symlink there
//!    would send the write wherever it points.
//! 2. The parent directory is canonicalized and must lie under the root.
//! 3. The exact path that will be opened, the canonical parent joined
//!    with the name, must not be a symlink, dangling or not.
//!
//! The host then opens the final component without following a symlink
//! (`O_NOFOLLOW` where the platform has it) and checks again, on the
//! opened file, that it still lies under the root (`host.rs`), before it
//! reads or writes anything.
//!
//! # What remains
//!
//! The check and the open are still separate steps on paths. Opening the
//! last component without following a symlink, and re-checking the opened
//! file, narrow the race but leave one window: another process that swaps
//! an *intermediate* directory for a symlink at the right moment can still
//! direct the open outside the root, though the re-check then refuses to
//! use what it opened (a write may already have created an empty file
//! there). Closing it needs the open to resolve the whole path beneath a
//! directory handle (Linux `openat2` with `RESOLVE_BENEATH`), which `std`
//! does not offer. The interpreter does not defend against a concurrent
//! attacker on the same filesystem (`docs/runtime.md`).

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::host::Host;

/// What a resolved path will be used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Reading an existing file, or restricting to an existing directory.
    Read,
    /// Writing a file, which may not exist yet.
    Write,
}

/// The real path `path` names under `root`, a canonical directory, or an
/// error when it would escape `root`.
pub fn resolve(
    host: &mut dyn Host,
    root: &Path,
    path: &str,
    intent: Intent,
) -> io::Result<PathBuf> {
    if path.is_empty() {
        return Err(refused("an empty path names no file"));
    }
    let relative = Path::new(path);
    if relative.has_root() || relative.is_absolute() {
        return Err(refused(&format!(
            "`{path}` is absolute; a restricted filesystem takes paths relative to its root"
        )));
    }
    match intent {
        // The root itself, which may be a file: `file/.` would not
        // canonicalize, so the root is taken as it is, already canonical.
        Intent::Read if relative.components().all(|c| c == Component::CurDir) => {
            Ok(root.to_path_buf())
        }
        Intent::Read => inside(root, host.canonicalize(&root.join(relative))?, path),
        Intent::Write => file_to_write(host, root, path),
    }
}

/// The path of the file a write to `path` opens: the canonical parent
/// directory, under `root`, joined with the plain name `path` ends in.
fn file_to_write(host: &mut dyn Host, root: &Path, path: &str) -> io::Result<PathBuf> {
    let name = last_name(path).ok_or_else(|| {
        refused(&format!(
            "`{path}` does not end in a file name; a write needs one"
        ))
    })?;
    let joined = root.join(path);
    let parent = joined.parent().ok_or_else(|| escapes(path))?;
    let parent = inside(root, host.canonicalize(parent)?, path)?;
    let target = parent.join(name);
    if host.is_symlink(&target) {
        return Err(refused(&format!(
            "`{path}` is a symlink; a write never follows one"
        )));
    }
    Ok(target)
}

/// The plain file name `path` ends in, as written: `None` when it ends in
/// a separator, `.`, or `..`, which name a directory, not a file.
fn last_name(path: &str) -> Option<&str> {
    if path.ends_with(std::path::is_separator) {
        return None;
    }
    let name = path.rsplit(std::path::is_separator).next()?;
    match Path::new(name).components().next() {
        Some(Component::Normal(_)) => Some(name),
        _ => None,
    }
}

/// `real` if it is `root` or lies under it.
fn inside(root: &Path, real: PathBuf, path: &str) -> io::Result<PathBuf> {
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(escapes(path))
    }
}

fn escapes(path: &str) -> io::Error {
    refused(&format!("`{path}` leads outside the restricted directory"))
}

fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.to_string())
}
