//! Confined filesystem access for the Oska runtime: the one crate in the
//! workspace allowed to use `unsafe` (ADR 0009).
//!
//! `std` cannot open a path beneath a directory handle, and the workspace
//! takes no crates from outside the repo, so this crate declares the
//! system functions it needs itself. On Unix they are the C library's
//! `openat`, `mkdirat`, `renameat`, `unlinkat`, `fdopendir`, `readdir`
//! and `closedir`, the address of `errno`, and `geteuid`, which `std`
//! already links.
//! On Windows they are `ntdll`'s `NtCreateFile`, `NtQueryInformationFile`,
//! `NtQueryDirectoryFile` and `RtlNtStatusToDosError`, which every
//! Windows process has loaded. It adds no dependency.
//!
//! It offers one safe type, [`Dir`], and on Unix the function `effective_uid`. A
//! [`Dir`] is a handle for a directory, beneath
//! which paths are opened one component at a time without following a
//! symlink, or on Windows any other reparse point (a junction or a mount
//! point), to read a file, write one, create a directory or list one, and
//! on Unix to rename or remove a file within one directory.
//! A file with more than one link is refused. Because every component is
//! opened relative to the handle of the one before, and none is followed,
//! there is no separate check for another process to race: a symlink or
//! junction swapped in at any component, at any moment, makes the open
//! fail. On Windows the refusals are worded as on Unix.
//!
//! # Rules for `unsafe` here
//!
//! - Every `unsafe` block holds one unsafe operation and starts with a
//!   `// SAFETY:` comment stating the invariant it relies on (clippy's
//!   `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block`).
//! - Every foreign function is called only from a safe wrapper in this
//!   crate. Nothing `unsafe` is public but the [`GlobalAlloc`] trait's
//!   own methods on [`memory::Counting`], which only another crate's
//!   `unsafe` could call, and every other crate forbids it.
//!
//! It also holds [`memory`]: the system allocator with a count of each
//! thread's heap bytes beside it, for the interpreter's `--max-memory`
//! run limit (spec #446), since an allocator is `unsafe` to write.
//!
//! [`GlobalAlloc`]: std::alloc::GlobalAlloc

pub mod memory;
mod refusal;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
mod wording;

pub use wording::{
    io_error_message, system_wording, wording, IO_KINDS, LINUX_ERRORS, MACOS_ERRORS, OTHER_WORDING,
    WINDOWS_ERRORS,
};

#[cfg(unix)]
pub use unix::{effective_uid, Dir};
#[cfg(windows)]
pub use windows::Dir;

#[cfg(not(any(unix, windows)))]
compile_error!("oska-sys confines file access only on Unix and Windows");
