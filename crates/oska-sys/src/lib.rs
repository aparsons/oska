// SPDX-License-Identifier: Apache-2.0
//! Confined filesystem access for the Oska runtime, and the musl build's
//! allocator: the one crate in the workspace allowed to use `unsafe`
//! (ADR 0009).
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
//! It also offers [`NetStream`] and [`NetListener`], TCP connections to one
//! host and port and a loopback listener, worded the same on every platform.
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
//! It also offers [`SizeClassAlloc`], the global allocator of the musl
//! build of `oskac` (spec #521): size classes for small requests over
//! memory from `std`'s system allocator, with no lock on a thread's fast
//! path. Its only `unsafe` interface is the `GlobalAlloc` trait, which
//! `oskac` names in a `#[global_allocator]` static without writing any
//! `unsafe` itself.
//!
//! # Rules for `unsafe` here
//!
//! - Every `unsafe` block holds one unsafe operation and starts with a
//!   `// SAFETY:` comment stating the invariant it relies on (clippy's
//!   `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block`).
//! - Every foreign function is called only from a safe wrapper in this
//!   crate. Nothing `unsafe` is public, except the methods of the
//!   standard `GlobalAlloc` trait on [`SizeClassAlloc`] (ADR 0009), which
//!   only the compiler's `#[global_allocator]` glue calls.

mod heap;
#[cfg(target_arch = "x86_64")]
mod mem;
mod net;
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

pub use heap::{SizeClassAlloc, HOLD_CAP, HOLD_MAX};
#[cfg(target_arch = "x86_64")]
#[doc(hidden)]
pub use mem::{copy_slice, move_within, MemPath, ERMS_MIN};
pub use net::{exchange_datagram, NetListener, NetStream};
#[cfg(unix)]
pub use unix::system_name_servers;
#[cfg(unix)]
pub use unix::{effective_uid, fill_random, Dir};
#[cfg(windows)]
pub use windows::system_name_servers;
#[cfg(windows)]
pub use windows::{fill_random, Dir};

#[cfg(not(any(unix, windows)))]
compile_error!("oska-sys confines file access only on Unix and Windows");
