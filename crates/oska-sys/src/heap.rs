// SPDX-License-Identifier: Apache-2.0
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
//!   Every other request goes to [`System`], and so does its `dealloc`
//!   and `realloc`. The route is decided from the layout alone
//!   ([`route`]), and `GlobalAlloc` passes `dealloc` and `realloc`
//!   the layout the block was allocated with, so a block always goes
//!   back to the allocator it came from.
//! - **Held large blocks.** musl's mallocng maps and unmaps a group of
//!   pages for each request over 1 KiB, so a program that makes and drops
//!   large buffers pays a system call and page faults each time (section
//!   13 of the research note: 21,811 `mmap` calls on the interpreted site
//!   build, against 20 under glibc). A request of more than [`MAX_SMALL`]
//!   and at most [`HOLD_MAX`] bytes, aligned to at most [`ALIGN`], is
//!   rounded up to one of [`LARGE_BUCKETS`] sizes (four per doubling, so
//!   at most a quarter over) and allocated from [`System`] at that
//!   rounded size. When the program frees it, the block is kept on its
//!   bucket's list in the shared pool, up to [`HOLD_COUNT`] blocks a
//!   bucket and [`HOLD_CAP`] bytes in all, and the next request of that
//!   bucket takes it; past either limit it goes back to [`System`]. A
//!   held block is still a [`System`] block, allocated and freed at its
//!   bucket's size and alignment [`ALIGN`], and only ever freed through
//!   `System` with that layout, whichever thread frees it. The shared
//!   pool's lock guards the lists (one short critical section per large
//!   request); the small-block fast path does not touch it. A request
//!   above [`HOLD_MAX`] goes straight to [`System`], never held: it
//!   would be a large part of the cap by itself.
//! - **Regions.** Blocks are carved from regions of [`REGION`] bytes
//!   that [`System`] allocates. A region is never given back: its blocks
//!   are reused for the life of the process. Beyond [`System`], the
//!   allocator calls out only to arm a thread's exit hook (below).
//! - **No lock on the fast path.** Each thread keeps a free list for
//!   each class in a thread-local [`Cache`]. `alloc` pops from it and
//!   `dealloc` pushes onto it, whichever thread allocated the block: a
//!   block freed on another thread simply joins that thread's list, as
//!   every block of a class is interchangeable. Only when a list is empty
//!   (`alloc`) or holds twice its class's [`batch`] (`dealloc`) does the
//!   thread lock the shared pool ([`with_pool`]), to take or give back
//!   one batch of blocks, or to carve a new batch from the current
//!   region. So the lock is taken at most once per [`batch`] calls, and
//!   memory freed on one thread is reused on another.
//! - **A thread's exit gives its lists back, on Linux.** `oskac test`
//!   runs each test block on a thread of its own, so a program can start
//!   and end thousands of threads. On Linux, when a block first joins a
//!   thread's lists, the thread arms an exit hook ([`exit_hook`]): a
//!   `pthread` key with a destructor the C library calls as the thread
//!   ends. The hook moves every list of the thread into the shared
//!   pool's loose lists ([`flush`]), which `take_batch` draws on as it
//!   does on whole batches. `std`'s `thread_local!` destructors are not
//!   used: one registers itself by pushing onto a `Vec`, which
//!   allocates, and if that happened inside this allocator while another
//!   thread-local was registering, `std` would abort. Arming the key
//!   allocates nothing through this allocator. A block that joins a
//!   thread's list after the hook ran (another thread-local's destructor
//!   freeing a value) arms it again, and the C library runs it again, up
//!   to its limit of rounds (4 on musl and glibc). Only blocks freed
//!   after the last round stay on the thread's lists unused: a few, at
//!   most what a thread's last destructors free. A thread whose hook
//!   could not be armed (the system out of keys) strands its lists:
//!   under twice a batch per class, so under 8 KiB per class. Off Linux,
//!   where only tests install this allocator, there is no hook, and
//!   every ending thread strands its lists that way.
//! - **Memory follows each class's peak.** A freed block stays in its
//!   class for the life of the process; it is never split, merged or
//!   handed to another class. So the heap holds, for each class, the
//!   most blocks that class ever had live at once, whatever the other
//!   classes hold now. A program that fills one class, frees it, then
//!   fills the next, and so on, makes the heap the sum of every class's
//!   peak rather than the largest live set: up to [`CLASSES`] (20) times
//!   the memory it ever holds at once, and in the security review's
//!   repro (twenty lists of 100,000 texts, one size class at a time,
//!   section 8 of the research note) about 5 times what the glibc build
//!   takes. `--max-memory` charges a program's values, not this heap,
//!   so it bounds what the program holds at once but no longer bounds
//!   the process's memory by itself.
//! - **Held large blocks add to that.** On top of the classes, the heap
//!   holds up to [`HOLD_CAP`] (8 MiB) of freed large blocks, and each
//!   large block, live or held, takes up to a quarter more than its
//!   request, rounded to its bucket. Both are bounded (8 MiB, and 25% of
//!   the large blocks the program holds), and neither is charged to
//!   `--max-memory`.
//!
//! # Soundness
//!
//! A free block is memory that no live allocation covers. Its first
//! word holds the next block of its list (null at the end); the first
//! block of a batch in the shared pool also holds, in its second word,
//! the next batch. Every block is at least [`ALIGN`] (16) bytes and
//! 16-aligned, so both words fit and are aligned. A free block is
//! reachable from exactly one list: one thread's cache (touched only by
//! that thread, its exit hook included, which runs on the ending thread
//! itself) or the shared pool's batches and loose lists (touched only
//! under its lock). An ending thread's lists move to the pool whole,
//! under the lock, and leave its cache empty. It is
//! handed out by being removed from that list, and returns to a list
//! only through `dealloc` or `realloc`, whose callers promise the
//! program no longer uses it. Regions are carved by a bump pointer under
//! the lock, so no two batches overlap, and no block is ever handed out
//! twice.
//!
//! A held large block is likewise memory no live allocation covers: a
//! block `System` returned for its bucket's layout, at least 1280 bytes
//! and 16-aligned, whose first word holds the next block of its bucket's
//! list, reached only under the pool's lock (the word is written under it
//! too, by `dealloc_large`). It leaves the list when `alloc_large` takes
//! it, and returns to `System` only through `dealloc_large` with its
//! bucket's layout, whether it was held or not, so `System` always sees
//! the size and alignment it allocated with. The bucket's size is up to a
//! quarter above the request; the bytes past the request are never read.
//!
//! Nothing here panics or unwinds: no `unwrap`, no overflowing
//! arithmetic (sizes are at most [`REGION`]), and table lookups whose
//! index is bounded by construction. The shared pool's lock is a flag
//! ([`with_pool`]), not `std::sync::Mutex`, so taking it never allocates
//! or panics, and nothing allocates through this allocator while
//! holding it. A request that cannot be served (the system allocator
//! refusing a region or a large block) returns null, as `GlobalAlloc`
//! asks.
//!
//! A child made by `fork` gets a copy of the lock as it was, and of the
//! exit hook's key ([`hook_key`]); if another thread held the lock, or
//! was making the key, then, the child must not allocate before `exec`.
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

