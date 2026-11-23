// SPDX-License-Identifier: Apache-2.0
//! The heap `oskac` allocates from (spec #521, ticket #523).
//!
//! The musl build uses `oska-sys`'s [`oska_sys::SizeClassAlloc`] as its
//! global allocator: musl's own allocator locks on every call once the
//! interpreter's thread exists, and maps and unmaps memory over and over
//! (`docs/research/2026-11-musl-oskac-performance.md`). Every other
//! build, macOS, Windows and the glibc build, keeps the system allocator.
//! The allocator's `unsafe` code is in `oska-sys` (ADR 0009); naming it
//! here needs none.

#[cfg(all(target_os = "linux", target_env = "musl"))]
#[global_allocator]
static HEAP: oska_sys::SizeClassAlloc = oska_sys::SizeClassAlloc::new();

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_musl_build_takes_its_small_blocks_from_the_size_classes() {
        let held: Vec<Box<[u8; 64]>> = (0..20_000).map(|_| Box::new([7; 64])).collect();
        let musl = cfg!(all(target_os = "linux", target_env = "musl"));
        assert_eq!(
            oska_sys::SizeClassAlloc::regions() > 0,
            musl,
            "the size classes served {} build",
            if musl { "the musl" } else { "a non-musl" }
        );
        assert!(held.iter().all(|value| value.iter().all(|&byte| byte == 7)));
    }
}
