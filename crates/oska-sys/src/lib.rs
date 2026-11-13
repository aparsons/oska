//! Confined filesystem access for the Oska runtime: the one crate in the
//! workspace allowed to use `unsafe` (ADR 0009).
//!
//! `std` cannot open a path beneath a directory handle, and the workspace
//! takes no crates from outside the repo, so this crate declares the
//! system functions it needs itself. On Unix they are the C library's
//! `openat`, `mkdirat`, `fdopendir`, `readdir` and `closedir`, and the
//! address of `errno`, which `std` already links. On Windows they are
//! `ntdll`'s `NtCreateFile`, `NtQueryInformationFile`,
//! `NtQueryDirectoryFile` and `RtlNtStatusToDosError`, which every
//! Windows process has loaded. It adds no dependency.
//!
//! It offers one safe type, [`Dir`]: a handle for a directory, beneath
//! which paths are opened one component at a time without following a
//! symlink, or on Windows any other reparse point (a junction or a mount
//! point), to read a file, write one, create a directory or list one.
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
//!   crate. Nothing `unsafe` is public.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::Dir;
#[cfg(windows)]
pub use windows::Dir;

#[cfg(not(any(unix, windows)))]
compile_error!("oska-sys confines file access only on Unix and Windows");
