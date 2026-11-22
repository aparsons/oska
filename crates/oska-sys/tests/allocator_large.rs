//! The musl build's allocator holds freed large blocks (ticket #614, spec
//! #612): a request above the small-block limit (1 KiB) and up to
//! `HOLD_MAX` (1 MiB) is kept when freed, up to `HOLD_CAP` bytes in all,
//! and reused by the next request of its size, instead of going back to
//! the system allocator at once (musl's allocator maps and unmaps pages
//! for each such block).
//!
//! These tests use only safe code (the unsafe guard compiles every
//! crate's tests with `unsafe` forbidden): `Vec`'s capacity requests are
//! the layouts, `Vec::with_capacity(n)` an `alloc` of `n` bytes,
//! `vec![0; n]` an `alloc_zeroed`, `reserve_exact` and `shrink_to_fit` a
//! `realloc`. They check:
//!
//! - **The cap:** many threads allocate, grow, shrink and free large
//!   blocks, and the held total never passes `HOLD_CAP`.
//! - **Zeroed memory after reuse:** a block that was dirtied, freed and
//!   taken again as a zeroed request is zero.
//! - **The per-block limit:** a block above `HOLD_MAX` goes straight to
//!   the system allocator, and is never held.
//! - **A block goes back where it came from:** a large block is held when
//!   freed and reused, a small block is not held and is reused from its
//!   size class, and a block grown from small to large, and back, keeps
//!   its bytes and returns to the right place.
//!
//! - **Alignment:** a block aligned past 16 bytes is never held, whatever
//!   its size.
//!
//! The held total is process-wide, so every test holds [`SERIAL`], and
//! the ones that measure it first [`drain`] the pool, so a test that ran
//! before cannot leave it near the cap.

use oska_sys::{SizeClassAlloc, HOLD_CAP, HOLD_MAX};
use std::sync::Mutex;

#[global_allocator]
static HEAP: SizeClassAlloc = SizeClassAlloc::new();

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Takes every held block out of the pool, by requesting each bucket's
/// size until a request no longer lowers the held total, and returns them
/// so the caller keeps them live. With them out, the pool is empty and
/// has all of `HOLD_CAP` and every bucket's count to spare.
fn drain() -> Vec<Vec<u8>> {
    // Room for every block, so that growing this list frees nothing.
    let mut taken = Vec::with_capacity(4096);
    for bucket in 0..40 {
        let size = (5 + bucket % 4) << (bucket / 4 + 8);
        loop {
            let before = SizeClassAlloc::held_bytes();
            taken.push(Vec::<u8>::with_capacity(size));
            if SizeClassAlloc::held_bytes() == before {
                break;
            }
        }
    }
    assert_eq!(SizeClassAlloc::held_bytes(), 0);
    taken
}

/// A buffer of capacity exactly `size`, holding `size` copies of `tag`.
fn filled(size: usize, tag: u8) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(size);
    buffer.resize(size, tag);
    buffer
}

/// Grows or shrinks `buffer` to exactly `size` bytes, a `realloc` of its
/// block, the new bytes `tag`.
fn resized(buffer: &mut Vec<u8>, size: usize, tag: u8) {
    if size > buffer.len() {
        buffer.reserve_exact(size - buffer.len());
        buffer.resize(size, tag);
    } else {
        buffer.truncate(size);
        buffer.shrink_to_fit();
    }
}

#[test]
fn a_zeroed_request_is_zero_after_a_dirtied_block_is_reused() {
    let _guard = serial();
    let size = 100_000;
    let first = filled(size, 0xAA);
    let address = first.as_ptr();
    drop(first);
    let second = vec![0u8; size];
    assert_eq!(
        second.as_ptr(),
        address,
        "the freed block was held and handed out again"
    );
    assert!(second.iter().all(|&byte| byte == 0), "reused block is zero");
}

#[test]
fn a_block_above_the_per_block_limit_goes_straight_to_the_system() {
    let _guard = serial();
    let _live = drain();
    let before = SizeClassAlloc::held_bytes();
    drop(filled(HOLD_MAX + 1, 1));
    assert_eq!(SizeClassAlloc::held_bytes(), before, "nothing was held");
    // The largest held size is held, and taken again.
    let block = filled(HOLD_MAX, 1);
    let address = block.as_ptr();
    drop(block);
    assert_eq!(SizeClassAlloc::held_bytes(), before + HOLD_MAX);
    let again: Vec<u8> = Vec::with_capacity(HOLD_MAX);
    assert_eq!(again.as_ptr(), address);
    assert_eq!(SizeClassAlloc::held_bytes(), before);
}

