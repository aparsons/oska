// SPDX-License-Identifier: Apache-2.0
//! The wording of an `IoError` is keyed by each operating system's raw
//! error code (the `errno` on Linux and macOS, the Win32 code on
//! Windows), not by `io::ErrorKind`, whose mapping from a code changes
//! between Rust versions (#363). So the same failure has the same wording
//! on every operating system, whatever Rust built the toolchain.

use std::io;

use oska_sys::{
    io_error_message, system_wording, IO_KINDS, LINUX_ERRORS, MACOS_ERRORS, OTHER_WORDING,
    WINDOWS_ERRORS,
};

/// Every operating system's table, by name.
const TABLES: [(&str, &[(i32, &str)]); 3] = [
    ("Linux", LINUX_ERRORS),
    ("macOS", MACOS_ERRORS),
    ("Windows", WINDOWS_ERRORS),
];

#[test]
fn every_system_code_is_listed_once_with_a_kind_that_has_a_wording() {
    for (os, table) in TABLES {
        for (index, (code, kind)) in table.iter().enumerate() {
            assert!(
                IO_KINDS.iter().any(|(known, _)| known == kind),
                "{os} {code}: {kind:?} has no wording"
            );
            assert!(
                table[..index].iter().all(|(other, _)| other != code),
                "{os} {code} is listed twice"
            );
        }
    }
}

#[test]
fn the_same_failure_has_the_same_wording_on_every_operating_system() {
    // (Linux's errno, macOS's errno, Windows's Win32 code) of one failure.
    let same = [
        (2, 2, 2, "not found"),                     // ENOENT, ERROR_FILE_NOT_FOUND
        (2, 2, 3, "not found"),                     // ERROR_PATH_NOT_FOUND
        (13, 13, 5, "permission denied"),           // EACCES, ERROR_ACCESS_DENIED
        (17, 17, 80, "already exists"),             // EEXIST, ERROR_FILE_EXISTS
        (17, 17, 183, "already exists"),            // ERROR_ALREADY_EXISTS
        (20, 20, 267, "not a directory"),           // ENOTDIR, ERROR_DIRECTORY
        (21, 21, 336, "is a directory"),            // EISDIR, ERROR_DIRECTORY_NOT_SUPPORTED
        (39, 66, 145, "directory not empty"),       // ENOTEMPTY, ERROR_DIR_NOT_EMPTY
        (28, 28, 112, "no space left on device"),   // ENOSPC, ERROR_DISK_FULL
        (32, 32, 109, "broken pipe"),               // EPIPE, ERROR_BROKEN_PIPE
        (22, 22, 87, "invalid input"),              // EINVAL, ERROR_INVALID_PARAMETER
        (36, 63, 206, "invalid file name"),         // ENAMETOOLONG, ERROR_FILENAME_EXCED_RANGE
        (30, 30, 19, "read-only file system"),      // EROFS, ERROR_WRITE_PROTECT
        (18, 18, 17, "cannot move across devices"), // EXDEV, ERROR_NOT_SAME_DEVICE
        (110, 60, 1460, "timed out"),               // ETIMEDOUT, ERROR_TIMEOUT
    ];
    for (linux, macos, windows, text) in same {
        assert_eq!(system_wording(LINUX_ERRORS, linux), text, "Linux {linux}");
        assert_eq!(system_wording(MACOS_ERRORS, macos), text, "macOS {macos}");
        assert_eq!(
            system_wording(WINDOWS_ERRORS, windows),
            text,
            "Windows {windows}"
        );
    }
}

#[test]
fn a_code_in_no_table_says_i_o_error() {
    for (os, table) in TABLES {
        // Codes no table lists: Linux's `ELOOP`, and one no system has.
        assert_eq!(system_wording(table, 40), OTHER_WORDING, "{os}");
        assert_eq!(system_wording(table, 99_999), OTHER_WORDING, "{os}");
    }
}

/// The system's own codes for the failures checked below, and their
/// wording.
#[cfg(target_os = "linux")]
const OWN: &[(i32, &str)] = &[
    (2, "not found"),
    (20, "not a directory"),
    (21, "is a directory"),
    (39, "directory not empty"),
    (28, "no space left on device"),
];
#[cfg(target_os = "macos")]
const OWN: &[(i32, &str)] = &[
    (2, "not found"),
    (20, "not a directory"),
    (21, "is a directory"),
    (66, "directory not empty"),
    (28, "no space left on device"),
];
#[cfg(windows)]
const OWN: &[(i32, &str)] = &[
    (2, "not found"),
    (267, "not a directory"),
    (336, "is a directory"),
    (145, "directory not empty"),
    (112, "no space left on device"),
];

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn an_error_from_the_system_is_worded_by_its_code_whatever_its_kind() {
    for (code, text) in OWN {
        let error = io::Error::from_raw_os_error(*code);
        let said = io_error_message(&error);
        assert_eq!(said, *text, "{code}, which Rust calls {:?}", error.kind());
        assert!(!said.contains("os error"), "{code}: {said}");
    }
}
