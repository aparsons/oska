// SPDX-License-Identifier: Apache-2.0
//! The words [`crate::Dir`] refuses with, shared by Unix and Windows so
//! that a refusal is byte-identical on every platform (#175). On Windows
//! a reparse point is refused in the words Unix uses for a symlink.
//!
//! Each function builds the error for one refusal. The platform modules
//! decide when a refusal applies; only the words live here.
//!
//! An error that arises from one platform's own system calls keeps its
//! words in that platform's module, as there are no words on the other
//! to match: on Unix, a failed `rename` (only Unix has
//! [`crate::Dir`]'s rename) and a path holding a NUL byte, which a C
//! string cannot carry; on Windows, a name too long for `NtCreateFile`'s
//! 16-bit length and a directory listing not laid out as documented. A
//! refusal both platforms can meet belongs here. A NUL byte in a name is
//! one exception still worded per platform: Windows refuses it as not a
//! single name, before any system call.

use std::ffi::OsStr;
use std::io;

/// `names` joined with `/`, for messages.
pub(crate) fn joined<N: AsRef<OsStr>>(names: &[N]) -> String {
    names
        .iter()
        .map(|name| name.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `error`, from an operation on `path`, with the path named and the
/// OS error in Oska's own words ([`crate::io_error_message`]).
pub(crate) fn at(path: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("`{path}`: {}", crate::io_error_message(&error)),
    )
}

/// The error for a symlink at `path`, or on Windows any reparse point.
pub(crate) fn symlink(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("`{path}` is a symlink, and confined access never follows one"),
    )
}

/// The error for something at `path`, where a walk needs a directory,
/// that is a symlink or not a directory.
pub(crate) fn symlink_or_not_a_directory(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{path}` is a symlink or not a directory, and confined access never follows a symlink"
        ),
    )
}

/// The error for the file `names` reach having more than one hard link.
pub(crate) fn hard_linked<N: AsRef<OsStr>>(names: &[N]) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{}` has more than one hard link, and confined access never opens one",
            joined(names)
        ),
    )
}

/// The error for the file `names` reach not being a regular file.
pub(crate) fn not_a_file<N: AsRef<OsStr>>(names: &[N]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is not a regular file", joined(names)),
    )
}

/// The error for `name` not being one plain component.
pub(crate) fn not_a_single_name(name: &OsStr) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is not a single file name", name.to_string_lossy()),
    )
}

/// The error for a file operation given no name.
pub(crate) fn no_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no file name to open")
}

/// The error for [`crate::Dir::open`] given an empty path.
pub(crate) fn empty_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "an empty path names no directory",
    )
}

/// The error for [`crate::Dir::lock`] when another holder has the lock:
/// the code `L0001` and the lock's name, in the words the native runtime
/// uses (`std.rt`'s `lock_held`).
pub(crate) fn lock_held() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "{}: the directory lock `{}` is already held",
            crate::LOCK_CODE,
            crate::LOCK_NAME
        ),
    )
}
