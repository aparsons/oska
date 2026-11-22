# How should Oska replace `memcpy` in the musl build, and what else would raise native performance?

- **Date:** 2026-11-22
- **Scope:** Adam decided on 2026-11-22 to keep both Linux builds and land the speedups ([ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). The musl build is slower than the glibc build ([2026-11-musl-oskac-performance.md](2026-11-musl-oskac-performance.md)). The allocator (PR #570) and short copies (#572) are done or in review. Replacing `memcpy` was discussed and not chosen for #570. This note asks: what is the best way to replace it, what does it buy, and what else would help?
- **Status:** x86_64 measured here. **aarch64 is not measured**: this machine is x86_64 only. Every aarch64 statement is read from source or marked **Inference**.
- **Important caveat:** the tree measured here is the checkout in `/home/user/oska`, which has **no allocator (`heap.rs`) and none of the #500/#501/#572 copy fixes**. It still evaluates a string literal into a fresh `Rc<str>` each time (`ExprKind::Str` calls `memory::text`) and makes one text per character in `String.chars`. So every gain below is measured against musl's mallocng and the old copy counts. After #570 the `memcpy` gain is **smaller** (section 3). Re-measure on top of #570 before building anything.
- **Method:** three scratch builds of `oskac` outside the repo (the repo was not changed): the checkout as is, one with `memcpy` and `memmove` replaced in `oska-sys`, and a counting build that records the size of each `mem*` call. A microbenchmark of copy routines for 1 B to 1 MiB. `scripts/bench-native.sh` and a CPU-time timer on `site/build.osk`. Code review of musl, glibc, `compiler-builtins` and Arm's Optimized Routines sources.
- **Machine:** Intel Xeon @ 2.10 GHz, 4 vCPUs in a KVM VM, with `erms`, `fsrm`, `avx2` and `avx512f` (the AMX and AVX512-FP16 flags suggest a Sapphire Rapids-class core, **Inference**). L1d 192 KiB, L2 8 MiB, L3 260 MiB across 4 instances, as `lscpu` reports them. Linux 6.18. Shared with other agents, so noisy: the same binary varied by about 10% between runs, and an empty indirect call cost 1.0 to 2.1 ns. Every figure below is the best of several runs, and a "gap" is a difference of two noisy numbers. `perf` is not installed here, so the share of time in `mem*` is found by replacing the routines and timing, not by sampling.
- **Toolchain:** Rust 1.100.0, whose `x86_64-unknown-linux-musl` `libc.a` is musl 1.2.5 (the string `1.2.5` is in it). musl's `master` (the kraj mirror, fetched today) says `VERSION` 1.2.6, and its `memcpy.s` is the same algorithm.

## Short answer

1. **Replace `memcpy` and `memmove` only, on x86_64 musl only, with our own Rust in `oska-sys`.** A prototype of about 170 lines does it. Small copies use overlapping loads and stores, medium ones 16-byte SSE2 moves, and copies of 2 KiB or more `rep movsb`. It needs no AVX2, no ifunc and (with a fallback) one `cpuid` bit. Replacing `memcmp`, `memset` and `memchr` too gave **no measurable gain**.
2. **Measured on the pre-#570 tree:** the musl interpreter ran the site build in 11.1 s against glibc's 7.3 s (1.53x). Replacing `memcpy` and `memmove` brought it to 9.8 s (1.35x), about a third of the gap. `oskac build site/build.osk` went from 108 ms to 79 to 84 ms, against glibc's 64 ms: more than half of the gap. A 1-byte copy cost 25.7 ns in musl and 1.9 ns in the replacement.
3. **The size of that win depends on #570.** In the interpreter, 85% of `memcpy` calls copy one byte (41 of 48 million). Avoiding those copies (#500, #501, #572) gives the same saving on both builds. Re-measure first. **Inference:** if they are gone, the remaining `memcpy` share of the musl interpreter is a few percent, and only the compile (2 million calls, half of them 3 to 4 bytes) is left.
4. **A bigger and cheaper lever is not `memcpy` at all:** `[profile.release]` has no settings. `lto = "fat"` with `codegen-units = 1` cut the glibc interpreter's site build by 22% (7.29 s to 5.69 s) and the patched musl one by 15%, shrank the binary by 23%, and took no new code. Release build time doubles (24 s to 50 s).
5. **Skip:** vendoring glibc routines (LGPL), a custom musl, AVX2 (a 10 to 30% win in one size band), `memcmp`, `memset` and `memchr` replacements, `-C target-cpu` (no gain here, and the repo refuses `RUSTFLAGS`), PGO now (mixed result), and a different allocator crate.

## 1. What the musl build runs today

Facts read from source, checked against the binary where noted.

- **x86_64 `memcpy`** ([musl `src/string/x86_64/memcpy.s`](https://github.com/kraj/musl/blob/master/src/string/x86_64/memcpy.s)): for 8 bytes or more it copies single bytes until the destination is 8-byte aligned, then `rep movsq`, then single bytes for the last 0 to 7. It makes no use of vectors. A `rep` instruction has a start-up cost even for a count of zero, which is why a 1-byte copy costs 25 ns (section 2).
- **x86_64 `memmove`** ([`memmove.s`](https://github.com/kraj/musl/blob/master/src/string/x86_64/memmove.s)): jumps to `__memcpy_fwd`, a label inside `memcpy.s`, when the ranges do not overlap forwards, else copies backwards with `std; rep movsb; cld`. This matters for linking (section 4).
- **`memset`** ([`x86_64/memset.s`](https://github.com/kraj/musl/blob/master/src/string/x86_64/memset.s)) is already decent: overlapping stores up to 126 bytes, then `rep stosq`.
- **`memcmp`** ([`memcmp.c`](https://github.com/kraj/musl/blob/master/src/string/memcmp.c)) is a byte loop. **`memchr`** ([`memchr.c`](https://github.com/kraj/musl/blob/master/src/string/memchr.c)) reads a word at a time. Both are portable C.
- **aarch64:** musl's `memcpy` and `memset` are Arm's. [musl's COPYRIGHT](https://github.com/kraj/musl/blob/master/COPYRIGHT) says "The AArch64 memcpy and memset code (src/string/aarch64/*) are Copyright 1999-2019, Arm Limited", and [`aarch64/memcpy.S`](https://github.com/kraj/musl/blob/master/src/string/aarch64/memcpy.S) is the Optimized Routines code under the MIT licence. It copies up to 32 bytes with `ldp`/`stp` pairs and uses a 64-byte pipelined loop for large copies. Its `memmove` is the portable C one, which calls `memcpy` unless the ranges overlap and otherwise copies by byte and word loops ([`memmove.c`](https://github.com/kraj/musl/blob/master/src/string/memmove.c)). **Inference:** aarch64 has no cause-3 problem like x86_64's, as the earlier note guessed from the disassembly. The remaining aarch64 cost is the allocator and page faults.
- **glibc** picks a routine at load time with an ifunc ([`ifunc-memmove.h`](https://github.com/bminor/glibc/blob/master/sysdeps/x86_64/multiarch/ifunc-memmove.h) tests `AVX512F`, `ERMS`, `FSRM` and others). Its `rep movsb` threshold starts at 2048 bytes ([`cacheinfo.h`](https://github.com/bminor/glibc/blob/master/sysdeps/x86/cacheinfo.h): `__x86_rep_movsb_threshold = 2048`) and is tuned at start-up. glibc's own files are LGPL 2.1 or later (the header of [`memmove-vec-unaligned-erms.S`](https://github.com/bminor/glibc/blob/master/sysdeps/x86_64/multiarch/memmove-vec-unaligned-erms.S)).
- **musl has no ifunc.** The dynamic loader source [`ldso/dynlink.c`](https://github.com/kraj/musl/blob/master/ldso/dynlink.c) never mentions `ifunc` or `IRELATIVE` (0 matches; checked with `grep -ci`). So any CPU-specific choice is ours to make, with `cpuid` and a cached flag.
- **Rust's `compiler-builtins`** has `memcpy` code with `rep movsb`, but its `mem` feature, which turns on the `no_mangle` symbols, is for targets without a C library ([its `Cargo.toml`](https://github.com/rust-lang/compiler-builtins/blob/master/compiler-builtins/Cargo.toml): "Enable `no_mangle` symbols for memory-related intrinsics like memcpy"). Its intrinsics are marked `linkage = "weak"` ([`macros.rs`](https://github.com/rust-lang/compiler-builtins/blob/master/compiler-builtins/src/macros.rs)). I checked the `libcompiler_builtins` rlib in the 1.100.0 musl target: `nm` shows no `memcpy`, `memmove`, `memset`, `memcmp` or `bcmp` in it. So on musl nothing in Rust's runtime competes with an `oska-sys` definition. The x86_64 file's header comment quotes the Intel Optimization Manual: ERMSB is Ivy Bridge and later, FSRM is Ice Lake and later, and ERMSB does not speed up the backwards `rep movsb` ([`mem/x86_64.rs`](https://github.com/rust-lang/compiler-builtins/blob/master/compiler-builtins/src/mem/x86_64.rs)). I could not reach Intel's manual itself (section "Sources").

## 2. What was measured

### 2.1 How big are the calls?

A counting build records a log2 size bucket for each call to `memcpy`, `memmove`, `memset`, `memcmp` and `bcmp`, and `memchr`, then prints the table at exit. The program is the site generator, on the pre-#570 tree.

| Size (bytes) | Interpreter `memcpy` | Interpreter `memcmp`/`bcmp` | Compile `memcpy` | Compile `memcmp`/`bcmp` |
| --- | ---: | ---: | ---: | ---: |
| 0 | 833,299 | 129,880 | 448 | 1,172 |
| 1 | 40,990,985 | 75,284,791 | 186,346 | 2,972,296 |
| 2 | 246,823 | 47,237 | 25,989 | 12,907 |
| 3 to 4 | 1,535,121 | 56,517,077 | 1,030,523 | 109,832 |
| 5 to 8 | 1,246,955 | 65,363,502 | 489,261 | 105,640 |
| 9 to 16 | 548,121 | 71,415,083 | 167,152 | 98,058 |
| 17 to 32 | 366,284 | 2,285,374 | 36,087 | 4,570 |
| 33 to 128 | 797,854 | 4,067 | 19,323 | 38,313 |
| 129 to 2048 | 1,571,426 | 0 | 110,398 | 28,774 |
| over 2048 | 194,265 | 0 | 8,488 | 0 |
| **Total** | **48,331,133** | **271,047,011** | **2,074,015** | **3,371,562** |

- 94.7% of interpreter `memcpy` calls and 93.3% of the compile's copy at most 32 bytes. Only 0.4% are 2 KiB or more. So the **fixed cost per call is what matters**, not bandwidth.
- `memmove` is rare (420 calls in the interpreter, 42,389 in the compile), `memset` is rarer (123 and 28,841) and `memchr` almost never (14 and 1; Rust's own search code does not use it).
- `memcmp` is called 271 million times, all of at most 32 bytes. musl's byte loop is cheap for names that differ early. Replacing it made no measurable difference (section 2.3).

### 2.2 Cost of one call, by size

A small program (not in the repo) times each routine on warm, 64-byte-aligned buffers, through an indirect call, best of 7 repeats, and takes the lower of two full runs. "Call" in the header means every figure includes about 1.0 to 1.5 ns for the call itself (an empty routine measured 1.0 ns at best). Columns: musl's `memcpy` (static musl build), glibc's (dynamic glibc build, so ifunc picks it), a bare `rep movsb`, and two candidates, "SSE2+rep" and "AVX2+rep". Both candidates copy up to 16 bytes with overlapping scalar moves, up to 128 bytes (256 for AVX2) with overlapping vector moves, from 2048 bytes with `rep movsb`, and in between with a 64-byte (128-byte) loop.

| Size | musl | glibc | `rep movsb` | SSE2+rep | AVX2+rep |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 B | 25.7 ns | 2.5 | 3.4 | 1.9 | 2.5 |
| 8 B | 13.0 | 2.4 | 3.0 | 1.3 | 1.5 |
| 32 B | 11.2 | 2.4 | 2.8 | 1.6 | 2.0 |
| 128 B | 10.4 | 2.1 | 2.6 | 1.9 | 3.0 |
| 256 B | 10.9 | 3.5 | 7.7 | 3.6 | 4.0 |
| 1 KiB | 13.9 | 6.8 | 11.8 | 10.7 | 6.5 |
| 2 KiB | 16.7 | 15.8 | 14.3 | 15.6 | 15.2 |
| 4 KiB | 19.2 | 27.1 | 18.6 | 19.2 | 19.1 |
| 16 KiB | 69.7 | 82.6 | 67.2 | 67.1 | 73.6 |
| 64 KiB | 1,260 | 1,495 | 1,323 | 1,189 | 1,479 |
| 1 MiB | 40,586 | 40,060 | 37,605 | 36,090 | 36,779 |

Reading it:

- **musl costs 10 to 26 ns for any copy up to 1 KiB**, where glibc and the candidates cost 1.3 to 4 ns. That is the whole cause-3 gap of the earlier note, and it matches what the size table says matters.
- **From 4 KiB up the routines are equal** within noise, and above 64 KiB all of them are limited by the memory system, not the instructions.
- **Misaligned source is where musl's `rep movsq` hurts.** With the source one byte off alignment, musl takes 280 ns at 4 KiB and 875 ns at 16 KiB, against 39 to 41 ns and 127 to 147 ns for glibc and the SSE2+rep candidate. Slices of text start at any offset, so large copies of text often meet this.
- **A bare `rep movsb` is the wrong tool below 2 KiB**: 2.6 to 3.4 ns for 1 to 128 bytes even with FSRM, and 7.7 to 11.8 ns for 256 B to 1 KiB. On a CPU without FSRM the short case would cost more (not measured; no such CPU here). That is why a hybrid is needed, and why `compiler-builtins`' "use `rep movsb` for everything when `ermsb` is on" is not enough for a program that makes 95% short copies.
- **AVX2 buys 10 to 40% between 256 B and 1.5 KiB** and nothing elsewhere, and costs a function-multiversioning and detection step. It is not worth it for a 0.4% band.

### 2.3 End to end

All figures are CPU time (user plus system) of `oskac` built for `x86_64-unknown-linux-musl` unless "glibc" is named, run from a copy of the repo root. "Patched" is the checkout with `memcpy` and `memmove` replaced in `oska-sys` by the SSE2+rep candidate above. Best of 3 for the interpreter (each run takes about 10 s), best of 15 for the compile.

| Workload | glibc | musl as shipped | musl patched | Patched closes |
| --- | ---: | ---: | ---: | ---: |
| `run --interpret site/build.osk`, first set | 7,686 ms | 10,566 ms | 9,126 ms | 50% of the gap |
| `run --interpret site/build.osk`, second set | 7,294 ms | 11,141 ms | 9,837 ms | 34% |
| `build site/build.osk` | 64 ms | 108 ms | 84 ms | 55% |
| `build site/build.osk` (`memcpy` and `memmove` only, built again) | 64 ms | 108 ms | 79 ms | 66% |
| bench `text`, interpreter | 133 ms | 218 ms | 149 ms | 82% |
| bench `bulk-text`, interpreter | 30 ms | 50 ms | 31 ms | 95% |
| bench `recursion`, interpreter | 438 ms | 1,195 ms | 1,224 ms | 0% |
| bench `loop`, interpreter | 694 ms | 549 ms | 681 ms | none (noise) |

(Best of two `bench-native.sh` invocations of 3 runs each, per binary; the two site rows are two separate sessions an hour apart, which shows the noise. `loop` allocates nothing and does not call `mem*`.)

What this says:

- **`memcpy` and `memmove` cost the musl interpreter at least 11 to 14% of its CPU time** (1.2 to 1.6 s of 11.1 s), and the compile at least 22 to 27% (24 to 29 ms of 108 ms). Those are lower bounds: the replacement is not free.
- The histogram and the timing table agree. 41 million 1-byte copies at about 24 ns saved each is 1.0 s, and the other 7 million copies add 0.1 to 0.2 s. The measured saving is 1.2 to 1.6 s. This check is **estimated**, not measured separately.
- **`recursion` does not move at all.** Its gap (2.7x) is the allocator and its lock, not `memcpy`. So `memcpy` is one of two big causes, as the earlier note said (28% of the interpreter gap by sampling there; 34 to 50% by replacement here).
- **Replacing `memcmp`, `bcmp`, `memset` and `memchr` as well made no difference.** A build with only `memcpy` and `memmove` replaced ran the site build in 10.1 to 10.35 s against 9.98 to 10.4 s for all six. Keep musl's.
- The check that the routines are right: a differential test (about 300 sizes up to 10 KB, random source and destination offsets, overlap in both directions, checks for overrun on both sides) passed against naive loops for `memcpy`, `memmove`, `memset`, `memcmp` and `bcmp`. A test like it should be part of the ticket.
- **Binary size:** 6,419,776 bytes as shipped, 6,420,680 patched (+0.9 KB).

## 3. The options

Estimates are labelled. Agent-hours means one agent working, including a full `scripts/check.sh` and review.

### (a) Link a patched or newer musl

- **What:** build musl from source in CI and link it in place of the `libc.a` in the Rust toolchain, or get a faster `memcpy.s` into musl.
- **Impact:** same as (b) for `memcpy`, plus whatever else we patch. **Newer musl alone does nothing**: `master` is 1.2.6 and has the same `memcpy.s` as the 1.2.5 in Rust's `libc.a` (compared by reading; the earlier note reached the same result from the release notes).
- **Effort and time (Inference):** 2 to 4 tickets, 10 to 20 agent-hours. We would own a libc build: the cross toolchain, Rust's CVE patches to musl (the earlier note found Rust 1.100.0 carries `musl-cve-2026-*.diff` patches), a new supply-chain link, new release checks. Rust's std for the musl targets is built against the musl its CI builds, so a mismatch is possible.
- **Risks:** high. Every release needs the libc rebuilt and re-verified. Upstream acceptance of an x86-specific `rep movsb` or SSE2 `memcpy` is not something I could check (musl's site was unreachable).
- **Outcomes:** best, the same speed as (b) at five times the cost. Likely, a stalled change. Worst, a custom libc that nobody updates.
- **Verdict: no.**

### (b) Our own `memcpy` and `memmove` in `oska-sys` (recommended, in a minimal form)

- **What:** two `#[no_mangle] pub unsafe extern "C"` functions, on `cfg(all(target_os = "linux", target_env = "musl", target_arch = "x86_64"))` only. Small copies use overlapping moves, medium ones SSE2, and 2 KiB or more `rep movsb`. The prototype has no `cpuid` and uses `rep movsb` from 2048 bytes on every CPU.
- **Impact (measured, pre-#570):** section 2.3. Interpreter site build 11.1 s to 9.8 s. Compile 108 ms to 79 to 84 ms. Roughly a third of the interpreter gap and over half of the compile gap. **Estimated after #570:** if the 1-byte copies are gone, the interpreter saving falls to the cost of the other 7 million calls, 0.1 to 0.3 s (1 to 3%). The compile saving falls only if #572 removed its 3 to 4 byte copies.
- **Effort and time (estimate):** one ticket, 4 to 8 agent-hours. The prototype took about an hour. The rest is: wrapping each intrinsic in its own `unsafe` block with a `// SAFETY:` comment, because `oska-sys` denies `multiple_unsafe_ops_per_block` and `undocumented_unsafe_blocks` ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)); the `cpuid` fallback below; the differential test; the ADR 0009 amendment (the allocator spec amends the same ADR); a check that the functions contain no call to `memcpy` (see below).
- **Required fallback for old CPUs:** `rep movsb` is fast on ERMSB parts (Ivy Bridge and later) and was slow before that. The way to be safe is one `cpuid` read (leaf 7, EBX bit 9 is ERMS) cached in an atomic, with a plain loop or `rep movsq` as the fallback. No ifunc is needed. **I could not test a CPU without ERMS**, so the fallback's speed is unknown. The plain SSE2 loop was 2 to 3x slower than `rep movsb` at 4 to 16 KiB here (59.6 ns against 22.9 ns at 4 KiB, in the scratch run without the `rep` branch), so it is a poor fallback. `rep movsq`, which musl uses today, is a better one for aligned copies (musl matched `rep movsb` at 4 to 16 KiB), but it was 7x slower with a misaligned source (section 2.2). The right fallback is a choice to make with a non-ERMS CPU in hand.
- **Risks:**
  - **A heap bug-like blast radius.** A wrong `memcpy` corrupts anything, capability checks included. Mitigation: the differential test, the existing musl CI job (it runs the `oska-sys` and `oska-run` tests with the musl build), and a human merge (per [merge-policy.md](../agents/merge-policy.md), a PR that adds `unsafe` is a human merge).
  - **LLVM turning the copy loop into a call to `memcpy`**, so that `memcpy` calls itself. I looked for it in the disassembly of the shipped function, with and without LTO: no `call` in `memcpy`, `memmove`, `memset`, `memcmp` or `bcmp`. A future compiler or flag change could break that. A recursion crashes the first run, so any test catches it, but the ticket should still assert it.
  - **Do not use `#![no_builtins]` in `oska-sys`.** It is the usual fix for that recursion, and it is how `compiler-builtins` works. But I built the crate with it and fat LTO, for glibc and musl, and the link failed with undefined `__rust_alloc` and other `alloc` symbols (the no-builtins crate is left out of LTO, so the `alloc` code it uses is never emitted). If a compiler change forces `no_builtins`, the functions need their own tiny `#![no_std]` crate with no `alloc`, as `compiler-builtins` has, and that crate would be a second `unsafe` crate (an ADR 0009 change).
  - **Lint:** rustc warns `suspicious_runtime_symbol_definitions` on a `no_mangle` `memcpy` (five warnings in my build); `compiler-builtins` allows it. Allow it on these functions only.
  - **Edition:** the workspace is edition 2021, so `#[no_mangle]` works as written. The Reference's examples write the attribute as [`#[unsafe(no_mangle)]`](https://github.com/rust-lang/reference/blob/master/src/abi.md), the form newer editions use, so a later edition move may touch them.
  - **Applies only to musl.** A strong `memcpy` in a dynamic glibc executable would also win over glibc's ifunc one. The `cfg` keeps the glibc build and macOS and Windows as they are.
  - **aarch64 and the other targets get nothing**, by design (section 1).
- **Outcomes:** best, the musl interpreter is 1.3x glibc's from this change alone, and with the allocator and LTO inside the 1.10x of the spec. Likely, a third of the interpreter gap before #570 and a few percent after, and a bigger share of the compile. Worst, the gain after #570 is under the noise floor and the `unsafe` is a cost without a return.

### (c) Vendor Arm Optimized Routines or glibc routines

- **Arm Optimized Routines** ([repository](https://github.com/ARM-software/optimized-routines)): `string/aarch64/` has `memcpy.S`, `memset.S`, `memcmp.S`, `memchr.S` and `memrchr.S` (each fetched; all 200). The licence is "MIT OR Apache-2.0 WITH LLVM-exception" ([LICENSE](https://github.com/ARM-software/optimized-routines/blob/master/LICENSE)), and the newer `memcpy.S` header says the same, so Oska (Apache-2.0, see `LICENSE`) can use the Apache-2.0 with LLVM-exception terms. musl's aarch64 `memcpy` is already an older copy of this file (header 2012-2020, MIT only). The newer one has `memmove` handling of overlap in the same entry, which musl's does not (the portable `memmove.c` is used). **Impact: unknown until #506** measures aarch64. **Inference:** small, since `memcpy` is already this code. Effort: 6 to 10 agent-hours (the `.S` uses macros from `asmdefs.h`, so it needs porting to `global_asm!`). Verdict: **wait for #506**.
- **glibc routines** are LGPL 2.1 or later. Copying them into an Apache-2.0 repo, or linking them statically, brings LGPL's terms (relinkability) with it. **No.**
- **compiler-builtins' x86_64 code** is [`MIT AND Apache-2.0 WITH LLVM-exception AND (MIT OR Apache-2.0)`](https://github.com/rust-lang/compiler-builtins/blob/master/compiler-builtins/Cargo.toml), so it could be copied. It is `rep movsb` for everything, which section 2.2 shows is the wrong shape for 95% short copies. Write our own.

### (d) A different libc or allocator

- **Different libc:** Rust's Linux targets with std are gnu and musl, as [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md) and the [earlier note](2026-11-oskac-without-glibc.md) found. Adam kept both. No third libc is on offer. **No.**
- **mimalloc** ([MIT](https://github.com/microsoft/mimalloc/blob/master/LICENSE)): it is C and a crate, and the workspace takes no external crates and has no C compiler step. It also does not provide `memcpy`. The allocator question is #570's, and it helps `recursion` (the other half of the gap), not this. **No.**
- **A bump or arena allocator** would remove the per-call allocator cost but changes memory behaviour under `--max-memory`. Out of scope here.

### (e) Make fewer copies in the interpreter (cheapest, do first)

- **What:** the fixes already ticketed: one `Rc<str>` for each string literal (#500), shared one-character texts (#501), short copies (#572), one `Vec` for each call's arguments (#505).
- **Impact (measured count, estimated saving):** 41 of 48 million interpreter `memcpy` calls are 1-byte copies (section 2.1). At musl's 25 ns each that is 1.0 s of the 11.1 s run, and at glibc's 2.5 ns about 0.1 s. So these fixes help **both** builds, and take most of the musl `memcpy` penalty away.
- **Effort and time:** already ticketed and in #570. **Risk:** low, safe Rust. **Limit:** they cannot help `String`-heavy compiles where the copy is a real 3 to 4 byte name, and they do nothing for `realloc`'s copies inside musl.
- **Outcomes:** best, `memcpy` no longer matters and (b) is not needed. Likely, (b)'s remaining value is the compile. Worst, #570 did not fix them all.
- **Verdict: first.** Then re-measure to decide on (b).

### (f) Other levers

Which side each lever helps: "oskac" is the interpreter and the compiler in the `oskac` binary. "Native output" is the executable that `oskac build` writes, which does not use a C library and so is not touched by anything in (a) to (d).

| Lever | Helps | Measured here | Effort (estimate) | Verdict |
| --- | --- | --- | --- | --- |
| `lto = "fat"` and `codegen-units = 1` in `[profile.release]` | oskac, both builds | glibc interpreter 7,294 to 5,693 ms (-22%); patched musl 9,837 to 8,349 ms (-15%); compile 64 to 59 ms and 84 to 73 ms; binary 6.42 to 4.96 MB (-23%). Release build 24 s to 50 s | 1 to 2 agent-hours | **Do.** Needs no `unsafe`. |
| `panic = "abort"` on top | oskac | Another -6% (8,349 to 7,840 ms best of 3) and a 4.29 MB binary (-33% in all) | 1 to 2 agent-hours | **Maybe.** Changes a panic's exit code and message. `oskac` does not depend on `oska-eval`, which is the one crate that calls `catch_unwind`, but `oska-run` re-raises an interpreter thread's panic with `resume_unwind`. Check the golden tests first. |
| `-C target-cpu=x86-64-v3` | oskac | none: 10,370 ms against 9,837 ms for the same build without (within noise) | none | **Skip.** It would also drop pre-Haswell CPUs, and `scripts/unsafe-guard/guard.sh` refuses `RUSTFLAGS` and a checked-in cargo config. |
| PGO (`-C profile-generate`, then `profile-use`) | oskac | trained on the site build and run: site interpreter 8,534 to 7,761 ms (-9%), but **held-out workloads moved both ways**: `loop` 522 to 711 ms (+36%), `recursion` 1,057 to 1,237 ms (+17%), `text` -15%, `lists` -10%. The compile got 11% slower | 8 to 16 agent-hours for a pipeline (training set, profile data handling, `llvm-tools` in the toolchain pin, release workflow) | **Skip for now.** A bad training set is worse than none. A human merge too (release path). |
| BOLT | oskac | not tried ([llvm-bolt needs a perf profile](https://github.com/llvm/llvm-project/blob/main/bolt/README.md)) | 10+ agent-hours | Skip. |
| Allocator (#570) | oskac, musl only | the other half of the gap: `recursion` is 1,195 ms on musl against 438 ms on glibc and `mem*` does not touch it | in review | Already chosen. |
| Cut allocations in the checker (`Infer::flow` clones, `Ty` names) | oskac, both builds | from the earlier note: up to 326,000 and 218,000 of 689,000 allocations | medium, #503 and #504 | Already ticketed. |
| Resolve intrinsics once | oskac, both builds | the earlier note: about 25% of the glibc interpreter's instructions | small, #502 | Already ticketed. |
| `copy_bytes` in `std.rt` | **native output** | not measured | 2 to 3 tickets (a new intrinsic on all five backends under the parity gate) | **Hold.** See below. |
| Native code generation (`optimize.rs`, `regalloc.rs`, inlining) | **native output** | the bench shows 26x to 83x over the interpreter on `loop`, `text`, `lists` and `recursion`, but only 1.3x to 1.9x on `bulk-text` | not estimated | Separate research. |

On native output: its text copy is the Oska function `copy_bytes` in `crates/oska-check/src/std/rt.osk`. It already copies 32 bytes a time with word loads and stores, and finishes with one overlapping word, the same shape as the prototype's small path. A `rep movsb` intrinsic would help only copies of 2 KiB or more, which are about 0.4% of calls in the interpreter's workload. `bulk-text` (the workload where native is only 1.3x to 1.9x faster) works on 520 KB texts and is where such an intrinsic might show, but **I did not measure native copy cost**, so I do not recommend it yet.

## 4. How the override works on a static musl link

What I tried, on Rust 1.100.0 for `x86_64-unknown-linux-musl`:

- **`#[no_mangle]` gives the symbol the item's own name** ([Rust Reference, `no_mangle`](https://github.com/rust-lang/reference/blob/master/src/abi.md)). A `#[no_mangle] pub unsafe extern "C" fn memcpy` in `oska-sys` is therefore the symbol `memcpy`.
- **A static archive supplies a member only for a symbol that is still undefined** when the linker reaches it. `oska-sys`'s object comes before `libc.a` on the link line, so it defines `memcpy` first, and musl's `memcpy.lo` is never pulled in.
- **Overriding only `memcpy` fails to link.** musl's `memmove.lo` needs `__memcpy_fwd`, which lives in `memcpy.lo`. The linker then pulls `memcpy.lo` in and reports `multiple definition of 'memcpy'`. I reproduced it with a small crate that defines only `memcpy`. **Define `memmove` too.** With both, the link is clean, `nm` shows our `memcpy` (0x166 bytes) and `memmove` (0x246) in the executable, and musl's `__memcpy_fwd` is gone.
- **musl's own callers get ours.** Inside `libc.a`, `realloc` calls `memcpy` by symbol, so it takes the replacement at link time. That is the 1.3 million `realloc` copies of the earlier note.
- **No conflict with `compiler-builtins`** on musl: its rlib has no `mem*` symbols (checked with `nm`), and where its symbols are on they are weak, so a strong definition would win.
- **A different link order** (a dependency that makes `libc.a` come first) could break this. The link is a test: the CI musl job would fail with a duplicate symbol, so it is caught, but the ticket should add a check that the executable's `memcpy` is ours, such as the differential test calling through the symbol.
- **`#[global_allocator]`** is a separate mechanism, the Rust runtime's ([Rust Reference, Runtime](https://github.com/rust-lang/reference/blob/master/src/runtime.md)): a `static` of a `GlobalAlloc` type in the final crate. It is how #570's allocator is chosen, and it does not interact with `memcpy`.

## 5. Ranked recommendation

1. **Finish and merge #570, then re-measure on the musl and glibc builds** (the counting build and `bench-native.sh`). This decides whether anything below is worth its `unsafe`. It is the cheapest step and it protects the rest from guesswork.
2. **Add `lto = "fat"` and `codegen-units = 1` to `[profile.release]`.** It is the largest measured win per hour, for both builds, with no new code. It does lift glibc too, so the spec's "musl at most 1.10x glibc" must say whether glibc is built the same way (it should be: the same profile).
3. **If `memcpy` is still 5% or more of the musl run after #570, land option (b) in its minimal form:** `memcpy` and `memmove` only, x86_64 musl only, SSE2 plus `rep movsb`, an ERMS check with a fallback chosen on a non-ERMS CPU, a differential test, an ADR 0009 amendment, a human merge. The ticket includes the prototype's measurements as its baseline.
4. **Run #506 (aarch64).** Do nothing for aarch64 `memcpy` until it shows a share. If it does, port Arm's current `memcpy.S` and `memmove` handling (option (c)).
5. **Consider `panic = "abort"`** after checking the exit-code tests.

**Skip:** a patched or custom musl (a); glibc routines (LGPL); AVX2 variants; replacing `memcmp`, `memset` or `memchr`; `-C target-cpu`; PGO and BOLT until the build has a stable, representative training set; mimalloc or another libc.

## 6. Proposed tickets, each sized for one agent

| # | Ticket | Size | Notes |
| --- | --- | --- | --- |
| 1 | **Re-measure the musl gap after #570.** Run `bench-native.sh` for both builds, and a counting build for `mem*` calls by size, on the post-#570 tree. Record it in `2026-11-musl-oskac-performance.md`. | 1 to 2 h, no code in the repo | Decides ticket 3. |
| 2 | **Release profile: `lto = "fat"`, `codegen-units = 1`.** Record build time, binary size and bench numbers for both builds. Check that the release workflow stays far below its `timeout-minutes` and that the check scripts still pass. | 1 to 2 h | Agent merge if no workflow or script changes. |
| 3 | **x86_64 musl `memcpy` and `memmove` in `oska-sys`.** SSE2 and `rep movsb` with the ERMS check, one `unsafe` op per block with `// SAFETY:`, `#[allow(suspicious_runtime_symbol_definitions)]`, a differential test in `crates/oska-sys/tests/` that also checks the executable's symbol, ADR 0009 amendment. No `#![no_builtins]`. | 4 to 8 h | Human merge (new `unsafe`). Only if ticket 1 says so. |
| 4 | **aarch64 measurement (#506).** Add the `mem*` size counts and the share of `memcpy` and `memmove` to it. | 1 to 2 h on an aarch64 runner | Decides whether to port Arm's routines. |
| 5 | **`panic = "abort"` for the release profile.** Check the golden and exit-code tests, change the documented exit code if any, record size and speed. | 1 to 2 h | Optional. |

## Open questions

- **For Adam:** may `oska-sys` also hold `memcpy` and `memmove` (an ADR 0009 amendment, with the allocator's)? A `no_mangle` libc symbol is a new kind of `unsafe` surface: it is called by code that has no `unsafe` of its own.
- **For Adam:** is the 1.10x threshold measured against a glibc build with the same profile (LTO on both)? With LTO, glibc gets faster too: 5.7 s against 8.3 s for patched musl is 1.47x, against 1.35x for the same two without LTO. The ratio gets worse while both builds get faster, so the threshold needs the profile named.
- **For Adam:** is `panic = "abort"` acceptable for the shipped `oskac` (exit status and message of an internal panic change)?
- **For Adam:** how old a CPU must the musl build run on? SSE2 is the x86_64 baseline and needs no detection. ERMS (2012 and later) is the one bit the fast path uses. I could not test a CPU without it.
- **Open, measurement:** the aarch64 numbers (#506). Whether musl's `memmove` (portable C) matters on aarch64.
- **Open, measurement:** the `memcpy` share after #570. This note's figures are from before it.
- **Open, measurement:** `perf` was not available, so the share of time in `mem*` comes from replacing them. A machine with `perf` and hardware counters would show cache and TLB effects (cause 4 of the earlier note).
- **Open:** the numbers come from one CPU type under KVM. A GitHub runner (AMD EPYC for `ubuntu-latest`, per `docs/design.md`) may show a different ERMS and FSRM picture.

## How this was measured

- **Builds:** `cargo build --release --locked -p oskac` and the same with `--target x86_64-unknown-linux-musl`, Rust 1.100.0, `CARGO_INCREMENTAL=0`, in copies of the repo under the session's scratch directory. The patch added `crates/oska-sys/src/mem.rs` with the six functions (kept: `memcpy` and `memmove`), declared in `lib.rs` behind the `cfg` above. The repo itself is unchanged. LTO builds used `CARGO_PROFILE_RELEASE_LTO=fat` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` (and `CARGO_PROFILE_RELEASE_PANIC=abort`). The `target-cpu` build used `RUSTFLAGS=-C target-cpu=x86-64-v3`. PGO used `-Cprofile-generate`, `llvm-profdata merge` (the `llvm-tools` component) and `-Cprofile-use`, trained by one `oskac build site/build.osk` and one `oskac run --interpret site/build.osk`, and held out the five `bench-native.sh` programs.
- **Timing:** a small script took the user plus system CPU time of a child process, best of N. `scripts/bench-native.sh 3` for the workload table (two invocations per binary). The interpreter site run is 7 to 12 s, so one run is one sample; a run is noisy by about 10%.
- **Microbenchmark:** a Rust program (not in the repo) with each copy routine behind a function pointer, 28 sizes from 1 B to 1 MiB, 64-byte-aligned buffers (and a second run with the source one byte off), best of 7 repeats per point, two full runs. Built once for musl (static) and once for glibc (dynamic). It checks each routine against the source for a few sizes.
- **Counting build:** each `mem*` entry increments a relaxed atomic in a log2 size bucket, and a `.fini_array` function writes the table to fd 2 with a raw `write` system call.
- **Not done:** no `perf`, `callgrind` or sampling (no `perf` here, and `callgrind` counts each `rep` iteration as an instruction, which misstates a `rep movsq`). No aarch64. No CPU without ERMS. No post-#570 tree.

## Sources

Everything in this list was read today (2026-11-22) from `master` of each repository, or from the installed 1.100.0 toolchain. I could not record commit hashes: the GitHub API was not reachable from this session.

1. musl `src/string/x86_64/memcpy.s`, `memmove.s`, `memset.s`, `src/string/memcmp.c`, `memchr.c`, `memmove.c`, `aarch64/memcpy.S`, `COPYRIGHT`, `VERSION` (1.2.6), `ldso/dynlink.c`: https://github.com/kraj/musl/tree/master (mirror of git.musl-libc.org, which was not reachable)
2. glibc `sysdeps/x86_64/multiarch/ifunc-memmove.h`, `memmove-vec-unaligned-erms.S`, `sysdeps/x86/cacheinfo.h`, `sysdeps/aarch64/multiarch/memcpy.c` and `memcpy_mops.S`: https://github.com/bminor/glibc (mirror of sourceware.org, which was not reachable)
3. Arm Optimized Routines, `string/aarch64/memcpy.S`, `LICENSE`: https://github.com/ARM-software/optimized-routines
4. compiler-builtins, `compiler-builtins/Cargo.toml`, `src/macros.rs`, `src/mem/mod.rs`, `src/mem/x86_64.rs`: https://github.com/rust-lang/compiler-builtins
5. The Rust Reference, `no_mangle` (`src/abi.md`), `no_builtins` (`src/attributes/codegen.md`), `global_allocator` (`src/runtime.md`): https://github.com/rust-lang/reference
6. The rustc book, codegen options `lto`, `codegen-units`, `panic`, `target-cpu`, `target-feature` (`src/doc/rustc/src/codegen-options/index.md`) and profile-guided optimization: https://github.com/rust-lang/rust/tree/master/src/doc/rustc/src
7. LLVM BOLT: https://github.com/llvm/llvm-project/blob/main/bolt/README.md
8. mimalloc licence: https://github.com/microsoft/mimalloc/blob/master/LICENSE
9. The Rust 1.100.0 toolchain on this machine: `rustlib/x86_64-unknown-linux-musl/lib/self-contained/libc.a` (musl 1.2.5) and `libcompiler_builtins-*.rlib`, read with `nm`, `objdump` and `strings`.
10. Repo files: [2026-11-musl-oskac-performance.md](2026-11-musl-oskac-performance.md), [2026-11-oskac-without-glibc.md](2026-11-oskac-without-glibc.md), [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md), [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md), [scripts/bench-native.sh](../../scripts/bench-native.sh), [crates/oska-sys](../../crates/oska-sys/), [crates/oska-check/src/std/rt.osk](../../crates/oska-check/src/std/rt.osk), [docs/agents/merge-policy.md](../agents/merge-policy.md), `/mnt/project-files/musl-only/spec-draft.md` (outside the repo).
11. **Not reachable, so not cited for any claim:** the Intel Software Developer's Manual and Optimization Reference Manual (ERMSB, FSRM; the `compiler-builtins` header comment is the source used instead), the Arm Architecture Reference Manual (FEAT_MOPS and the Arm memcpy instructions; glibc's `memcpy_mops.S` shows glibc uses MOPS on aarch64, and that is all this note claims about it), `musl.libc.org`, `wiki.musl-libc.org`, `doc.rust-lang.org` (the Rust Reference and rustc book were read from their GitHub sources instead; the Cargo book's profile page was not found at the path tried).
