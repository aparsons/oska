// SPDX-License-Identifier: Apache-2.0
//! Differential test of the x86_64 `memcpy` and `memmove` routines
//! (spec #577, ticket #581): every size from 0 to 4 KiB, sampled sizes up
//! to 1 MiB, several alignments, both overlap directions, and both large
//! paths (`rep movsb` as on a CPU with ERMS, SSE2 as on one without), each
//! against a byte-by-byte reference. The routines compile on every x86_64
//! target; only the musl build also exports them as the C symbols
//! (`tests/mem_symbols.rs`).

#![cfg(target_arch = "x86_64")]

use oska_sys::{copy_slice, move_within, MemPath, ERMS_MIN};
use std::hint::black_box;

const PATHS: [MemPath; 3] = [MemPath::Detect, MemPath::Erms, MemPath::Sse2];

/// Sizes beyond 4 KiB, around the thresholds and powers of two.
const SAMPLED: [usize; 20] = [
    4097,
    5000,
    8191,
    8192,
    8193,
    16_387,
    32_768,
    65_535,
    65_536,
    65_537,
    100_003,
    131_072,
    262_144,
    262_147,
    524_287,
    524_288,
    777_777,
    1_048_575,
    1_048_576,
    ERMS_MIN * 3 + 1,
];

/// The bytes of a test pattern: no repeat within 256 bytes, no run.
fn pattern(len: usize, seed: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(131).wrapping_add(seed * 17 + 7) >> 1) as u8 ^ (i >> 8) as u8)
        .collect()
}

/// A byte-by-byte copy that the compiler cannot turn into a call to
/// `memcpy` (which on musl is the routine under test).
fn reference_copy(dst: &mut [u8], src: &[u8]) {
    for (to, from) in dst.iter_mut().zip(src) {
        *to = black_box(*from);
    }
}

const GUARD: usize = 80;
const FILL: u8 = 0xA5;

/// Copies `n` bytes at source offset `so` to destination offset `dof` (each
/// inside its own buffer, with guard bytes around the destination) by
/// `path`, and checks the bytes and the guards.
fn check_copy(n: usize, so: usize, dof: usize, path: MemPath) {
    let src_all = pattern(n + so + 8, n + so);
    let src = &src_all[so..so + n];
    let mut dst_all = vec![FILL; GUARD + dof + n + GUARD];
    let at = GUARD + dof;
    copy_slice(&mut dst_all[at..at + n], src, path);
    let mut want = vec![0u8; n];
    reference_copy(&mut want, src);
    assert!(
        dst_all[at..at + n] == want[..],
        "copy of {n} bytes (src +{so}, dst +{dof}, {path:?}) is wrong"
    );
    assert!(
        dst_all[..at].iter().all(|&b| b == FILL),
        "copy of {n} bytes (src +{so}, dst +{dof}, {path:?}) wrote before the destination"
    );
    assert!(
        dst_all[at + n..].iter().all(|&b| b == FILL),
        "copy of {n} bytes (src +{so}, dst +{dof}, {path:?}) wrote after the destination"
    );
}

/// Moves `n` bytes from offset `src` to offset `dst` of one buffer by
/// `path`, and checks the whole buffer against a reference.
fn check_move(len: usize, n: usize, dst: usize, src: usize, path: MemPath) {
    let original = pattern(len, dst * 3 + src);
    let mut buf = original.clone();
    move_within(&mut buf, dst, src, n, path);
    let mut want = original.clone();
    let mut staged = vec![0u8; n];
    reference_copy(&mut staged, &original[src..src + n]);
    reference_copy(&mut want[dst..dst + n], &staged);
    assert!(
        buf == want,
        "move of {n} bytes from {src} to {dst} in {len} ({path:?}) is wrong"
    );
}

#[test]
fn copies_every_size_up_to_4_kib() {
    // Offsets that cross 8, 16 and 64 byte boundaries, in pairs.
    const OFFSETS: [(usize, usize); 8] = [
        (0, 0),
        (1, 0),
        (0, 3),
        (7, 9),
        (15, 1),
        (16, 31),
        (63, 5),
        (64, 65),
    ];
    for n in 0..=4096 {
        for path in PATHS {
            if n <= 160 {
                for (so, dof) in OFFSETS {
                    check_copy(n, so, dof, path);
                }
            } else {
                let (so, dof) = OFFSETS[n % OFFSETS.len()];
                check_copy(n, so, dof, path);
            }
        }
    }
}

#[test]
fn copies_sampled_sizes_up_to_1_mib() {
    for n in SAMPLED {
        for path in PATHS {
            check_copy(n, 0, 0, path);
            check_copy(n, 3, 5, path);
            check_copy(n, 33, 1, path);
        }
    }
}

#[test]
fn small_moves_work_at_every_overlap() {
    // Every size to 200 bytes, every shift of the destination from 40
    // bytes below the source to 40 above it, both ways.
    for n in 0..=200 {
        for shift in 0..=40usize {
            for path in PATHS {
                check_move(n + 128, n, 48 + shift, 48, path);
                check_move(n + 128, n, 48, 48 + shift, path);
            }
        }
    }
}

#[test]
fn moves_overlap_forwards_and_backwards() {
    // Sizes around every step of the routine and past the `rep movsb`
    // threshold, with shifts of 1 byte up to a whole block and more.
    let sizes = [
        65,
        100,
        127,
        128,
        129,
        1000,
        2047,
        ERMS_MIN,
        ERMS_MIN + 1,
        4096,
        5000,
        70_001,
    ];
    let shifts = [
        1usize, 2, 3, 7, 8, 15, 16, 17, 31, 32, 63, 64, 65, 127, 1000,
    ];
    for n in sizes {
        for shift in shifts {
            for path in PATHS {
                let len = n + 2 * shift + 64;
                // Destination above the source (copies backwards), and below it.
                check_move(len, n, 32 + shift, 32, path);
                check_move(len, n, 32, 32 + shift, path);
            }
        }
        // Equal ranges, disjoint ranges, and a destination just past the end.
        check_move(2 * n + 64, n, 8, 8, MemPath::Detect);
        check_move(2 * n + 64, n, 0, n + 16, MemPath::Erms);
        check_move(2 * n + 64, n, n + 16, 0, MemPath::Sse2);
    }
}

#[test]
fn moves_sampled_sizes_up_to_1_mib() {
    for n in SAMPLED {
        for shift in [1usize, 17, 4096] {
            for path in PATHS {
                let len = n + shift + 16;
                check_move(len, n, shift, 0, path);
                check_move(len, n, 0, shift, path);
            }
        }
    }
}
