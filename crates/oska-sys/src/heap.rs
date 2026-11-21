//! The musl build's allocator (spec #521, ticket #523; ADR 0009).
//!
//! musl's allocator, mallocng, takes a lock on every `malloc` once a
//! process has a second thread, and gives a group of blocks back to the
//! kernel as soon as its last block is freed, only to map a new one on
//! the next request (`docs/research/2026-11-musl-oskac-performance.md`).
//! The interpreter always runs on a thread of its own and makes tens of
//! millions of short-lived small allocations, so it pays for both.
//! [`SizeClassAlloc`] serves those small requests itself.
//!
//! # How it works
//!
//! - **Size classes.** A request of at most [`MAX_SMALL`] bytes with an
//!   alignment of at most [`ALIGN`] is served from one of [`CLASSES`]
//!   size classes ([`SIZES`]), rounded up to the class's block size.
//!   Every other request goes straight to [`System`], and so does its
//!   `dealloc` and `realloc`. The route is decided from the layout alone
//!   ([`class_of`]), and `GlobalAlloc` passes `dealloc` and `realloc`
//!   the layout the block was allocated with, so a block always goes
//!   back to the allocator it came from.
//! - **Regions.** Blocks are carved from regions of [`REGION`] bytes
//!   that [`System`] allocates. A region is never given back: its blocks
//!   are reused for the life of the process. The allocator makes no
//!   system call of its own.
//! - **No lock on the fast path.** Each thread keeps a free list for
//!   each class in a thread-local [`Cache`]. `alloc` pops from it and
//!   `dealloc` pushes onto it, whichever thread allocated the block: a
//!   block freed on another thread simply joins that thread's list, as
//!   every block of a class is interchangeable. Only when a list is empty
//!   (`alloc`) or holds twice its class's [`batch`] (`dealloc`) does the
//!   thread lock the shared pool ([`with_pool`]), to take or give back one batch of
//!   blocks, or to carve a new batch from the current region. So the
//!   lock is taken at most once per [`batch`] calls, and memory freed on
//!   one thread is reused on another.
//! - **What a thread's exit leaves.** The thread-local cache has no
//!   destructor (registering one may allocate, inside the allocator), so
//!   the blocks on an exiting thread's lists are not reused. That is at
//!   most twice a batch per class, under 4 KiB per class.
//!
//! # Soundness
//!
//! A free block is memory that no live allocation covers. Its first
//! word holds the next block of its list (null at the end); the first
//! block of a batch in the shared pool also holds, in its second word,
//! the next batch. Every block is at least [`ALIGN`] (16) bytes and
//! 16-aligned, so both words fit and are aligned. A free block is
//! reachable from exactly one list: one thread's cache (touched only by
//! that thread) or the shared pool (touched only under its lock). It is
//! handed out by being removed from that list, and returns to a list
//! only through `dealloc` or `realloc`, whose callers promise the
//! program no longer uses it. Regions are carved by a bump pointer under
//! the lock, so no two batches overlap, and no block is ever handed out
//! twice.
//!
//! Nothing here panics or unwinds: no `unwrap`, no overflowing
//! arithmetic (sizes are at most [`REGION`]), and table lookups whose
//! index is bounded by construction. The shared pool's lock is a flag
//! ([`with_pool`]), not `std::sync::Mutex`, so taking it never allocates
//! or panics, and nothing allocates through this allocator while
//! holding it. A request that cannot be served
//! (the system allocator refusing a region or a large block) returns
//! null, as `GlobalAlloc` asks.
//!
//! A child made by `fork` gets a copy of the lock as it was; if another
//! thread held it then, the child must not allocate before `exec`.
//! `std::process::Command`, the only way `oskac` starts a process, does
//! not allocate between the two.
//!
//! The thread-local cache needs native thread-local storage, which every
//! platform Oska builds for has: a `const`-initialised `thread_local!`
//! with no destructor is a plain thread-local variable there, and
//! reading it never allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The largest request, in bytes, served from a size class.
const MAX_SMALL: usize = 1024;

/// The alignment of every block, and the largest alignment a size class
/// serves.
const ALIGN: usize = 16;