/// The largest request, in bytes, that is held for reuse after it is
/// freed: 1 MiB. Above it a block is a large part of [`HOLD_CAP`] and the
/// kernel's cost per byte is already small.
pub const HOLD_MAX: usize = 1024 * 1024;

/// The most bytes, in all buckets, the allocator holds freed: 8 MiB. The
/// interpreter's buffers that matter are a few KiB to a few hundred KiB
/// (the site build); 8 MiB holds a working set of those and is small next
/// to what a program that needs `oskac` holds anyway.
pub const HOLD_CAP: usize = 8 * 1024 * 1024;

/// The most blocks held in one bucket.
const HOLD_COUNT: usize = 16;

/// The number of large buckets: four sizes for each doubling from 1 KiB
/// to [`HOLD_MAX`] (10 doublings).
pub const LARGE_BUCKETS: usize = 40;

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
        assert!(SIZES[class].is_multiple_of(ALIGN));
        assert!(class == 0 || SIZES[class] > SIZES[class - 1]);
        // A block holds the two words a free block uses.
        assert!(SIZES[class] >= 2 * std::mem::size_of::<*mut u8>());
        class += 1;
    }
    assert!(SIZES[CLASSES - 1] == MAX_SMALL);
    assert!(REGION.is_multiple_of(ALIGN) && REGION >= MAX_SMALL * 64);
    assert!(MAX_SMALL == 1024 && HOLD_MAX == 1 << 20 && LARGE_BUCKETS == 40);
    assert!(bucket_size(0) == 1280 && bucket_size(LARGE_BUCKETS - 1) == HOLD_MAX);
    assert!(HOLD_CAP >= HOLD_MAX);
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

