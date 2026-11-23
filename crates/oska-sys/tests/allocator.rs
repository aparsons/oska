// SPDX-License-Identifier: Apache-2.0
//! The musl build's allocator (spec #521, ticket #523): `oska-sys`'s
//! [`SizeClassAlloc`], which serves requests of up to 1 KiB from size
//! classes it keeps for the life of the process, and hands larger or
//! more strictly aligned ones to the system allocator.
//!
//! This test binary installs it as its own global allocator, so every
//! `Box`, `Vec`, `String` and `Rc` below, and everything the test harness
//! itself allocates, goes through it, as in the musl build of `oskac`.
//! The tests use only safe code: they check what a program sees, that
//! each value keeps its bytes and its alignment while others are made,
//! grown, shrunk and freed, on one thread or across threads.
//!
//! - **Sizes and alignments:** every size up to past the largest class,
//!   and alignments from 1 byte to a page.
//! - **Growth and shrinking:** a buffer grown a byte at a time through
//!   every class into the system allocator, and shrunk back.
//! - **Frees on another thread:** values made on one thread and dropped
//!   on another, both ways at once, by several threads.
//! - **A long run:** millions of steps of the churn an interpreter makes
//!   (short texts, growing lists, values dropped out of order).
//! - **A request it cannot serve** is refused, with no panic.
//!
//! The musl CI job runs this file with the musl build
//! (`cargo test -p oska-sys --target x86_64-unknown-linux-musl`); every
//! other job runs it with its own C library underneath.

use oska_sys::SizeClassAlloc;

#[global_allocator]
static HEAP: SizeClassAlloc = SizeClassAlloc::new();

/// A byte that depends on a value's identity and its position in it, so
/// a block handed out twice, or overlapping another, shows up as wrong
/// bytes.
fn pattern(id: usize, at: usize) -> u8 {
    (id.wrapping_mul(31) ^ at.wrapping_mul(7)) as u8
}

/// A buffer of exactly `len` bytes, with capacity `len`, filled with
/// [`pattern`] for `id`.
fn filled(id: usize, len: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(len);
    bytes.extend((0..len).map(|at| pattern(id, at)));
    bytes
}

/// Panics unless `bytes` still holds [`pattern`] for `id`.
fn check(id: usize, bytes: &[u8]) {
    for (at, byte) in bytes.iter().enumerate() {
        assert_eq!(*byte, pattern(id, at), "value {id} changed at byte {at}");
    }
}

#[test]
fn every_size_up_to_past_the_largest_class_keeps_its_bytes() {
    let held: Vec<Vec<u8>> = (1..=2100).map(|len| filled(len, len)).collect();
    for (id, bytes) in held.iter().enumerate() {
        let len = id + 1;
        assert_eq!(bytes.len(), len);
        assert_eq!(
            bytes.as_ptr() as usize % 16,
            0,
            "{len} bytes are 16-aligned"
        );
        check(len, bytes);
    }
    assert!(
        SizeClassAlloc::regions() > 0,
        "the small values came from the size classes"
    );
}

/// A wrapper `$name<N>` of `N` bytes whose type asks for an alignment of
/// `$align` bytes.
macro_rules! aligned {
    ($name:ident, $align:literal) => {
        #[repr(align($align))]
        struct $name<const N: usize>([u8; N]);
    };
}
aligned!(Align1, 1);
aligned!(Align2, 2);
aligned!(Align8, 8);
aligned!(Align16, 16);
aligned!(Align32, 32);
aligned!(Align64, 64);
aligned!(Align256, 256);
aligned!(Align4096, 4096);

/// Makes 200 boxed values of each of several sizes with alignment
/// `$align`, then checks each one's address and bytes.
macro_rules! check_alignment {
    ($wrapper:ident, $align:literal, $($size:literal),+) => {{
        $(
            let held: Vec<Box<$wrapper<$size>>> = (0..200)
                .map(|id| {
                    let mut bytes = [0u8; $size];
                    for (at, byte) in bytes.iter_mut().enumerate() {
                        *byte = pattern(id, at);
                    }
                    Box::new($wrapper(bytes))
                })
                .collect();
            for (id, value) in held.iter().enumerate() {
                let address = &**value as *const $wrapper<$size> as usize;
                let align = std::mem::align_of::<$wrapper<$size>>();
                assert_eq!(align, $align);
                assert_eq!(address % align, 0, "{} bytes aligned to {}", $size, align);
                check(id, &value.0);
            }
        )+
    }};
}

#[test]
fn every_alignment_from_a_byte_to_a_page_is_honoured() {
    check_alignment!(Align1, 1, 1, 3, 17, 1000, 1025);
    check_alignment!(Align2, 2, 2, 30, 1024);
    check_alignment!(Align8, 8, 8, 24, 520, 2048);
    check_alignment!(Align16, 16, 16, 48, 1024, 1040);
    check_alignment!(Align32, 32, 32, 96, 1024);
    check_alignment!(Align64, 64, 64, 640);
    check_alignment!(Align256, 256, 256, 1024);
    check_alignment!(Align4096, 4096, 4096);
}

