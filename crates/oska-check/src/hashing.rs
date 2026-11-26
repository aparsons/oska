// SPDX-License-Identifier: Apache-2.0
//! The fixed hash of `Hash` (ADR 0028): the one definition the interpreter
//! and the native backend both follow, and the constants the backend folds.
//!
//! It is 64-bit FNV-1a, with no seed, so every platform and both backends
//! give the same value:
//!
//! - text and `Bytes` hash as FNV-1a of their bytes ([`bytes`]);
//! - an integer or `Bool` hashes as FNV-1a of the 8 little-endian bytes of
//!   its 64-bit value, from the offset basis ([`word`]);
//! - an `Option`, a struct or an enum hashes as the state [`word`] of its
//!   variant's index (a struct is variant 0, the first `Option` variant
//!   `Some` is 0), then [`mix`] of each field's hash, in order;
//! - a `List` hashes as the state [`word`] of its length, then [`mix`] of
//!   each item's hash, in order.

/// FNV-1a's offset basis, 0xcbf29ce484222325.
pub const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a's prime, 0x100000001b3.
pub const PRIME: u64 = 0x0000_0100_0000_01b3;

/// The state `state` after FNV-1a takes the 8 little-endian bytes of `word`.
pub fn mix(state: u64, word: u64) -> u64 {
    word.to_le_bytes()
        .iter()
        .fold(state, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
}

/// The hash of an integer or `Bool`, whose 64-bit value is `word`.
pub fn word(word: u64) -> u64 {
    mix(OFFSET, word)
}

/// The FNV-1a hash of `bytes`: of a `String`, a `Path`'s text or a `Bytes`.
pub fn bytes(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
}
