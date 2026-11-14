//! Confined filesystem access for the Oska runtime: the one crate in the
//! workspace allowed to use `unsafe` (ADR 0009).
//!
//! `std` cannot open a path beneath a directory handle, and the workspace
//! takes no crates from outside the repo, so this crate declares the
//! C-library functions it needs itself: `openat`, `mkdirat`,
//! `renameat`, `unlinkat`, `fdopendir`, `readdir` and `closedir`, and the
//! address of `errno`.
//! `std` already links that library, so it adds no dependency.
//!
//! It offers one safe type, [`Dir`]: a handle for a directory, beneath
//! which paths are opened one component at a time without following a
//! symlink, to read a file, write one, create a directory or list one,
//! and to rename or remove a file within one directory. Because every
//! component is opened relative to the handle of the one before, and
//! none is followed, there is no separate check for
//! another process to race: a symlink swapped in at any component, at any
//! moment, makes the open fail.
//!
//! On platforms other than Unix the crate is empty, and the runtime keeps
//! its path-based checks.
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

#[cfg(unix)]
pub use unix::Dir;