#[test]
fn a_buffer_grown_a_byte_at_a_time_and_shrunk_back_keeps_its_bytes() {
    for id in 0..4 {
        let mut bytes = Vec::new();
        for at in 0..5000 {
            bytes.push(pattern(id, at));
            if at % 97 == 0 {
                check(id, &bytes);
            }
        }
        check(id, &bytes);
        // Shrink through every class, from the system allocator's range
        // down to the smallest, checking what is left each time.
        let mut len = bytes.len();
        while len > 1 {
            len = len * 7 / 8;
            bytes.truncate(len);
            bytes.shrink_to_fit();
            assert_eq!(bytes.capacity(), len);
            check(id, &bytes);
        }
        // And grow again, by whole reallocations.
        for target in [2, 17, 100, 1000, 1024, 1025, 3000] {
            let from = bytes.len();
            bytes.reserve_exact(target - from);
            bytes.extend((from..target).map(|at| pattern(id, at)));
            check(id, &bytes);
        }
    }
}

#[test]
fn values_made_on_one_thread_and_dropped_on_another_stay_intact() {
    use std::sync::mpsc;
    use std::thread;

    // Four pairs of threads. In each, both threads make values, send
    // them to the other, and check and drop what they receive, so each
    // thread frees blocks it did not allocate, all the time.
    let pairs: Vec<_> = (0..4)
        .map(|pair| {
            let (to_b, from_a) = mpsc::channel::<(usize, Vec<u8>)>();
            let (to_a, from_b) = mpsc::channel::<(usize, Vec<u8>)>();
            let side = move |send: mpsc::Sender<(usize, Vec<u8>)>,
                             receive: mpsc::Receiver<(usize, Vec<u8>)>,
                             first: usize| {
                move || {
                    for round in 0..20_000 {
                        let id = first + round;
                        let len = 1 + (id * 37) % 1500;
                        send.send((id, filled(id, len)))
                            .expect("the other side runs");
                        let (id, bytes) = receive.recv().expect("the other side sends");
                        check(id, &bytes);
                    }
                }
            };
            let a = thread::spawn(side(to_b, from_b, pair * 1_000_000));
            let b = thread::spawn(side(to_a, from_a, pair * 1_000_000 + 500_000));
            (a, b)
        })
        .collect();
    for (a, b) in pairs {
        a.join().expect("thread a passes");
        b.join().expect("thread b passes");
    }

    // One thread makes, another only drops: the blocks it frees go back
    // to the shared pool and are reused, so memory does not grow with
    // each round.
    let (send, receive) = mpsc::sync_channel::<(usize, Box<[u8]>)>(64);
    let dropper = thread::spawn(move || {
        for (id, bytes) in receive {
            check(id, &bytes);
        }
    });
    let before = SizeClassAlloc::regions();
    for id in 0..400_000 {
        let len = 1 + id % 64;
        send.send((id, filled(id, len).into_boxed_slice()))
            .expect("the dropper runs");
    }
    drop(send);
    dropper.join().expect("the dropper passes");
    let taken = SizeClassAlloc::regions() - before;
    // 400,000 values of up to 64 bytes would need about 60 regions if
    // nothing were reused; with reuse a few do, whatever other tests in
    // this binary take at the same time.
    assert!(
        taken < 30,
        "{taken} regions taken for blocks freed on another thread"
    );
}

/// [`pattern`] as an ASCII character, for a text.
fn ascii(id: usize, at: usize) -> u8 {
    pattern(id, at) & 0x7f
}

/// Panics unless `text` still holds [`ascii`] for `id`.
fn check_text(id: usize, text: &str) {
    for (at, byte) in text.bytes().enumerate() {
        assert_eq!(byte, ascii(id, at), "text {id} changed at byte {at}");
    }
}

/// A small generator of pseudo-random numbers (xorshift), so a run is the
/// same each time.
struct Steps(u64);

impl Steps {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }
}

#[test]
fn a_long_run_of_interpreter_like_churn_keeps_every_value_intact() {
    use std::rc::Rc;

    // What the interpreter does most: make short texts and drop them,
    // grow lists a value at a time, share values and drop them out of
    // order. Each value is checked when it is replaced.
    let mut steps = Steps(0x9e37_79b9_7f4a_7c15);
    let mut texts: Vec<Option<(usize, Rc<str>)>> = vec![None; 4096];
    let mut lists: Vec<(usize, Vec<u8>)> = (0..64).map(|id| (id, Vec::new())).collect();
    for id in 0..2_000_000 {
        match steps.next(4) {
            0 | 1 => {
                let slot = steps.next(texts.len());
                if let Some((old, text)) = &texts[slot] {
                    check_text(*old, text);
                }
                let longest = if steps.next(16) == 0 { 2000 } else { 40 };
                let len = steps.next(longest);
                let text: String = (0..len).map(|at| char::from(ascii(id, at))).collect();
                texts[slot] = Some((id, Rc::from(text)));
            }
            2 => {
                let (list_id, list) = &mut lists[steps.next(64)];
                if list.len() > 3000 {
                    check(*list_id, list);
                    *list = Vec::new();
                    *list_id = id;
                }
                let at = list.len();
                list.push(pattern(*list_id, at));
            }
            _ => {
                let from = steps.next(texts.len());
                let to = steps.next(texts.len());
                texts[to] = texts[from].clone();
            }
        }
    }
    for (id, text) in texts.iter().flatten() {
        check_text(*id, text);
    }
    for (id, list) in &lists {
        check(*id, list);
    }
}

#[test]
fn a_request_the_system_cannot_serve_is_refused_without_a_panic() {
    // Too large for any machine: the system allocator returns null, and
    // the allocator passes that on as a refusal.
    let mut bytes: Vec<u8> = Vec::new();
    assert!(bytes.try_reserve_exact(1 << 62).is_err());
    // A small request still works afterwards.
    bytes.extend_from_slice(b"still here");
    assert_eq!(bytes, b"still here");
}
