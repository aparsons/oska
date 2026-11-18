//! Oska's own wording for an `IoError`, the same on every platform and
//! both backends (ADR 0014).
//!
//! A failure the operating system reports is worded from its error code,
//! by the platform's table ([`LINUX_ERRORS`], [`MACOS_ERRORS`],
//! [`WINDOWS_ERRORS`]) and [`IO_KINDS`], never from the system's own text
//! or error number. The tables are keyed by the raw code, not by
//! `io::ErrorKind`, whose mapping from a code differs between Rust
//! versions (on 1.77 `ENOTDIR` is `Uncategorized`), so the wording does
//! not change with the compiler. The native runtime's `std.rt` words
//! Linux's numbers with the same text; a test in `oskac` checks its table
//! against [`LINUX_ERRORS`] for every number.

use std::io;

/// What an `IoError` says for a failure whose code is in no table.
pub const OTHER_WORDING: &str = "I/O error";

/// The kinds of failure the runtime words itself, by Oska's name for the
/// kind (the name of the `io::ErrorKind` variant it matches, though
/// nothing is looked up by that variant), with the wording of each. No
/// wording holds an operating-system error number.
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

/// Linux's error numbers (`asm-generic/errno-base.h` and `errno.h`, the
/// same on every Linux processor Oska targets) of the kinds in
/// [`IO_KINDS`], with each kind.
pub const LINUX_ERRORS: &[(i32, &str)] = &[
    (1, "PermissionDenied"),    // EPERM
    (2, "NotFound"),            // ENOENT
    (4, "Interrupted"),         // EINTR
    (11, "WouldBlock"),         // EAGAIN, EWOULDBLOCK
    (12, "OutOfMemory"),        // ENOMEM
    (13, "PermissionDenied"),   // EACCES
    (16, "ResourceBusy"),       // EBUSY
    (17, "AlreadyExists"),      // EEXIST
    (18, "CrossesDevices"),     // EXDEV
    (20, "NotADirectory"),      // ENOTDIR
    (21, "IsADirectory"),       // EISDIR
    (22, "InvalidInput"),       // EINVAL
    (27, "FileTooLarge"),       // EFBIG
    (28, "StorageFull"),        // ENOSPC
    (29, "NotSeekable"),        // ESPIPE
    (30, "ReadOnlyFilesystem"), // EROFS
    (31, "TooManyLinks"),       // EMLINK
    (32, "BrokenPipe"),         // EPIPE
    (36, "InvalidFilename"),    // ENAMETOOLONG
    (38, "Unsupported"),        // ENOSYS
    (39, "DirectoryNotEmpty"),  // ENOTEMPTY
    (95, "Unsupported"),        // EOPNOTSUPP, ENOTSUP
    (110, "TimedOut"),          // ETIMEDOUT
    (122, "QuotaExceeded"),     // EDQUOT
];

/// macOS's error numbers (`sys/errno.h`) of the same failures as
/// [`LINUX_ERRORS`], with each kind.
pub const MACOS_ERRORS: &[(i32, &str)] = &[
    (1, "PermissionDenied"),    // EPERM
    (2, "NotFound"),            // ENOENT
    (4, "Interrupted"),         // EINTR
    (12, "OutOfMemory"),        // ENOMEM
    (13, "PermissionDenied"),   // EACCES
    (16, "ResourceBusy"),       // EBUSY
    (17, "AlreadyExists"),      // EEXIST
    (18, "CrossesDevices"),     // EXDEV
    (20, "NotADirectory"),      // ENOTDIR
    (21, "IsADirectory"),       // EISDIR
    (22, "InvalidInput"),       // EINVAL
    (27, "FileTooLarge"),       // EFBIG
    (28, "StorageFull"),        // ENOSPC
    (29, "NotSeekable"),        // ESPIPE
    (30, "ReadOnlyFilesystem"), // EROFS
    (31, "TooManyLinks"),       // EMLINK
    (32, "BrokenPipe"),         // EPIPE
    (35, "WouldBlock"),         // EAGAIN, EWOULDBLOCK
    (45, "Unsupported"),        // ENOTSUP
    (60, "TimedOut"),           // ETIMEDOUT
    (63, "InvalidFilename"),    // ENAMETOOLONG
    (66, "DirectoryNotEmpty"),  // ENOTEMPTY
    (69, "QuotaExceeded"),      // EDQUOT
    (78, "Unsupported"),        // ENOSYS
    (102, "Unsupported"),       // EOPNOTSUPP
];

