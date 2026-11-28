// SPDX-License-Identifier: Apache-2.0
//! What [`crate::Dir::metadata`] reports about a name: its kind and its
//! size, and nothing else. Times and permissions differ on every
//! platform, so none is offered.

/// What a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// Anything else, such as a FIFO or a device.
    Other,
}

/// The kind of a name and, for a regular file, its size in bytes (0 for
/// anything else).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    pub kind: Kind,
    pub size: u64,
}
