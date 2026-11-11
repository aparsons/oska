//! The one list of runtime tests that are not differential (spec #125):
//! the tests whose program the native backend does not run, each with
//! the reason. Every helper in this module compares the interpreter with
//! a native executable on aarch64 Linux (`native.rs`), except in the
//! tests named here.
//!
//! A test belongs here only when what it checks cannot happen on a real
//! OS (the fake host's clock, generator or wording), or, marked `gap:`,
//! while a known native parity gap is being closed. A test is named by
//! its file's crate and its path, as `cargo test` lists it:
//! `core::shadowing_in_a_block`; a program of the corpus that
//! `oskac`'s `native_corpus.rs` runs, by `native_corpus::` and its label
//! there, such as `native_corpus::guide-capabilities-0`. Tests on both
//! sides keep the list free of names that no longer exist.

/// The fake host's clock stands at the Unix epoch; a real one does not.
const FAKE_CLOCK: &str = "the fake host's clock stands at the Unix epoch";

/// The fake host's generator has a fixed seed; a real one is seeded by
/// the OS.
const FAKE_SEED: &str = "the fake host's generator has a fixed seed";

/// The fake host words its filesystem errors itself (`no such file or
/// directory: /x`); a real OS words them as `oska-sys` and the native
/// runtime do (`` `x`: No such file or directory (os error 2) ``).
const FAKE_WORDING: &str = "the fake host words filesystem errors its own way";

/// A program that prints the real clock prints a different time on
/// every run, whichever backend runs it.
const REAL_CLOCK: &str = "it prints the real clock, which differs from run to run";

/// Each test that is not differential, with why.
pub const SKIPS: &[(&str, &str)] = &[
    (
        "capabilities::the_fake_clock_stands_at_the_unix_epoch",
        FAKE_CLOCK,
    ),
    (
        "capabilities::random_numbers_vary_but_repeat_for_the_same_seed",
        FAKE_SEED,
    ),
    (
        "directory_capabilities::a_missing_root_fails_every_operation_naming_the_root",
        FAKE_WORDING,
    ),
    (
        "directory_capabilities::a_missing_subdirectory_fails_every_operation_naming_it",
        FAKE_WORDING,
    ),
    (
        "file_capabilities::a_file_beneath_a_symlinked_directory_is_refused",
        FAKE_WORDING,
    ),
    (
        "file_capabilities::a_missing_file_is_an_error_to_read_and_is_created_by_a_write",
        FAKE_WORDING,
    ),
    (
        "file_capabilities::a_missing_parent_fails_every_operation_naming_the_file",
        FAKE_WORDING,
    ),
    (
        "file_capabilities::a_name_that_is_not_a_file_fails_every_operation",
        FAKE_WORDING,
    ),
    (
        "file_capabilities::restrict_to_with_a_file_root_gives_a_broken_directory_capability",
        FAKE_WORDING,
    ),
    (
        "site_toolkit::create_dir_where_a_file_is_or_no_parent_is_an_error",
        FAKE_WORDING,
    ),
    ("text_toolkit::list_dir_of_a_file_is_an_error", FAKE_WORDING),
    (
        "text_toolkit::list_dir_of_a_missing_directory_is_an_error",
        FAKE_WORDING,
    ),
    ("native_corpus::guide-capabilities-0", REAL_CLOCK),
];