#[test]
fn a_block_goes_back_to_the_allocator_it_came_from_large_and_small() {
    let _guard = serial();
    let _live = drain();
    let before = SizeClassAlloc::held_bytes();

    // Small: freed into its size class, never held, reused from it.
    let small = filled(200, 1);
    let address = small.as_ptr();
    drop(small);
    assert_eq!(SizeClassAlloc::held_bytes(), before);
    let again = filled(200, 2);
    assert_eq!(
        again.as_ptr(),
        address,
        "a small block returns to its class"
    );
    drop(again);

    // Large: held when freed, taken again by the same size, and not by a
    // small request.
    let large = filled(5000, 1);
    let address = large.as_ptr();
    drop(large);
    assert!(SizeClassAlloc::held_bytes() >= before + 5000);
    let other = filled(200, 3);
    assert_ne!(
        other.as_ptr(),
        address,
        "a small request never takes a large block"
    );
    drop(other);
    let again = filled(5000, 2);
    assert_eq!(
        again.as_ptr(),
        address,
        "a large block returns to its bucket"
    );
    drop(again);

    // Across the boundary: small to large to above the limit and back,
    // each step keeps the bytes and frees through its own route.
    let mut buffer = filled(100, 0x5C);
    for size in [2000, 700_000, 3_000_000, 900_000, 3000, 100] {
        let kept = buffer.len().min(size);
        resized(&mut buffer, size, 0x5C);
        assert_eq!(buffer.len(), size);
        assert!(buffer[..kept].iter().all(|&byte| byte == 0x5C), "to {size}");
    }
    drop(buffer);
    assert!(SizeClassAlloc::held_bytes() <= HOLD_CAP);
}

/// 32-aligned, 32 bytes.
#[repr(align(32))]
#[allow(dead_code)] // only its size and alignment matter
struct Wide([u8; 32]);

#[test]
fn a_block_aligned_past_16_bytes_goes_straight_to_the_system() {
    let _guard = serial();
    let _live = drain();
    // 64 * 32 = 2048 bytes, in the range that is held at an alignment of
    // 16 or less.
    let wide: Vec<Wide> = Vec::with_capacity(64);
    assert_eq!(wide.as_ptr() as usize % 32, 0);
    assert_eq!(wide.capacity() * std::mem::size_of::<Wide>(), 2048);
    drop(wide);
    assert_eq!(SizeClassAlloc::held_bytes(), 0, "nothing was held");
    drop(Vec::<u8>::with_capacity(2048));
    assert_eq!(
        SizeClassAlloc::held_bytes(),
        2048,
        "a 16-aligned block of that size is held"
    );
}

#[test]
fn a_realloc_within_one_bucket_stays_in_place() {
    let _guard = serial();
    let _live = drain();
    // 1100 and 1200 both round up to the 1280-byte bucket.
    let mut buffer = filled(1100, 7);
    let address = buffer.as_ptr();
    resized(&mut buffer, 1200, 7);
    assert_eq!(buffer.as_ptr(), address);
}

/// A small xorshift, so each thread's sequence differs.
fn next(state: &mut u64) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as usize
}

#[test]
fn threads_that_allocate_grow_and_free_large_blocks_never_hold_past_the_cap() {
    let _guard = serial();
    let handles: Vec<_> = (1..=4u64)
        .map(|id| {
            std::thread::spawn(move || {
                let mut state = id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let mut live: Vec<(Vec<u8>, u8)> = Vec::new();
                let mut peak = 0;
                for _ in 0..3_000 {
                    peak = peak.max(SizeClassAlloc::held_bytes());
                    match next(&mut state) % 3 {
                        0 | 1 if live.len() < 16 => {
                            let size = 1025 + next(&mut state) % (2 * HOLD_MAX);
                            let tag = (size % 251) as u8 + 1;
                            let buffer = if size % 2 == 0 {
                                let mut zeroed = vec![0u8; size];
                                assert!(zeroed.iter().step_by(4099).all(|&byte| byte == 0));
                                zeroed.fill(tag);
                                zeroed
                            } else {
                                filled(size, tag)
                            };
                            live.push((buffer, tag));
                        }
                        2 if !live.is_empty() => {
                            let at = next(&mut state) % live.len();
                            let (mut buffer, tag) = live.swap_remove(at);
                            assert!(buffer.iter().step_by(4099).all(|&byte| byte == tag));
                            if next(&mut state) % 2 == 0 {
                                drop(buffer);
                            } else {
                                let size = 1 + next(&mut state) % (2 * HOLD_MAX);
                                let kept = buffer.len().min(size);
                                resized(&mut buffer, size, tag);
                                assert!(buffer[..kept]
                                    .iter()
                                    .step_by(4099)
                                    .all(|&byte| byte == tag));
                                live.push((buffer, tag));
                            }
                        }
                        _ => {}
                    }
                }
                peak
            })
        })
        .collect();
    for handle in handles {
        let peak = handle.join().expect("a worker ran to the end");
        assert!(peak <= HOLD_CAP, "held {peak} bytes, cap {HOLD_CAP}");
    }
    assert!(SizeClassAlloc::held_bytes() <= HOLD_CAP);
}
