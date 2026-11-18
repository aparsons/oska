//! The wording of an `IoError` (ADR 0014): one Oska table keyed by the
//! kind of failure, with no operating-system error number.

use std::io::{self, ErrorKind};

use oska_sys::{io_error_message, wording, IO_KINDS, OTHER_WORDING};

/// Kinds in the table that no Linux error number maps to, so the test
/// below cannot reach them from a number, with why.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
#[test]
fn no_system_error_says_its_number_and_every_kind_is_reachable_or_documented() {
    let mut reached = Vec::new();
    for errno in 1..=133 {
        let error = io::Error::from_raw_os_error(errno);
        let text = io_error_message(&error);
        assert!(!text.contains("os error"), "errno {errno}: {text}");
        assert!(
            !text.chars().any(|c| c.is_ascii_digit()),
            "errno {errno}: {text}"
        );
        assert_eq!(text, wording(error.kind()), "errno {errno}");
        reached.push(format!("{:?}", error.kind()));
    }
    for (kind, _) in IO_KINDS {
        let documented = NOT_REACHED_BY_A_NUMBER.iter().any(|(k, _)| k == kind);
        assert!(
            reached.iter().any(|r| r == kind) || documented,
            "{kind:?} is neither reachable nor documented"
        );
    }
}

#[test]
fn the_wording_of_a_kind() {
    assert_eq!(wording(ErrorKind::NotFound), "not found");
    assert_eq!(wording(ErrorKind::DirectoryNotEmpty), "directory not empty");
    assert_eq!(wording(ErrorKind::Other), OTHER_WORDING);
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
