// SPDX-License-Identifier: Apache-2.0
//! `memcpy` and `memmove` for the x86_64 musl build (spec #577, ticket
//! #581; ADR 0009).
//!
//! musl's x86_64 `memcpy` copies single bytes up to an 8-byte boundary,
//! then runs `rep movsq`, then copies the last bytes one at a time. A call
//! costs about 13 ns whatever the size, and the interpreter makes tens of
//! millions of short copies (`docs/research/2026-11-musl-oskac-performance.md`,
//! sections 3, 11 and 16). This module replaces both functions on that one
//! target. The glibc build, and every other target, keep their own.
//!
//! # How it copies
//!
//! - **0 to 64 bytes:** loads from the start and from the end of the source,
//!   so that they overlap for sizes between the widths, then stores them.
//!   Every load comes before every store, so this part is correct for
//!   overlapping ranges too and `memmove` shares it.
//! - **65 bytes and up, below [`ERMS_MIN`] or without ERMS:** 64 bytes at a
//!   time with SSE2, the last 64 bytes (or the first 64, going backwards)
//!   loaded before the loop and stored after it.
//! - **[`ERMS_MIN`] bytes and up, with ERMS:** `rep movsb`. The CPU reports
//!   ERMS (enhanced `rep movsb`) in `cpuid` leaf 7; the check runs once.
//! - **`memmove`** copies forwards unless the destination starts inside the
//!   source, and then backwards, by the same SSE2 loop.
//!
//! # Rules the code relies on
//!
//! - The functions must never call `memcpy`, `memmove` or `memset` (the
//!   compiler can turn a copy loop into such a call, and the call would be
//!   this function). Every copy here is a load and a store of a fixed width
//!   (`read_unaligned`, `write_unaligned`) or `rep movsb`. The tests would
//!   overflow the stack at once if this broke. The crate does not use
//!   `#![no_builtins]`: it makes the link fail, because the crate is left
//!   out of LTO and the `alloc` code it uses is never emitted.
//! - Nothing here allocates, locks, panics or unwinds. The one global is an
//!   atomic byte. A caller may be `malloc`'s own `realloc`, or the start-up
//!   code.
//! - The direction flag is clear, as the System V ABI requires at every
//!   call, and `rep movsb` leaves it clear.
//!
//! The functions are defined on musl for x86_64 only. The copy routines
//! themselves compile on every x86_64 target, so that the differential
//! tests (`tests/mem.rs`) run on all of them.

use core::arch::x86_64::{__cpuid, __m128i};
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};

/// From this many bytes, a CPU with ERMS copies with `rep movsb`.
pub const ERMS_MIN: usize = 2048;

/// Which copy routine a call takes for sizes of [`ERMS_MIN`] and up. Only
/// tests choose; the exported functions detect.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemPath {
    /// `rep movsb` if the CPU has ERMS, else SSE2.
    Detect,
    /// `rep movsb`.
    Erms,
    /// SSE2, as on a CPU without ERMS.
    Sse2,
}

/// 0 unknown, 1 ERMS, 2 no ERMS.
static ERMS: AtomicU8 = AtomicU8::new(0);

fn has_erms() -> bool {
    match ERMS.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let yes = detect_erms();
            ERMS.store(if yes { 1 } else { 2 }, Ordering::Relaxed);
            yes
        }
    }
}

/// `cpuid` leaf 7, sub-leaf 0, `ebx` bit 9: enhanced `rep movsb`.
fn detect_erms() -> bool {
    // `cpuid` exists on every x86_64 CPU.
    let highest = __cpuid(0).eax;
    if highest < 7 {
        return false;
    }
    let leaf = __cpuid(7);
    leaf.ebx & (1 << 9) != 0
}

/// Loads a `T` from `p + at`, which need not be aligned.
///
/// # Safety
/// `size_of::<T>()` bytes from `p + at` must be readable.
#[inline(always)]
unsafe fn ld<T>(p: *const u8, at: usize) -> T {
    // SAFETY: the caller promises the bytes are readable; `read_unaligned`
    // has no alignment need, and `T` is a plain integer or vector type.
    unsafe { ptr::read_unaligned(p.wrapping_add(at).cast::<T>()) }
}

/// Stores a `T` at `p + at`, which need not be aligned.
///
/// # Safety
/// `size_of::<T>()` bytes from `p + at` must be writable.
#[inline(always)]
unsafe fn st<T>(p: *mut u8, at: usize, v: T) {
    // SAFETY: the caller promises the bytes are writable; `write_unaligned`
    // has no alignment need.
    unsafe { ptr::write_unaligned(p.wrapping_add(at).cast::<T>(), v) }
}