/// The number of size classes.
const CLASSES: usize = 20;

/// The block size of each class: a multiple of [`ALIGN`], rising, the
/// last [`MAX_SMALL`]. A request takes the smallest that fits it.
const SIZES: [usize; CLASSES] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024,
];

/// The bytes of each region taken from [`System`].
const REGION: usize = 256 * 1024;

/// A region's layout: page-aligned, so every block in it, at a multiple
/// of [`ALIGN`] from its start, is [`ALIGN`]-aligned.
const REGION_LAYOUT: Layout = match Layout::from_size_align(REGION, 4096) {
    Ok(layout) => layout,
    Err(_) => panic!("a region's layout is valid"),
};

/// The class of each request size, rounded up to a multiple of
/// [`ALIGN`]: entry `n` is the class for `n * ALIGN` bytes (entry 0, for
/// a zero size, is the smallest class).
const CLASS_OF: [u8; MAX_SMALL / ALIGN + 1] = {
    let mut table = [0u8; MAX_SMALL / ALIGN + 1];
    let mut granules = 0;
    while granules < table.len() {
        let mut class = 0;
        while SIZES[class] < granules * ALIGN {
            class += 1;
        }
        // Every entry is a valid class index, which is why indexing with
        // it never panics.
        assert!(class < CLASSES);
        table[granules] = class as u8;
        granules += 1;
    }
    table
};

const _: () = {
    let mut class = 0;
    while class < CLASSES {
        assert!(SIZES[class] % ALIGN == 0);
        assert!(class == 0 || SIZES[class] > SIZES[class - 1]);
        // A block holds the two words a free block uses.
        assert!(SIZES[class] >= 2 * std::mem::size_of::<*mut u8>());
        class += 1;
    }
    assert!(SIZES[CLASSES - 1] == MAX_SMALL);
    assert!(REGION % ALIGN == 0 && REGION >= MAX_SMALL * 64);
};

/// The size class that serves `layout`, or `None` for one that goes to
/// [`System`].
fn class_of(layout: Layout) -> Option<usize> {
    if layout.size() <= MAX_SMALL && layout.align() <= ALIGN {
        Some(CLASS_OF[layout.size().div_ceil(ALIGN)] as usize)
    } else {
        None
    }
}

/// How many blocks of `class` move between a thread's cache and the
/// shared pool at once: about 4 KiB, and between 4 and 64 blocks.
fn batch(class: usize) -> usize {
    (4096 / SIZES[class]).clamp(4, 64)
}

/// The number of regions taken from [`System`] so far, by every
/// [`SizeClassAlloc`] in the process.
static REGIONS: AtomicUsize = AtomicUsize::new(0);

/// The pool every thread shares, behind [`SHARED`]'s lock.
struct Pool {
    /// For each class, the first block of the first batch of free
    /// blocks given back by threads, or null. Each batch is a list of
    /// exactly [`batch`] blocks linked through their first word; its
    /// first block's second word holds the next batch, or null.
    batches: [*mut u8; CLASSES],
    /// The start of the unused part of the current region, or null
    /// before the first region.
    next: *mut u8,
    /// The bytes left in the current region from [`Pool::next`].
    left: usize,
}

/// The [`Pool`] behind a lock of its own: a flag that a thread sets to
/// enter and clears to leave, spinning (and yielding) while another
/// thread is in. Not `std::sync::Mutex`, which on some platforms
/// allocates its system lock on first use, inside the allocator.
struct Shared {
    /// Whether a thread is in [`with_pool`].
    busy: AtomicBool,
    pool: UnsafeCell<Pool>,
}

// SAFETY: `pool` is reached only in `with_pool`, by the one thread that
// set `busy` from false to true (an `Acquire` exchange) and until it
// clears it (a `Release` store), so no two threads touch it at once,
// and each sees every write the one before it made. The pool holds only
// addresses of free blocks and of the unused part of a region, memory
// that no thread and no live allocation owns, so any thread may use it.
unsafe impl Sync for Shared {}

