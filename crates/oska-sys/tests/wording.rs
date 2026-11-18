//! The wording of an `IoError` (ADR 0014): one Oska table keyed by the
//! kind of failure, with no operating-system error number.

use std::io::{self, ErrorKind};

use oska_sys::{io_error_message, wording, IO_KINDS, LINUX_ERRORS, OTHER_WORDING};

/// Kinds in the table that no Linux error number maps to, with why.
const NOT_REACHED_BY_A_NUMBER: &[(&str, &str)] = &[(
    "InvalidData",
    "Oska raises it itself, for bytes that are not text",
)];

#[test]
fn no_wording_holds_an_error_number() {
    for (kind, text) in IO_KINDS.iter().chain([("Other", OTHER_WORDING)].iter()) {
        assert!(!text.is_empty(), "{kind:?}");
        assert!(!text.contains("os error"), "{kind:?}: {text}");
        assert!(
            !text.chars().any(|c| c.is_ascii_digit()),
            "{kind:?}: {text}"
        );
    }
}

#[test]
fn every_kind_has_one_wording() {
    for (index, (kind, _)) in IO_KINDS.iter().enumerate() {
        assert!(
            IO_KINDS[..index].iter().all(|(other, _)| other != kind),
            "{kind:?} is listed twice"
        );
    }
}

#[test]
fn no_system_error_says_its_number() {
    for code in 1..=16_000 {
        let text = io_error_message(&io::Error::from_raw_os_error(code));
        assert!(!text.contains("os error"), "code {code}: {text}");
        assert!(
            !text.chars().any(|c| c.is_ascii_digit()),
            "code {code}: {text}"
        );
    }
}

#[test]
fn every_kind_is_reached_by_a_linux_number_or_documented() {
    for (kind, _) in IO_KINDS {
        let reached = LINUX_ERRORS.iter().any(|(_, k)| k == kind);
        let documented = NOT_REACHED_BY_A_NUMBER.iter().any(|(k, _)| k == kind);
        assert!(
            reached || documented,
            "{kind:?} is neither reachable nor documented"
        );
    }
}

#[test]
fn the_wording_of_a_kind() {
    assert_eq!(wording("NotFound"), "not found");
    assert_eq!(wording("DirectoryNotEmpty"), "directory not empty");
    assert_eq!(wording("Other"), OTHER_WORDING);
}

#[cfg(target_os = "linux")]
#[test]
fn an_error_from_the_system_is_worded_by_kind() {
    let said = |errno| io_error_message(&io::Error::from_raw_os_error(errno));
    assert_eq!(said(2), "not found");
    assert_eq!(said(39), "directory not empty");
    assert_eq!(said(28), "no space left on device");
}

#[test]
fn an_error_oska_made_keeps_its_message() {
    let own = io::Error::new(ErrorKind::PermissionDenied, "`x` is a symlink");
    assert_eq!(io_error_message(&own), "`x` is a symlink");
}