/// Copies the first and the last `size_of::<T>()` bytes of an `n`-byte
/// range, which together cover it when `n` is at most twice that. Both loads
/// come before both stores.
///
/// # Safety
/// `size_of::<T>() <= n <= 2 * size_of::<T>()`, `n` bytes readable from `s`
/// and writable from `d`.
#[inline(always)]
unsafe fn pair<T>(d: *mut u8, s: *const u8, n: usize) {
    let w = size_of::<T>();
    // SAFETY: `w <= n`, so the first `w` bytes are inside the range.
    let head: T = unsafe { ld(s, 0) };
    // SAFETY: `n - w` does not underflow (`w <= n`) and the `w` bytes from
    // it end at `n`.
    let tail: T = unsafe { ld(s, n - w) };
    // SAFETY: as for the first load, on the destination.
    unsafe { st(d, 0, head) };
    // SAFETY: as for the second load, on the destination.
    unsafe { st(d, n - w, tail) };
}

/// Copies 0 to 64 bytes. Correct when the ranges overlap.
///
/// # Safety
/// `n <= 64`, `n` bytes readable from `s` and writable from `d`.
#[inline(always)]
unsafe fn small(d: *mut u8, s: *const u8, n: usize) {
    if n >= 33 {
        // SAFETY: `n >= 33`, so offsets 0, 16, `n - 32` and `n - 16` each
        // start a readable 16-byte vector. They cover the range because
        // `n - 32 <= 32`.
        let a: __m128i = unsafe { ld(s, 0) };
        // SAFETY: as above.
        let b: __m128i = unsafe { ld(s, 16) };
        // SAFETY: as above.
        let c: __m128i = unsafe { ld(s, n - 32) };
        // SAFETY: as above.
        let e: __m128i = unsafe { ld(s, n - 16) };
        // SAFETY: the same offsets, writable, after every load.
        unsafe { st(d, 0, a) };
        // SAFETY: as above.
        unsafe { st(d, 16, b) };
        // SAFETY: as above.
        unsafe { st(d, n - 32, c) };
        // SAFETY: as above.
        unsafe { st(d, n - 16, e) };
    } else if n >= 17 {
        // SAFETY: `16 <= n <= 32`.
        unsafe { pair::<__m128i>(d, s, n) };
    } else if n >= 9 {
        // SAFETY: `8 <= n <= 16`.
        unsafe { pair::<u64>(d, s, n) };
    } else if n >= 4 {
        // SAFETY: `4 <= n <= 8`.
        unsafe { pair::<u32>(d, s, n) };
    } else if n >= 2 {
        // SAFETY: `2 <= n <= 4`.
        unsafe { pair::<u16>(d, s, n) };
    } else if n == 1 {
        // SAFETY: one byte is readable.
        let b: u8 = unsafe { ld(s, 0) };
        // SAFETY: one byte is writable.
        unsafe { st(d, 0, b) };
    }
}

/// Four vectors, loaded before they are stored.
type Block = [__m128i; 4];

/// Loads 64 bytes.
///
/// # Safety
/// 64 bytes from `s + at` must be readable.
#[inline(always)]
unsafe fn load_block(s: *const u8, at: usize) -> Block {
    // SAFETY: the caller promises it. `Block` is four vectors, 64 bytes.
    unsafe { ld(s, at) }
}

/// Stores 64 bytes.
///
/// # Safety
/// 64 bytes from `d + at` must be writable.
#[inline(always)]
unsafe fn store_block(d: *mut u8, at: usize, b: Block) {
    // SAFETY: the caller promises it.
    unsafe { st(d, at, b) }
}

/// Copies `rep movsb`.
///
/// # Safety
/// `n` bytes readable from `s` and writable from `d`, and the destination
/// does not start after the source inside it (a forward copy is correct).
#[inline(always)]
unsafe fn rep_movsb(d: *mut u8, s: *const u8, n: usize) {
    // SAFETY: the registers are the ones `rep movsb` uses; it reads `n`
    // bytes from `rsi` and writes `n` to `rdi`, ascending, because the
    // direction flag is clear (ABI), and leaves it so.
    unsafe {
        core::arch::asm!(
            "rep movsb",
            inout("rcx") n => _,
            inout("rdi") d => _,
            inout("rsi") s => _,
            options(nostack, preserves_flags),
        );
    }
}

/// Copies more than 64 bytes ascending. Correct when the ranges do not
/// overlap, or the destination starts below the source.
///
/// # Safety
/// `n > 64`, `n` bytes readable from `s` and writable from `d`.
#[inline(always)]
unsafe fn forward(d: *mut u8, s: *const u8, n: usize, erms: bool) {
    if erms && n >= ERMS_MIN {
        // SAFETY: the caller's range; a forward copy is the caller's case.
        unsafe { rep_movsb(d, s, n) };
        return;
    }
    // SAFETY: `n > 64`, so the last 64 bytes are inside the range. Loaded
    // before any store, so an overlapping loop below cannot change them.
    let tail = unsafe { load_block(s, n - 64) };
    let mut i = 0;
    while i + 64 <= n {
        // SAFETY: `i + 64 <= n`.
        let block = unsafe { load_block(s, i) };
        // SAFETY: `i + 64 <= n`. The destination below the source means
        // the earlier stores (below `d + i`) never reach the bytes not yet
        // loaded (from `s + i`).
        unsafe { store_block(d, i, block) };
        i += 64;
    }
    // SAFETY: `n - 64 + 64 = n`. The bytes are the original last 64, which
    // are also the final bytes of the destination.
    unsafe { store_block(d, n - 64, tail) };
}

