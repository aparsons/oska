// SPDX-License-Identifier: Apache-2.0
//! `DirWrite.with_lock`, the exclusive lock on a directory (ticket #846,
//! spec #842, ADR 0008): one writer holds a store at a time. The lock is
//! scoped, because Oska has no destructors: it is taken, a closure runs
//! with the same directory capability, and it is released when the
//! closure returns or panics. A second lock on the directory, from this
//! program or another process, fails at once with an `IoError` that
//! begins `L0001` and names the lock.
//!
//! The first half runs on [`oska_run::FakeHost`] through the differential
//! helpers, so every program also runs as a native executable and the
//! two must agree on output and on the filesystem afterwards (the lock
//! file included). The second half runs on [`oska_run::RealHost`] against
//! a real directory, on Unix and Windows, and holds the lock from a second
//! process.

mod common;

use common::{printed, printed_with, run_on};
use oska_run::FakeHost;

/// What a lock already held says, with the root's path left out.
const HELD: &str = "REFUSED: L0001: the directory lock `.oska-lock` is already held";

#[test]
fn a_lock_runs_the_closure_with_the_directory_and_gives_its_result() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let locked = store.with_lock(|dir: DirWrite| dir.write(Path.new("a.txt"), "inside"))
    match locked {
        Ok(inner) => io.println(done(inner))?
        Err(e) => io.println(refused(e))?
    }
    io.println(shown(read.restrict_to("store").read_to_string(Path.new("a.txt"))))?"#,
    );
    assert_eq!(out, "done\ninside\n");
}

#[test]
fn a_second_lock_on_the_same_directory_fails_and_names_the_lock() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        match dir.with_lock(|again: DirWrite| 1) {
            Ok(_) => "second lock taken"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n"));
}

#[test]
fn a_second_capability_for_the_same_directory_conflicts_too() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let first = write.restrict_to("store")
    let second = write.restrict_to("store/../store")
    let outer = first.with_lock(|dir: DirWrite| {
        match second.with_lock(|again: DirWrite| 1) {
            Ok(_) => "second lock taken"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n"));
}

#[test]
fn the_lock_is_released_when_the_closure_returns() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    for round in 0..3 {
        match store.with_lock(|dir: DirWrite| round) {
            Ok(n) => io.println("round {n}")?
            Err(e) => io.println(refused(e))?
        }
    }"#,
    );
    assert_eq!(out, "round 0\nround 1\nround 2\n");
}

#[test]
fn the_failed_attempt_does_not_release_the_holders_lock() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        let first = match dir.with_lock(|a: DirWrite| 1) {
            Ok(_) => "taken"
            Err(e) => refused(e)
        }
        let second = match dir.with_lock(|b: DirWrite| 2) {
            Ok(_) => "taken"
            Err(e) => refused(e)
        }
        [first, second]
    })
    match outer {
        Ok(lines) => {
            for line in lines {
                io.println(line)?
            }
        }
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n{HELD}\n"));
}

#[test]
fn locks_on_different_directories_do_not_conflict() {
    let host = FakeHost::new().with_dir("one").with_dir("two");
    let out = printed(
        host,
        r#"    let one = write.restrict_to("one")
    let two = write.restrict_to("two")
    let outer = one.with_lock(|dir: DirWrite| {
        match two.with_lock(|other: DirWrite| 7) {
            Ok(n) => "both {n}"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, "both 7\n");
}

#[test]
fn a_subdirectory_has_its_own_lock() {
    let host = FakeHost::new().with_dir("store/sub");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        match dir.restrict_to(Path.new("sub")).with_lock(|inner: DirWrite| 3) {
            Ok(n) => "inner {n}"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, "inner 3\n");
}

#[test]
fn the_lock_file_is_made_once_and_left_in_place() {
    let mut host = FakeHost::new().with_dir("store");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| 0)
    let _ = store.with_lock(|dir: DirWrite| 0)
    io.println(listed(read.restrict_to("store").list_dir(Path.new("."))))?
    io.println(shown(read.restrict_to("store").read_to_string(Path.new(".oska-lock"))))?"#,
    );
    assert_eq!(out, ".oska-lock\n\n");
    assert_eq!(host.file("store/.oska-lock"), Some(""));
}

#[test]
fn a_lock_on_a_missing_directory_is_an_error_and_runs_nothing() {
    let host = FakeHost::new();
    let out = printed(
        host,
        r#"    let store = write.restrict_to("absent")
    match store.with_lock(|dir: DirWrite| 1) {
        Ok(_) => io.println("ran")?
        Err(e) => io.println("refused")?
    }"#,
    );
    assert_eq!(out, "refused\n");
}

#[test]
fn a_lock_needs_no_more_authority_than_the_directory_capability() {
    // A closure may capture other capabilities, as a `cap fn`.
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| io.println("holding"))
    io.println("released")?"#,
    );
    assert_eq!(out, "holding\nreleased\n");
}

#[test]
fn a_panic_in_the_closure_ends_the_program_with_its_panic_code() {
    let host = FakeHost::new().with_dir("store");
    let src = common::fs_program(
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| [1, 2].at(5))"#,
    );
    let outcome = run_on(host, &src, &[]);
    assert_eq!(outcome.panic.map(|p| p.code), Some("P0002"), "{src}");
    assert_eq!(outcome.exit_code, 101);
}
