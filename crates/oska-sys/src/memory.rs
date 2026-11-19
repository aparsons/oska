//! The heap bytes a thread holds, net of frees, for the interpreter's
//! `--max-memory` run limit (spec #446).
//!
//! [`Counting`] is the system allocator with a count beside it. Until
//! [`count_this_thread`] is first called, by any thread, it only adds
//! one load of a flag to each allocation. From then on each allocation,
//! growth and free adds its bytes to, or takes them from, the count of
//! the thread that makes it; [`counted`] reads the calling thread's.
//! `oska-run` installs it as the global allocator.
//!
//! Its `unsafe` is the [`GlobalAlloc`] trait's own: each method passes
//! its arguments on to [`System`] unchanged, under the same contract.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// The system allocator, counting each thread's bytes once any thread
/// counts ([`count_this_thread`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct Counting;

/// Whether any thread has started counting: until then no allocation
/// touches a count.
static COUNTING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// This thread's bytes allocated, net of frees, since it started
    /// counting. Freeing what it allocated before then can take it below
    /// zero. Constant-initialized with no destructor, so reading it never
    /// allocates and works while the thread exits.
    static BYTES: Cell<i64> = const { Cell::new(0) };
}

/// Start counting this thread's heap bytes from zero.
pub fn count_this_thread() {
    COUNTING.store(true, Ordering::Relaxed);
    BYTES.with(|bytes| bytes.set(0));
}

/// The heap bytes this thread has allocated, net of frees, since it
/// last called [`count_this_thread`]: 0 if it never did.
pub fn counted() -> i64 {
    BYTES.try_with(Cell::get).unwrap_or(0)
}

/// Add `delta` bytes to this thread's count, if any thread counts.
#[inline]
fn count(delta: i64) {
    if COUNTING.load(Ordering::Relaxed) {
        let _ = BYTES.try_with(|bytes| bytes.set(bytes.get().saturating_add(delta)));
    }
}

/// `size` bytes as a count's change.
#[inline]
fn bytes(size: usize) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

// SAFETY: every method hands its arguments to `System` unchanged and
// returns what it returns, so `Counting` keeps `System`'s contract; the
// count beside it never allocates (a constant-initialized thread-local
// without a destructor) and never unwinds.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc` is `System.alloc`'s.
        let at = unsafe { System.alloc(layout) };
        if !at.is_null() {
            count(bytes(layout.size()));
        }
        at
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc_zeroed` is
        // `System.alloc_zeroed`'s.
        let at = unsafe { System.alloc_zeroed(layout) };
        if !at.is_null() {
            count(bytes(layout.size()));
        }
        at
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which is `System`, with
        // `layout`, as the caller's contract says.
        unsafe { System.dealloc(ptr, layout) };
        count(-bytes(layout.size()));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from this allocator, which is `System`, with
        // `layout`, and `new_size` meets `realloc`'s contract, as the
        // caller's contract says.
        let at = unsafe { System.realloc(ptr, layout, new_size) };
        if !at.is_null() {
            count(bytes(new_size).saturating_sub(bytes(layout.size())));
        }
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_counts_what_it_holds_net_of_frees() {
        std::thread::spawn(|| {
            count_this_thread();
            let layout = Layout::from_size_align(4096, 8).expect("a layout");
            // SAFETY: the layout's size is not zero.
            let at = unsafe { Counting.alloc(layout) };
            assert!(!at.is_null());
            assert_eq!(counted(), 4096);
            // SAFETY: `at` came from `Counting` with `layout`; 8192 is a
            // valid size for its alignment.
            let grown = unsafe { Counting.realloc(at, layout, 8192) };
            assert!(!grown.is_null());
            assert_eq!(counted(), 8192);
            let grown_layout = Layout::from_size_align(8192, 8).expect("a layout");
            // SAFETY: `grown` came from `Counting` with `grown_layout`.
            unsafe { Counting.dealloc(grown, grown_layout) };
            assert_eq!(counted(), 0);
        })
        .join()
        .expect("the thread should finish");
    }
}
