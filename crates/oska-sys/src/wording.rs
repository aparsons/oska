//! Oska's own wording for an `IoError`, the same on every platform and
//! both backends (ADR 0014).
//!
//! A failure the operating system reports is worded from the kind of
//! failure it is, by [`IO_KINDS`], never from the system's own text or
//! error number, which differ between Linux, macOS and Windows. The
//! native runtime's `std.rt` words the same kinds with the same text; a
//! test in `oskac` checks its table against this one for every number.

use std::io::{self, ErrorKind};

/// What an `IoError` says for a failure whose kind is not in [`IO_KINDS`].
pub const OTHER_WORDING: &str = "I/O error";

/// The kinds of failure the runtime words itself, with the wording of
/// each. No wording holds an operating-system error number.
pub const IO_KINDS: &[(ErrorKind, &str)] = &[
    (ErrorKind::NotFound, "not found"),
    (ErrorKind::PermissionDenied, "permission denied"),
    (ErrorKind::AlreadyExists, "already exists"),
    (ErrorKind::NotADirectory, "not a directory"),
    (ErrorKind::IsADirectory, "is a directory"),
    (ErrorKind::DirectoryNotEmpty, "directory not empty"),
    (ErrorKind::StorageFull, "no space left on device"),
    (ErrorKind::BrokenPipe, "broken pipe"),
    (ErrorKind::InvalidData, "invalid data"),
    (ErrorKind::InvalidInput, "invalid input"),
    (ErrorKind::InvalidFilename, "invalid file name"),
    (ErrorKind::ReadOnlyFilesystem, "read-only file system"),
    (ErrorKind::FileTooLarge, "file too large"),
    (ErrorKind::ResourceBusy, "resource busy"),
    (ErrorKind::QuotaExceeded, "quota exceeded"),
    (ErrorKind::CrossesDevices, "cannot move across devices"),
    (ErrorKind::TooManyLinks, "too many links"),
    (ErrorKind::NotSeekable, "not seekable"),
    (ErrorKind::OutOfMemory, "out of memory"),
    (ErrorKind::Unsupported, "not supported"),
    (ErrorKind::Interrupted, "interrupted"),
    (ErrorKind::WouldBlock, "would block"),
    (ErrorKind::TimedOut, "timed out"),
];

/// The wording for a failure of `kind`.
pub fn wording(kind: ErrorKind) -> &'static str {
    IO_KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or(OTHER_WORDING, |(_, text)| text)
}

/// The text of `error` for an `IoError`: Oska's wording for the kind of
/// an error the operating system reported, which carries its error
/// number, and the message of an error Oska made itself, which has none.
pub fn io_error_message(error: &io::Error) -> String {
    if error.raw_os_error().is_some() {
        wording(error.kind()).to_string()
    } else {
        error.to_string()
    }
}