static SHARED: Shared = Shared {
    busy: AtomicBool::new(false),
    pool: UnsafeCell::new(Pool {
        batches: [ptr::null_mut(); CLASSES],
        next: ptr::null_mut(),
        left: 0,
    }),
};

/// Runs `f` on the shared [`Pool`], with no other thread in it.
///
/// `f` must not call `with_pool` (it would wait for itself), and does
/// not: nothing in it allocates through this allocator. It must not
/// panic either, which would leave the lock held; nothing in it does.
fn with_pool<R>(f: impl FnOnce(&mut Pool) -> R) -> R {
    while SHARED
        .busy
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
        std::thread::yield_now();
    }
    // SAFETY: this thread set `busy`, so until it clears it below no
    // other thread reaches `pool` (see `Shared`'s `Sync`), and this is
    // the only reference to it.
    let pool = unsafe { &mut *SHARED.pool.get() };
    let result = f(pool);
    SHARED.busy.store(false, Ordering::Release);
    result
}

/// One thread's free lists.
struct Cache {
    /// For each class, the first free block of this thread's list, or
    /// null. The list is linked through each block's first word.
    heads: [Cell<*mut u8>; CLASSES],
    /// For each class, the number of blocks on this thread's list.
    counts: [Cell<usize>; CLASSES],
}

#[allow(clippy::declare_interior_mutable_const)] // copied into each array slot, never shared
const NO_BLOCK: Cell<*mut u8> = Cell::new(ptr::null_mut());
#[allow(clippy::declare_interior_mutable_const)] // copied into each array slot, never shared
const NO_COUNT: Cell<usize> = Cell::new(0);

thread_local! {
    // `const` and no destructor: a plain thread-local variable, which
    // never allocates and is never torn down while the thread runs.
    static CACHE: Cache = const {
        Cache {
            heads: [NO_BLOCK; CLASSES],
            counts: [NO_COUNT; CLASSES],
        }
    };
}

/// The first word of the free block `block`: the next block of its list.
///
/// # Safety
///
/// `block` is a free block (see the module's "Soundness"), whose first
/// word was written by [`set_next`].
unsafe fn next_of(block: *mut u8) -> *mut u8 {
    // SAFETY: by this function's contract `block` is a free block: at
    // least 16 bytes, 16-aligned, owned by the caller's list, with its
    // first word initialised by `set_next`.
    unsafe { block.cast::<*mut u8>().read() }
}

/// Sets the first word of `block`, a block the caller holds free, to
/// `next`.
///
/// # Safety
///
/// `block` is a block of a size class that no live allocation covers
/// and that only the caller can reach.
unsafe fn set_next(block: *mut u8, next: *mut u8) {
    // SAFETY: by this function's contract `block` is at least 16 bytes,
    // 16-aligned, and no one else reads or writes it now.
    unsafe { block.cast::<*mut u8>().write(next) }
}

/// The second word of `batch`, the first block of a batch in the shared
/// pool: the next batch.
///
/// # Safety
///
/// `batch` heads a batch in [`Pool::batches`], read under the pool's lock,
/// whose second word was written by [`set_next_batch`].
unsafe fn next_batch_of(batch: *mut u8) -> *mut u8 {
    // SAFETY: by this function's contract `batch` is a free block of at
    // least 16 bytes, 16-aligned, reached under the lock, so its second
    // word is in bounds, aligned, initialised, and no one writes it now.
    unsafe { batch.cast::<*mut u8>().wrapping_add(1).read() }
}

/// Sets the second word of `batch`, a free block the caller holds, to
/// `next`.
///
/// # Safety
///
/// As for [`set_next`].
unsafe fn set_next_batch(batch: *mut u8, next: *mut u8) {
    // SAFETY: by this function's contract `batch` is at least 16 bytes,
    // 16-aligned, and only the caller reaches it, so its second word is
    // in bounds, aligned and written by no one else.
    unsafe { batch.cast::<*mut u8>().wrapping_add(1).write(next) }
}