/// Where a request goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    /// A size class.
    Small(usize),
    /// A large bucket: [`System`], at the bucket's size, held when freed.
    Large(usize),
    /// [`System`], at the requested size.
    System,
}

/// The route of `layout`, decided from it alone, so `dealloc` and
/// `realloc`, given the layout a block was allocated with, send it back
/// where `alloc` took it from.
fn route(layout: Layout) -> Route {
    if let Some(class) = class_of(layout) {
        Route::Small(class)
    } else if layout.size() <= HOLD_MAX && layout.align() <= ALIGN {
        Route::Large(large_bucket(layout.size()))
    } else {
        Route::System
    }
}

/// The bucket of a request of `size` bytes, `MAX_SMALL < size <=
/// HOLD_MAX`: the smallest of four sizes per doubling that fits it.
const fn large_bucket(size: usize) -> usize {
    // `size - 1` is at least 1024, so `high` is in 10..=19.
    let high = (usize::BITS - 1 - (size - 1).leading_zeros()) as usize;
    let units = size.div_ceil(1 << (high - 2));
    (high - 10) * 4 + (units - 5)
}

/// The size of a bucket's blocks, in bytes.
const fn bucket_size(bucket: usize) -> usize {
    (5 + bucket % 4) << (bucket / 4 + 8)
}

/// The layout [`System`] sees for each bucket's blocks: always the same
/// size and alignment, for `alloc` and `dealloc` alike. Built at compile
/// time, so none can be invalid and no lookup can fail.
const BUCKET_LAYOUTS: [Layout; LARGE_BUCKETS] = {
    let mut layouts = [REGION_LAYOUT; LARGE_BUCKETS];
    let mut bucket = 0;
    while bucket < LARGE_BUCKETS {
        layouts[bucket] = match Layout::from_size_align(bucket_size(bucket), ALIGN) {
            Ok(layout) => layout,
            Err(_) => panic!("a bucket's layout is valid"),
        };
        bucket += 1;
    }
    layouts
};

/// How many blocks of `class` move between a thread's cache and the
/// shared pool at once: as many blocks as fit in 4 KiB, but at most 64,
/// so a batch is 4 KiB or a little less, except in the classes of 16 to
/// 48 bytes, where the cap of 64 blocks makes it 1 to 3 KiB.
fn batch(class: usize) -> usize {
    (4096 / SIZES[class]).clamp(4, 64)
}

/// The number of regions taken from [`System`] so far, by every
/// [`SizeClassAlloc`] in the process.
static REGIONS: AtomicUsize = AtomicUsize::new(0);

/// A copy of [`Pool::held_bytes`], written under the pool's lock, for
/// [`SizeClassAlloc::held_bytes`].
static HELD: AtomicUsize = AtomicUsize::new(0);

/// The pool every thread shares, behind [`SHARED`]'s lock.
struct Pool {
    /// For each class, the first block of the first batch of free
    /// blocks given back by threads, or null. Each batch is a list of
    /// exactly [`batch`] blocks linked through their first word; its
    /// first block's second word holds the next batch, or null.
    batches: [*mut u8; CLASSES],
    /// For each class, the first block of a list of free blocks given
    /// back by threads that ended ([`flush`]), of any length, linked
    /// through their first word, or null.
    loose: [*mut u8; CLASSES],
    /// The start of the unused part of the current region, or null
    /// before the first region.
    next: *mut u8,
    /// The bytes left in the current region from [`Pool::next`].
    left: usize,
    /// For each large bucket, the first held block, or null; blocks are
    /// linked through their first word.
    held: [*mut u8; LARGE_BUCKETS],
    /// For each large bucket, the number of blocks held.
    held_count: [usize; LARGE_BUCKETS],
    /// The bytes held in all buckets: at most [`HOLD_CAP`].
    held_bytes: usize,
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
        loose: [ptr::null_mut(); CLASSES],
        next: ptr::null_mut(),
        left: 0,
        held: [ptr::null_mut(); LARGE_BUCKETS],
        held_count: [0; LARGE_BUCKETS],
        held_bytes: 0,
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
    /// Whether this thread's exit hook is armed ([`exit_hook::arm`]), or
    /// was tried and could not be: false until a block first joins this
    /// thread's lists, and again once the hook has run.
    armed: Cell<bool>,
}

