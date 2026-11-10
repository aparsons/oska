//! Filesystem confinement: how a directory or file capability keeps every path inside
//! its root.
//!
//! # A walk beneath the root's handle
//!
//! A directory capability holds its root as a [`RootHandle`](crate::host::RootHandle) the host
//! opened. [`names`] turns a program's path into the plain names to walk
//! beneath it, on the path as written, without touching the filesystem:
//!
//! 1. An empty path is refused, and so is an absolute one, even if it
//!    names a file inside the root: paths are relative to the root.
//! 2. `.` is dropped, and `..` drops the name before it. A `..` with no
//!    name before it would climb above the root, and is refused.
//!
//! A write and `create_dir` take [`names_to_create`] instead, which also
//! refuses a path that does not end in a plain name.
//!
//! The host then walks the names one at a time beneath the root's handle
//! to read, write, create a directory, list one or narrow to one
//! (`Host::read_file_beneath`, `Host::write_file_beneath`,
//! `Host::create_dir_beneath`, `Host::list_dir_beneath`,
//! `Host::open_beneath`). On Unix each name is opened relative to the one
//! before, and none is followed if it is a symlink, the last included
//! (`oska-sys`): a write opens or creates its file beneath its parent's
//! handle, `create_dir` makes its directory there, and `list_dir` lists
//! through the directory's own handle. So there is no check for another
//! process to race: a symlink at any component, planted before the walk
//! or swapped in during it, makes the walk fail. Every symlink is
//! refused, dangling or not, wherever it points.
//!
//! # What remains on Windows
//!
//! Windows has no handle-relative open in `std`, so its host checks each
//! component on the path not to be a symlink, opens the path, and checks
//! again that what it opened lies under the root. That narrows the race
//! but does not close it: another process that swaps an intermediate
//! directory for a symlink at the right moment can still direct the open
//! outside the root, though the re-check then refuses to use what it
//! opened (a write may already have created an empty file there)
//! (`docs/runtime.md`).

use std::io;
use std::path::{Component, Path};

/// The plain names `path` walks beneath a root, with `.` and `..`
/// resolved as written, or why it is refused: it is empty, absolute, or
/// climbs above the root. No names means the root itself.
pub fn names(path: &str) -> io::Result<Vec<String>> {
    if path.is_empty() {
        return Err(refused("an empty path names no file"));
    }
    let mut names: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(refused(&format!(
                    "`{path}` is absolute; a directory capability takes paths relative to its root"
                )))
            }
            Component::CurDir => {}
            Component::ParentDir => {
                names.pop().ok_or_else(|| escapes(path))?;
            }
            Component::Normal(name) => names.push(name.to_string_lossy().into_owned()),
        }
    }
    Ok(names)
}

/// What a filesystem operation does: what its capability's access must
/// allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Reading a file, or listing a directory.
    Read,
    /// Writing a file, or creating a directory.
    Write,
}

/// The plain names a write or `create_dir` of `path` walks beneath a
/// root, as [`names`] gives them, when `path` as written ends in a plain
/// name: not in a separator, `.`, or `..`. `Path::components` drops a
/// trailing `/` and `.`, so `dang/` would otherwise name `dang`, and
/// `sub/.` would name `sub`: a write or a new directory needs a name of
/// its own.
pub fn names_to_create(path: &str) -> io::Result<Vec<String>> {
    let names = names(path)?;
    if ends_in_a_name(path) {
        Ok(names)
    } else {
        Err(refused(&format!(
            "`{path}` does not end in a file name; a write, a new directory or a file needs one"
        )))
    }
}

/// Whether `path` as written ends in a plain name, not in a separator,
/// `.`, or `..`.
fn ends_in_a_name(path: &str) -> bool {
    if path.ends_with(std::path::is_separator) {
        return false;
    }
    let name = path.rsplit(std::path::is_separator).next().unwrap_or("");
    matches!(
        Path::new(name).components().next(),
        Some(Component::Normal(_))
    )
}

fn escapes(path: &str) -> io::Error {
    refused(&format!("`{path}` leads outside the restricted directory"))
}

fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.to_string())
}
