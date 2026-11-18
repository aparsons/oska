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

/// The kinds of failure the runtime words itself, by the name of `io::ErrorKind`'s variant (naming
/// it, not the variant, keeps the crate building on the oldest supported Rust), with the wording of
/// each. No wording holds an operating-system error number.
pub const IO_KINDS: &[(&str, &str)] = &[
    ("NotFound", "not found"),
    ("PermissionDenied", "permission denied"),
    ("AlreadyExists", "already exists"),
    ("NotADirectory", "not a directory"),
    ("IsADirectory", "is a directory"),
    ("DirectoryNotEmpty", "directory not empty"),
    ("StorageFull", "no space left on device"),
    ("BrokenPipe", "broken pipe"),
    ("InvalidData", "invalid data"),
    ("InvalidInput", "invalid input"),
    ("InvalidFilename", "invalid file name"),
    ("ReadOnlyFilesystem", "read-only file system"),
    ("FileTooLarge", "file too large"),
    ("ResourceBusy", "resource busy"),
    ("QuotaExceeded", "quota exceeded"),
    ("CrossesDevices", "cannot move across devices"),
    ("TooManyLinks", "too many links"),
    ("NotSeekable", "not seekable"),
    ("OutOfMemory", "out of memory"),
    ("Unsupported", "not supported"),
    ("Interrupted", "interrupted"),
    ("WouldBlock", "would block"),
    ("TimedOut", "timed out"),
];

/// The wording for a failure of `kind`.
pub fn wording(kind: ErrorKind) -> &'static str {
    let name = format!("{kind:?}");
    IO_KINDS
        .iter()
        .find(|(known, _)| *known == name)
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