#[allow(clippy::declare_interior_mutable_const)] // copied into each array slot, never shared
const NO_BLOCK: Cell<*mut u8> = Cell::new(ptr::null_mut());
#[allow(clippy::declare_interior_mutable_const)] // copied into each array slot, never shared
const NO_COUNT: Cell<usize> = Cell::new(0);

thread_local! {
    // `const` and no destructor: a plain thread-local variable, which
    // never allocates and is never torn down while the thread runs, so
    // the allocator works in every thread-local destructor, its own exit
    // hook ([`flush`]) included.
    static CACHE: Cache = const {
        Cache {
            heads: [NO_BLOCK; CLASSES],
            counts: [NO_COUNT; CLASSES],
            armed: Cell::new(false),
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
/// `block` is a block of a size class or a large bucket that no live
/// allocation covers and that only the caller can reach.
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
        let loose = pool.loose[class];
        if !loose.is_null() {
            // Take up to `wanted` blocks from the front of the loose
            // list: a walk of at most 64 blocks.
            let mut last = loose;
            let mut count = 1;
            while count < wanted {
                // SAFETY: `last` is a block of the loose list, a free
                // block, and this thread holds the pool's lock.
                let next = unsafe { next_of(last) };
                if next.is_null() {
                    break;
                }
                last = next;
                count += 1;
            }
            // SAFETY: as above.
            pool.loose[class] = unsafe { next_of(last) };
            // SAFETY: `last` is a free block now detached from the loose
            // list, so only this thread reaches it.
            unsafe { set_next(last, ptr::null_mut()) };
            return (loose, count, true);
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
    /// Arms this thread's exit hook, unless it is armed already: called
    /// whenever blocks join this thread's lists.
    fn arm(&self) {
        if !self.armed.get() {
            // Set first, and kept even if arming fails, so a thread with
            // no hook does not try again on every call.
            self.armed.set(true);
            exit_hook::arm();
        }
    }

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
            self.arm();
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
        self.arm();
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

/// The exit hook ([`exit_hook`]) of an ending thread, the caller: moves
/// every list of its cache into the shared pool's loose lists, and marks
/// the hook as run, so a block that joins a list afterwards arms it
/// again.
#[cfg(target_os = "linux")]
fn flush() {
    let _ = CACHE.try_with(|cache| {
        // Nothing below allocates, so no block joins a list while this
        // runs.
        cache.armed.set(false);
        // An ending thread runs while every other thread runs too, and
        // the C library does not call the hook when the process exits,
        // so whoever holds the lock leaves it.
        with_pool(|pool| {
            for class in 0..CLASSES {
                let first = cache.heads[class].replace(ptr::null_mut());
                cache.counts[class].set(0);
                if first.is_null() {
                    continue;
                }
                // The list is under twice a batch long: a walk of under
                // 128 blocks.
                let mut last = first;
                loop {
                    // SAFETY: `last` is a block of the list just taken
                    // from this thread's cache, a free block that only
                    // this thread reaches.
                    let next = unsafe { next_of(last) };
                    if next.is_null() {
                        break;
                    }
                    last = next;
                }
                // SAFETY: `last` ends that list, which only this thread
                // reaches until it joins the pool here, under the lock.
                unsafe { set_next(last, pool.loose[class]) };
                pool.loose[class] = first;
            }
        });
    });
}

/// The number of a thread's exit hook, made once for the process by
/// `make` (which returns `None` if the system has none to give), kept in
/// [`HOOK_KEY`]: `None` if it could not be made.
#[cfg(target_os = "linux")]
fn hook_key(make: fn() -> Option<usize>) -> Option<usize> {
    loop {
        match HOOK_KEY.compare_exchange(NO_KEY, MAKING_KEY, Ordering::Acquire, Ordering::Acquire) {
            Ok(_) => {
                let state = make().and_then(|key| key.checked_add(FIRST_KEY));
                HOOK_KEY.store(state.unwrap_or(NO_HOOK), Ordering::Release);
                return state.map(|state| state - FIRST_KEY);
            }
            Err(MAKING_KEY) => {
                std::hint::spin_loop();
                std::thread::yield_now();
            }
            Err(NO_HOOK) => return None,
            Err(state) => return state.checked_sub(FIRST_KEY),
        }
    }
}

/// [`HOOK_KEY`] before the key is made.
#[cfg(target_os = "linux")]
const NO_KEY: usize = 0;
/// [`HOOK_KEY`] while one thread makes the key.
#[cfg(target_os = "linux")]
const MAKING_KEY: usize = 1;
/// [`HOOK_KEY`] when the system gave no key.
#[cfg(target_os = "linux")]
const NO_HOOK: usize = 2;
/// [`HOOK_KEY`] holds the key plus this, once it is made.
#[cfg(target_os = "linux")]
const FIRST_KEY: usize = 3;

/// The state of the process's exit-hook key ([`hook_key`]).
#[cfg(target_os = "linux")]
static HOOK_KEY: AtomicUsize = AtomicUsize::new(NO_KEY);

/// A thread's exit hook on Linux: a `pthread` key whose destructor
/// ([`flush`]) the C library calls as a thread that set it ends, and
/// again, up to `PTHREAD_DESTRUCTOR_ITERATIONS` (4) rounds, if the
/// thread set it again meanwhile. The C library never calls this
/// allocator, so neither call here allocates through it. Not called when
/// the process exits, so it never waits for a lock that a thread ended
/// by `exit` held.
#[cfg(target_os = "linux")]
mod exit_hook {
    use super::hook_key;
    use std::ffi::c_void;
    use std::os::raw::c_int;
    use std::ptr::NonNull;

    /// `pthread_key_t`, on glibc and musl.
    type Key = std::os::raw::c_uint;

    extern "C" {
        /// `int pthread_key_create(pthread_key_t *key, void
        /// (*destructor)(void *))`.
        fn pthread_key_create(
            key: *mut Key,
            destructor: Option<extern "C" fn(*mut c_void)>,
        ) -> c_int;
        /// `int pthread_setspecific(pthread_key_t key, const void
        /// *value)`.
        fn pthread_setspecific(key: Key, value: *const c_void) -> c_int;
    }

    /// Makes the process's key, with [`run`] as its destructor.
    fn make() -> Option<usize> {
        let mut key: Key = 0;
        // SAFETY: `key` is a valid place for the new key, and `run` may be
        // called with any value.
        let made = unsafe { pthread_key_create(&mut key, Some(run)) };
        if made == 0 {
            usize::try_from(key).ok()
        } else {
            None
        }
    }

    /// Arms the calling thread's hook: sets its value of the key to a
    /// marker, never read, only not null, as the destructor runs only
    /// for a thread whose value is not null.
    pub fn arm() {
        let Some(key) = hook_key(make).and_then(|key| Key::try_from(key).ok()) else {
            return;
        };
        // SAFETY: `key` was made by `pthread_key_create` and never deleted.
        let _ = unsafe { pthread_setspecific(key, NonNull::<c_void>::dangling().as_ptr()) };
    }

    /// The key's destructor.
    extern "C" fn run(_marker: *mut c_void) {
        super::flush();
    }
}

/// No exit hook off Linux, where only tests install this allocator (the
/// musl build of `oskac` is the one that ships with it): an ending
/// thread's lists stay unused there. On macOS a `pthread` key's
/// destructor was tried and freed nothing: the likely cause is that
/// dyld's thread-local storage, which holds the cache, is torn down by
/// its own `pthread` key destructor, made before this one and so run
/// first, leaving this one an empty cache.
#[cfg(not(target_os = "linux"))]
mod exit_hook {
    /// Does nothing.
    pub fn arm() {}
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

    /// How many bytes of freed large blocks the allocator holds for
    /// reuse right now, in the whole process: never more than
    /// [`HOLD_CAP`].
    pub fn held_bytes() -> usize {
        HELD.load(Ordering::Relaxed)
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

/// A block of `bucket`, held or fresh from [`System`], zeroed in its
/// first `zero` bytes, or null.
fn alloc_large(bucket: usize, zero: usize) -> *mut u8 {
    let size = bucket_size(bucket);
    let held = with_pool(|pool| {
        let block = pool.held[bucket];
        if !block.is_null() {
            // SAFETY: `block` heads this bucket's list, a held block whose
            // first word `dealloc_large` set, and this thread holds the
            // lock.
            pool.held[bucket] = unsafe { next_of(block) };
            pool.held_count[bucket] -= 1;
            pool.held_bytes -= size;
            HELD.store(pool.held_bytes, Ordering::Relaxed);
        }
        block
    });
    if !held.is_null() {
        if zero > 0 {
            // SAFETY: `held` is a block of `size` >= `zero` bytes that
            // only this thread reaches now.
            unsafe { held.write_bytes(0, zero) };
        }
        return held;
    }
    let layout = BUCKET_LAYOUTS[bucket];
    if zero > 0 {
        // SAFETY: `layout` has a non-zero size.
        unsafe { System.alloc_zeroed(layout) }
    } else {
        // SAFETY: `layout` has a non-zero size.
        unsafe { System.alloc(layout) }
    }
}

/// Holds `block` of `bucket` for reuse, or gives it back to [`System`]
/// when the bucket or the whole is full.
///
/// # Safety
///
/// `block` was returned by [`alloc_large`] for `bucket`, and the program
/// no longer uses it.
unsafe fn dealloc_large(bucket: usize, block: *mut u8) {
    let size = bucket_size(bucket);
    let kept = with_pool(|pool| {
        if pool.held_count[bucket] >= HOLD_COUNT || pool.held_bytes + size > HOLD_CAP {
            return false;
        }
        // SAFETY: by this function's contract the program no longer uses
        // `block`, which is at least 1280 bytes and 16-aligned.
        unsafe { set_next(block, pool.held[bucket]) };
        pool.held[bucket] = block;
        pool.held_count[bucket] += 1;
        pool.held_bytes += size;
        HELD.store(pool.held_bytes, Ordering::Relaxed);
        true
    });
    if !kept {
        // SAFETY: `System` returned `block` for this layout in
        // `alloc_large`, and the program no longer uses it.
        unsafe { System.dealloc(block, BUCKET_LAYOUTS[bucket]) };
    }
}

// SAFETY: `alloc` returns null or a block of at least `layout.size()`
// bytes aligned to `layout.align()` that no live allocation overlaps
// (size classes: the module's "Soundness"; large buckets and the rest:
// `System`'s promise, a held block being one `System` returned that no
// one else holds). `dealloc` and `realloc` route a block by its layout,
// as `alloc` did, so each block goes back to where it came from. No
// method panics or unwinds (the module's "Soundness").
unsafe impl GlobalAlloc for SizeClassAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match route(layout) {
            Route::Small(class) => alloc_small(class),
            Route::Large(bucket) => alloc_large(bucket, 0),
            // SAFETY: the caller passes a layout of non-zero size.
            Route::System => unsafe { System.alloc(layout) },
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match route(layout) {
            Route::Small(class) => {
                let block = alloc_small(class);
                if !block.is_null() {
                    // SAFETY: `block` is a fresh block of at least
                    // `layout.size()` bytes that only the caller will use.
                    unsafe { block.write_bytes(0, layout.size()) };
                }
                block
            }
            Route::Large(bucket) => alloc_large(bucket, layout.size()),
            // SAFETY: the caller passes a layout of non-zero size.
            Route::System => unsafe { System.alloc_zeroed(layout) },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match route(layout) {
            // SAFETY: the caller passes a block this allocator returned
            // for `layout`, so `alloc_small` returned it for this class,
            // and the program no longer uses it.
            Route::Small(class) => unsafe { dealloc_small(class, ptr) },
            // SAFETY: as above, with `alloc_large` and this bucket.
            Route::Large(bucket) => unsafe { dealloc_large(bucket, ptr) },
            // SAFETY: `layout` has no class or bucket, so `System`
            // returned `ptr` for `layout`, which the caller gives back.
            Route::System => unsafe { System.dealloc(ptr, layout) },
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        match (route(layout), route(new_layout)) {
            // The block already has room for `new_size` bytes, and a
            // later `dealloc` with `new_layout` finds the same class or
            // bucket.
            (Route::Small(old), Route::Small(new)) if old == new => ptr,
            (Route::Large(old), Route::Large(new)) if old == new => ptr,
            // SAFETY: neither layout has a class or bucket, so `System`
            // returned `ptr` for `layout`; the caller passes a valid
            // `new_size`.
            (Route::System, Route::System) => unsafe { System.realloc(ptr, layout, new_size) },
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