/// Copies more than 64 bytes descending, for a destination that starts
/// inside the source.
///
/// # Safety
/// `n > 64`, `n` bytes readable from `s` and writable from `d`, `d > s`.
#[inline(always)]
unsafe fn backward(d: *mut u8, s: *const u8, n: usize) {
    // SAFETY: `n > 64`, so the first 64 bytes are inside the range.
    let head = unsafe { load_block(s, 0) };
    let mut i = n;
    while i >= 64 {
        i -= 64;
        // SAFETY: `i + 64 <= n`.
        let block = unsafe { load_block(s, i) };
        // SAFETY: `i + 64 <= n`. The destination above the source means
        // the earlier stores (from `d + i + 64`) never reach the bytes
        // not yet loaded (below `s + i + 64`).
        unsafe { store_block(d, i, block) };
    }
    // SAFETY: `n > 64`. The original first 64 bytes, stored last.
    unsafe { store_block(d, 0, head) };
}

/// `memcpy`'s body: the ranges do not overlap.
///
/// # Safety
/// `n` bytes readable from `s` and writable from `d`, not overlapping.
#[inline(always)]
unsafe fn copy(d: *mut u8, s: *const u8, n: usize, path: MemPath) {
    if n <= 64 {
        // SAFETY: `n <= 64`.
        unsafe { small(d, s, n) };
    } else {
        let erms = match path {
            MemPath::Detect => n >= ERMS_MIN && has_erms(),
            MemPath::Erms => true,
            MemPath::Sse2 => false,
        };
        // SAFETY: `n > 64`; no overlap, so ascending is correct.
        unsafe { forward(d, s, n, erms) };
    }
}

/// `memmove`'s body: the ranges may overlap.
///
/// # Safety
/// `n` bytes readable from `s` and writable from `d`.
#[inline(always)]
unsafe fn relocate(d: *mut u8, s: *const u8, n: usize, path: MemPath) {
    if n <= 64 {
        // SAFETY: `n <= 64`; every load precedes every store.
        unsafe { small(d, s, n) };
    } else if (d as usize).wrapping_sub(s as usize) >= n {
        // The destination is below the source, or the ranges are apart.
        // (Equal pointers give 0, which is below `n`: the backward branch,
        // harmless and rare.)
        let erms = match path {
            MemPath::Detect => n >= ERMS_MIN && has_erms(),
            MemPath::Erms => true,
            MemPath::Sse2 => false,
        };
        // SAFETY: `n > 64`; ascending is correct for this case.
        unsafe { forward(d, s, n, erms) };
    } else if !ptr::eq(d.cast_const(), s) {
        // SAFETY: `n > 64`, and `d > s` (the difference is below `n`).
        unsafe { backward(d, s, n) };
    }
}

/// C's `memcpy` on musl for x86_64.
///
/// # Safety
/// As C's: `n` bytes readable from `s`, writable from `d`, not overlapping.
#[cfg(all(target_os = "linux", target_env = "musl"))]
#[allow(unknown_lints, suspicious_runtime_symbol_definitions)]
#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    // SAFETY: the caller's promise, which is C's contract for `memcpy`.
    unsafe { copy(d, s, n, MemPath::Detect) };
    d
}

/// C's `memmove` on musl for x86_64.
///
/// # Safety
/// As C's: `n` bytes readable from `s` and writable from `d`.
#[cfg(all(target_os = "linux", target_env = "musl"))]
#[allow(unknown_lints, suspicious_runtime_symbol_definitions)]
#[no_mangle]
pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    // SAFETY: the caller's promise, which is C's contract for `memmove`.
    unsafe { relocate(d, s, n, MemPath::Detect) };
    d
}

/// Copies `src` to `dst` with the copy routine, by the chosen path. For
/// the differential tests; the slices cannot overlap.
#[doc(hidden)]
pub fn copy_slice(dst: &mut [u8], src: &[u8], path: MemPath) {
    assert_eq!(dst.len(), src.len(), "copy_slice: lengths differ");
    // SAFETY: both slices span `len` bytes, and Rust's borrow rules keep a
    // `&mut` slice from overlapping a `&` one.
    unsafe { copy(dst.as_mut_ptr(), src.as_ptr(), src.len(), path) };
}

/// Moves `n` bytes from `at_src` to `at_dst` inside `buf`, with the move
/// routine, by the chosen path. For the differential tests; the ranges may
/// overlap.
#[doc(hidden)]
pub fn move_within(buf: &mut [u8], at_dst: usize, at_src: usize, n: usize, path: MemPath) {
    assert!(
        at_dst <= buf.len() && at_src <= buf.len() && n <= buf.len() - at_dst.max(at_src),
        "move_within: range outside the buffer"
    );
    let base = buf.as_mut_ptr();
    // SAFETY: both ranges lie inside `buf` (asserted above), and `base`
    // is derived from the one `&mut` borrow, so reads and writes are both
    // allowed through it.
    unsafe {
        relocate(
            base.wrapping_add(at_dst),
            base.wrapping_add(at_src),
            n,
            path,
        )
    };
}
