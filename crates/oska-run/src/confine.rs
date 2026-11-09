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
//! A write may name a file that does not exist yet, which cannot be
//! canonicalized. Then the file's parent directory is canonicalized and
//! checked instead, the last component must be a plain name (not `..`),
//! and the joined path must not be a symlink: a dangling symlink would
//! otherwise send the write wherever it points.
//!
//! The check and the open are two steps, so a program racing another
//! process that swaps a directory for a symlink in between could escape.
//! The interpreter runs one program at a time and does not defend against
//! a concurrent attacker on the same filesystem.

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
    let joined = root.join(relative);
    match host.canonicalize(&joined) {
        Ok(real) => inside(root, real, path),
        Err(err) if intent == Intent::Read || err.kind() != io::ErrorKind::NotFound => Err(err),
        Err(_) => new_file(host, root, &joined, path),
    }
}

/// The real path of a file to create at `joined`, which does not resolve.
fn new_file(host: &mut dyn Host, root: &Path, joined: &Path, path: &str) -> io::Result<PathBuf> {
    let (Some(parent), Some(Component::Normal(name))) =
        (joined.parent(), joined.components().next_back())
    else {
        return Err(escapes(path));
    };
    if host.is_symlink(joined) {
        return Err(refused(&format!(
            "`{path}` is a symlink to nowhere; a write would follow it"
        )));
    }
    let parent = inside(root, host.canonicalize(parent)?, path)?;
    Ok(parent.join(name))
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