/// Windows's Win32 error codes (`winerror.h`) of the kinds in
/// [`IO_KINDS`], with each kind: the code `RtlNtStatusToDosError` or
/// `GetLastError` gives.
pub const WINDOWS_ERRORS: &[(i32, &str)] = &[
    (2, "NotFound"),            // ERROR_FILE_NOT_FOUND
    (3, "NotFound"),            // ERROR_PATH_NOT_FOUND
    (5, "PermissionDenied"),    // ERROR_ACCESS_DENIED
    (8, "OutOfMemory"),         // ERROR_NOT_ENOUGH_MEMORY
    (14, "OutOfMemory"),        // ERROR_OUTOFMEMORY
    (15, "NotFound"),           // ERROR_INVALID_DRIVE
    (17, "CrossesDevices"),     // ERROR_NOT_SAME_DEVICE
    (19, "ReadOnlyFilesystem"), // ERROR_WRITE_PROTECT
    (39, "StorageFull"),        // ERROR_HANDLE_DISK_FULL
    (53, "NotFound"),           // ERROR_BAD_NETPATH
    (67, "NotFound"),           // ERROR_BAD_NET_NAME
    (80, "AlreadyExists"),      // ERROR_FILE_EXISTS
    (87, "InvalidInput"),       // ERROR_INVALID_PARAMETER
    (109, "BrokenPipe"),        // ERROR_BROKEN_PIPE
    (112, "StorageFull"),       // ERROR_DISK_FULL
    (120, "Unsupported"),       // ERROR_CALL_NOT_IMPLEMENTED
    (121, "TimedOut"),          // ERROR_SEM_TIMEOUT
    (123, "InvalidFilename"),   // ERROR_INVALID_NAME
    (132, "NotSeekable"),       // ERROR_SEEK_ON_DEVICE
    (145, "DirectoryNotEmpty"), // ERROR_DIR_NOT_EMPTY
    (161, "InvalidFilename"),   // ERROR_BAD_PATHNAME
    (170, "ResourceBusy"),      // ERROR_BUSY
    (183, "AlreadyExists"),     // ERROR_ALREADY_EXISTS
    (206, "InvalidFilename"),   // ERROR_FILENAME_EXCED_RANGE
    (223, "FileTooLarge"),      // ERROR_FILE_TOO_LARGE
    (232, "BrokenPipe"),        // ERROR_NO_DATA
    (258, "TimedOut"),          // WAIT_TIMEOUT
    (267, "NotADirectory"),     // ERROR_DIRECTORY
    (336, "IsADirectory"),      // ERROR_DIRECTORY_NOT_SUPPORTED
    (1142, "TooManyLinks"),     // ERROR_TOO_MANY_LINKS
    (1295, "QuotaExceeded"),    // ERROR_DISK_QUOTA_EXCEEDED
    (1460, "TimedOut"),         // ERROR_TIMEOUT
];

/// The table of the operating system this is built for: empty on one
/// Oska does not target, where every system error says [`OTHER_WORDING`].
#[cfg(target_os = "linux")]
const SYSTEM_ERRORS: &[(i32, &str)] = LINUX_ERRORS;
#[cfg(target_os = "macos")]
const SYSTEM_ERRORS: &[(i32, &str)] = MACOS_ERRORS;
#[cfg(windows)]
const SYSTEM_ERRORS: &[(i32, &str)] = WINDOWS_ERRORS;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
const SYSTEM_ERRORS: &[(i32, &str)] = &[];

/// The wording for a failure of the kind named `kind` in [`IO_KINDS`].
pub fn wording(kind: &str) -> &'static str {
    IO_KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or(OTHER_WORDING, |(_, text)| text)
}

/// The wording for the system error `code`, by the table `errors`.
pub fn system_wording(errors: &[(i32, &str)], code: i32) -> &'static str {
    errors
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(OTHER_WORDING, |(_, kind)| wording(kind))
}

/// The text of `error` for an `IoError`: Oska's wording for the code of
/// an error the operating system reported, by this platform's table, and
/// the message of an error Oska made itself, which has none.
pub fn io_error_message(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => system_wording(SYSTEM_ERRORS, code).to_string(),
        None => error.to_string(),
    }
}