/// Takes a list of free blocks of `class` for a thread whose list is
/// empty: a batch from the pool if there is one, else blocks carved from
/// the current region (or a new one). Returns the list's first block and
/// its length, or null if the system allocator refuses a region.
fn take_batch(class: usize) -> (*mut u8, usize) {
    let size = SIZES[class];
    let wanted = batch(class);
    let (start, count, linked) = with_pool(|pool| {
        let first = pool.batches[class];
        if !first.is_null() {
            // SAFETY: `first` heads a batch in the pool, and this thread
            // holds the pool's lock.
            pool.batches[class] = unsafe { next_batch_of(first) };
            // Its blocks are linked and null-terminated already.
            return (first, wanted, true);
        }
        if pool.left < size {
            // The rest of the old region, smaller than one block, is
            // never used. `System` is the C library's allocator, which
            // never calls back into this one.
            // SAFETY: `REGION_LAYOUT` has a non-zero size.
            let region = unsafe { System.alloc(REGION_LAYOUT) };
            if region.is_null() {
                return (ptr::null_mut(), 0, true);
            }
            REGIONS.fetch_add(1, Ordering::Relaxed);
            pool.next = region;
            pool.left = REGION;
        }
        let count = wanted.min(pool.left / size);
        let start = pool.next;
        pool.next = start.wrapping_add(count * size);
        pool.left -= count * size;
        (start, count, false)
    });
    if linked {
        return (start, count);
    }
    // The `count` blocks from `start` are in the region, past every block
    // carved before and before every block carved later: only this
    // thread can reach them. Link them, outside the lock.
    for at in 0..count {
        let block = start.wrapping_add(at * size);
        let next = if at + 1 < count {
            block.wrapping_add(size)
        } else {
            ptr::null_mut()
        };
        // SAFETY: `block` is one of the blocks just carved, which only
        // this thread can reach.
        unsafe { set_next(block, next) };
    }
    (start, count)
}

impl Cache {
    /// A free block of `class`, from this thread's list, or null.
    fn pop(&self, class: usize) -> *mut u8 {
        let mut head = self.heads[class].get();
        if head.is_null() {
            let (start, count) = take_batch(class);
            if start.is_null() {
                return ptr::null_mut();
            }
            head = start;
            self.counts[class].set(count);
        }
        // SAFETY: `head` is the first block of this thread's list, a free
        // block whose first word was set when it joined the list.
        let next = unsafe { next_of(head) };
        self.heads[class].set(next);
        self.counts[class].set(self.counts[class].get().saturating_sub(1));
        head
    }

    /// Puts `block`, given back by the program, on this thread's list of
    /// `class`, and gives a batch to the shared pool if the list has
    /// grown to two batches.
    ///
    /// # Safety
    ///
    /// `block` is a block of `class` that the program no longer uses.
    unsafe fn push(&self, class: usize, block: *mut u8) {
        // SAFETY: by this function's contract the program no longer uses
        // `block`, a block of `class`, so it is now this thread's.
        unsafe { set_next(block, self.heads[class].get()) };
        self.heads[class].set(block);
        let count = self.counts[class].get() + 1;
        self.counts[class].set(count);
        let batch = batch(class);
        if count >= 2 * batch {
            // Detach the first `batch` blocks of the list.
            let first = block;
            let mut last = first;
            for _ in 1..batch {
                // SAFETY: `last` is one of the first `batch` blocks of
                // this thread's list, which holds `count` > `batch`
                // blocks, so it is a free block with a successor.
                last = unsafe { next_of(last) };
            }
            // SAFETY: `last` is a free block on this thread's list.
            let rest = unsafe { next_of(last) };
            // SAFETY: `last` is a free block on this thread's list, and
            // no other thread can reach it.
            unsafe { set_next(last, ptr::null_mut()) };
            self.heads[class].set(rest);
            self.counts[class].set(count - batch);
            with_pool(|pool| {
                // SAFETY: `first` heads the detached batch, which only
                // this thread can reach until it joins the pool below.
                unsafe { set_next_batch(first, pool.batches[class]) };
                pool.batches[class] = first;
            });
        }
    }
}

