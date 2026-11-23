// SPDX-License-Identifier: Apache-2.0
//! The musl build's allocator when threads come and go (ticket #523,
//! security review S1): `oskac test` runs each test block on a thread of
//! its own, so a program with many test blocks starts and ends many
//! threads. A thread that ends must give the blocks on its free lists
//! back, or each one strands them, and the heap grows with the number of
//! threads rather than with what the program holds.
//!
//! A file of its own, so that this test binary's process runs nothing
//! else: [`SizeClassAlloc::regions`] counts for the whole process.
//!
//! Linux only: the exit hook that gives an ending thread's lists back
//! exists only there, as only the musl build of `oskac`, a Linux build,
//! ships with this allocator (`heap.rs`, "A thread's exit gives its
//! lists back, on Linux").

#![cfg(target_os = "linux")]

use oska_sys::SizeClassAlloc;

#[global_allocator]
static HEAP: SizeClassAlloc = SizeClassAlloc::new();

/// What one short-lived thread does: makes 100 values in each size class
/// (16 bytes to 1 KiB), checks them, and frees them all, so it ends with
/// blocks of every class on its free lists.
fn work(id: usize) {
    let held: Vec<Vec<u8>> = (0..100)
        .flat_map(|copy| (1..=64).map(move |granules| (copy, granules * 16)))
        .map(|(copy, len)| vec![(id + copy + len) as u8; len])
        .collect();
    for (at, bytes) in held.iter().enumerate() {
        let (copy, len) = (at / 64, (at % 64 + 1) * 16);
        assert_eq!(bytes.len(), len);
        assert!(bytes.iter().all(|&byte| byte == (id + copy + len) as u8));
    }
}

#[test]
fn threads_that_end_one_after_another_reuse_each_others_blocks() {
    for id in 0..400 {
        std::thread::spawn(move || work(id))
            .join()
            .expect("a worker thread ran to the end");
    }
    // One thread's values take about 3.3 MiB, 13 regions of 256 KiB.
    // With every exiting thread's free lists stranded (about 100 KiB
    // each), 400 threads would take some 110 regions more.
    let regions = SizeClassAlloc::regions();
    assert!(
        regions <= 40,
        "400 threads that ended took {regions} regions of 256 KiB; \
         their free lists were not reused"
    );
}