/// The allocator of the musl build of `oskac` (ADR 0009): size classes
/// for requests of up to 1 KiB, kept for the life of the process, with no
/// lock on a thread's fast path, and [`System`] for every other request.
/// See the module's documentation for how it works and why it is sound.
///
/// Every value of this type shares the same pool, so it holds no state.
#[derive(Debug, Default)]
pub struct SizeClassAlloc;

impl SizeClassAlloc {
    /// The allocator, for a `#[global_allocator]` static.
    pub const fn new() -> Self {
        SizeClassAlloc
    }

    /// How many regions the size classes have taken from the system
    /// allocator so far, in the whole process: 0 until a small request
    /// is served.
    pub fn regions() -> usize {
        REGIONS.load(Ordering::Relaxed)
    }
}

/// A free block of `class`, or null.
fn alloc_small(class: usize) -> *mut u8 {
    // `try_with` fails only for a thread-local being destroyed, which
    // `CACHE`, with no destructor, never is; null then would be a refusal,
    // not a fault.
    CACHE
        .try_with(|cache| cache.pop(class))
        .unwrap_or(ptr::null_mut())
}

/// Gives `block` back to `class`.
///
/// # Safety
///
/// `block` was returned by [`alloc_small`] for `class`, and the program
/// no longer uses it.
unsafe fn dealloc_small(class: usize, block: *mut u8) {
    // If `try_with` failed (see `alloc_small`), the block would be left
    // unused: a leak, never a fault.
    let _ = CACHE.try_with(|cache| {
        // SAFETY: by this function's contract `block` is a block of
        // `class` that the program no longer uses.
        unsafe { cache.push(class, block) }
    });
}

// SAFETY: `alloc` returns null or a block of at least `layout.size()`
// bytes aligned to `layout.align()` that no live allocation overlaps
// (size classes: the module's "Soundness"; otherwise `System`'s promise).
// `dealloc` and `realloc` route a block by its layout, as `alloc` did,
// so each block goes back to where it came from. No method panics or
// unwinds (the module's "Soundness").
unsafe impl GlobalAlloc for SizeClassAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class_of(layout) {
            Some(class) => alloc_small(class),
            // SAFETY: the caller passes a layout of non-zero size.
            None => unsafe { System.alloc(layout) },
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match class_of(layout) {
            Some(class) => {
                let block = alloc_small(class);
                if !block.is_null() {
                    // SAFETY: `block` is a fresh block of at least
                    // `layout.size()` bytes that only the caller will use.
                    unsafe { block.write_bytes(0, layout.size()) };
                }
                block
            }
            // SAFETY: the caller passes a layout of non-zero size.
            None => unsafe { System.alloc_zeroed(layout) },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match class_of(layout) {
            // SAFETY: the caller passes a block this allocator returned
            // for `layout`, so `alloc_small` returned it for this class,
            // and the program no longer uses it.
            Some(class) => unsafe { dealloc_small(class, ptr) },
            // SAFETY: `layout` has no class, so `System` returned `ptr`
            // for `layout`, which the caller gives back.
            None => unsafe { System.dealloc(ptr, layout) },
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        match (class_of(layout), class_of(new_layout)) {
            // The block already has room for `new_size` bytes, and a
            // later `dealloc` with `new_layout` finds the same class.
            (Some(old), Some(new)) if old == new => ptr,
            // SAFETY: neither layout has a class, so `System` returned
            // `ptr` for `layout`; the caller passes a valid `new_size`.
            (None, None) => unsafe { System.realloc(ptr, layout, new_size) },
            _ => {
                // SAFETY: `new_layout` has a non-zero size, as the caller
                // passes a non-zero `new_size`.
                let moved = unsafe { self.alloc(new_layout) };
                if !moved.is_null() {
                    // SAFETY: `ptr` holds `layout.size()` bytes and
                    // `moved` at least `new_size`, both valid, and they
                    // do not overlap: `moved` is a fresh block and `ptr`
                    // is still live.
                    unsafe { ptr::copy_nonoverlapping(ptr, moved, layout.size().min(new_size)) };
                    // SAFETY: the caller gives back `ptr`, allocated with
                    // `layout`; its bytes are now in `moved`.
                    unsafe { self.dealloc(ptr, layout) };
                }
                moved
            }
        }
    }
}
