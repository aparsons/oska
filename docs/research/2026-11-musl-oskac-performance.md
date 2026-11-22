# Why is the musl build of `oskac` slower than the glibc build, and what is the cheapest fix?

- **Date:** 2026-11-20
- **Current status (2026-11-22):** the musl build has an allocator (section 8) and fewer short copies (section 9). It is at spec #521's 1.10x threshold on aarch64 and still above it on x86_64, so both Linux builds stay. See the Decision at the end of section 9.
- **Scope (as of 2026-11-20; later tickets added an allocator and `unsafe` code, see the updates below):** Ticket #469 of spec #461 ([ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). On 2026-11-19 the musl build took 0.32 s to `oskac build` the site generator against 0.20 s for the glibc build, and its interpreter took 5.8 s against 3.8 s. Find where the time goes, on x86_64 and aarch64, and rank the fixes. No allocator and no `unsafe` code in this ticket.
- **Status (superseded by the 2026-11-21 update below):** x86_64 measured here. **aarch64 was pending**: this machine is x86_64 only, and no aarch64 number below is measured. Section 5 gives the exact commands for an aarch64 runner.
- **Update 2026-11-21:** aarch64 measured on a GitHub `ubuntu-24.04-arm` runner (ticket #506). Section 5 has the figures: causes 1 and 2 hold there, cause 3 (`memcpy`) does not.
- **Update 2026-11-21 (ticket #507):** re-measured on both processors after fixes 1 to 6. The musl build still misses the 1.10x threshold of spec #521 on most workloads: section 7.
- **Update 2026-11-21 (ticket #523):** re-measured with the musl build's allocator (fix 7). aarch64 is at the 1.10x threshold (the site build 1.09x to 1.11x, every other workload under it in most runs); x86_64 still misses it on `text`, `bulk-text`, the site build and `oskac build`, which **(Inference)** is musl's `memcpy`: section 8.
- **Update 2026-11-21 (ticket #572):** profiled the x86_64 gap left with the allocator: `memcpy` is 97% of it on `text`, 79% on `bulk-text`, 73% on `oskac build` and 37% on the site build, whose next largest part is the kernel (musl's `mmap` churn for requests over 1 KiB). Cutting short copies took `bulk-text`'s `memcpy` calls down 99%, `text`'s 44%, the compile's 41% and the site build's 18%. x86_64 still misses 1.10x on `text` and `oskac build`, and is at it on the site build; aarch64 is at it: section 9.
- **Method:** Built both release binaries of `oskac` at `36167da` with the pinned Rust 1.100.0: `cargo build --release -p oskac` and the same with `--target x86_64-unknown-linux-musl`. Ran `scripts/bench-native.sh` once with each. Timed the site generator with `perf stat`. Profiled with `perf record` (CPU-clock sampling, as this VM has no hardware counters), `strace -c`, and `valgrind --tool=callgrind`, which counts instructions and calls exactly. Disassembled the C library routines in the binaries. Wrote a small Rust program that times `memcpy` and allocation on each C library.
- **Machine:** Intel Xeon @ 2.10GHz, 4 CPUs, a Linux 6.18 VM shared with one other agent. CPU times (user plus system) are used, as other work stretches the wall clock but not CPU time much. Each figure is the best of 3 to 5 runs. The musl build was slower in every run, but absolute times differ between runs and tools: the musl interpreter's site build took 8,333 ms in the bench script and 8,879 ms under `perf stat`, 6.5% apart.
- **Sources:** what this note measured, plus the musl release notes. The musl source site and its GitHub mirror were not reachable from this session, so each claim about how musl works is either read from the disassembly of the shipped `libc.a` or marked **Inference**.

## Short answer

The musl build is slower because of musl's C library routines, not because of Oska's code. Its instruction count is only 9% higher (63.3 billion against 58.0 billion for the interpreted site build), but it takes 70% more CPU time. On x86_64 the gap has four causes:

1. **musl's allocator (mallocng): about 44% of the interpreter's gap.** Its allocator code runs 46% more instructions for the same allocations (18.03 billion against 12.35 billion). Its lock functions, `__lock` and `__unlock`, take 26% of the gap, and only in the interpreter, which always runs on a second thread (for its 256 MiB stack). **Inference:** musl locks its allocator once a second thread exists.
2. **musl giving memory back to the kernel and taking it again: about 20% of the interpreter's gap and 35% of the compile's.** The interpreted site build makes 81,309 `mmap` and 81,264 `munmap` calls and takes 345,726 page faults. The glibc build makes 17 and 6 calls and takes 4,129 faults.
3. **musl's x86_64 `memcpy`: about 28% of the interpreter's gap and 37% of the compile's.** It is a `rep movsq` with byte loops before and after it. Each call costs about 13 ns, even for 16 bytes, where glibc's costs 2 ns. The interpreter calls it 39 million times.
4. **The rest, about 16% of the gap,** is Oska's own Rust code running slower on the same instructions. **Inference:** fresh pages and a different heap layout cost cache and TLB misses. This VM cannot count them. musl's `memcmp` is faster here, which takes back about 8%.

The causes are the C library's, but the cheapest fix is in Oska: allocate and copy less. Three interpreter sites make 88% of its 52 million allocations, and two checker sites make 87% of the compile's 689,000. Each fix helps the glibc build too. Only if the musl build is still too slow after those fixes should a custom allocator in `oska-sys` be considered (ADR 0009).

## 1. The numbers

### The bench script

`scripts/bench-native.sh` (default 5 runs, best CPU time), run once with each build. The script now takes `OSKA_BENCH_TARGET`, so the second run was `OSKA_BENCH_TARGET=x86_64-unknown-linux-musl scripts/bench-native.sh`.

| Workload | glibc interpreter | musl interpreter | musl / glibc | glibc native | musl native |
| --- | ---: | ---: | ---: | ---: | ---: |
| loop | 790 ms | 776 ms | 0.98x | 16 ms | 13 ms |
| text | 96 ms | 248 ms | 2.58x | 5 ms | 5 ms |
| bulk-text | 30 ms | 60 ms | 2.00x | 23 ms | 23 ms |
| lists | 551 ms | 598 ms | 1.09x | 17 ms | 16 ms |
| recursion | 513 ms | 808 ms | 1.58x | 4 ms | 4 ms |
| site | 5,249 ms | 8,333 ms | 1.59x | 226 ms | 228 ms |

The workload that allocates nothing (`loop`) runs at the same speed. The ones that allocate a text or an argument list on each step (`text`, `recursion`, `site`) are 1.6 to 2.6 times slower. Native executables run at the same speed from either build, as ADR 0022 says: `oskac build` writes the same bytes, and they do not use the C library.

### The site generator

`perf stat`, best of 3, from the repository root.

| Command | glibc | musl | musl / glibc | glibc page faults | musl page faults |
| --- | ---: | ---: | ---: | ---: | ---: |
| `oskac build site/build.osk` | 51 ms | 103 ms | 2.0x | 2,984 | 7,496 |
| `oskac run --interpret site/build.osk` | 5,242 ms | 8,879 ms | 1.7x | 4,129 | 345,726 |

The 2026-11-19 figures were 1.6x for the compile and 1.5x for the interpreter. Here the compile gap is larger, 2.0x, and the interpreter gap 1.7x. This machine and its load differ from that day's, and this note does not explain the difference.

## 2. Where the time goes (x86_64)

### Profiles

`perf record` at 999 Hz on the interpreted site build, so one sample is about 1 ms. Each sample is put in one group by its symbol.

| Group | glibc | musl | Change | Share of the gap |
| --- | ---: | ---: | ---: | ---: |
| Allocator: `malloc`, `free` and their helpers | 951 | 2,531 | +1,580 | 44% |
| of which musl's `__lock` and `__unlock` | 0 | 944 | +944 | 26% |
| Kernel (page faults, `mmap`, `munmap`, TLB flushes) | 63 | 795 | +732 | 20% |
| `memcpy` | 73 | 1,091 | +1,018 | 28% |
| `memcmp` and `bcmp` | 531 | 238 | −293 | −8% |
| Oska's Rust code and the rest | 3,936 | 4,528 | +592 | 16% |
| Total | 5,554 | 9,183 | +3,629 | 100% |

The compile is too short for one run, so it was profiled over 10 runs of `oskac build site/build.osk` at 1999 Hz.

| Group | glibc | musl | Change | Share of the gap |
| --- | ---: | ---: | ---: | ---: |
| Allocator | 329 | 552 | +223 | 30% |
| Kernel | 143 | 407 | +264 | 35% |
| `memcpy`, `memmove`, `memcmp` | 59 | 334 | +275 | 37% |
| Oska's Rust code and the rest | 660 | 650 | −10 | −1% |
| Total | 1,191 | 1,943 | +752 | 100% |

The compile runs on one thread, so musl takes no allocator lock there (11 samples of `__unlock` in 10 runs).

### Instruction counts

`valgrind --tool=callgrind` counts each instruction and call. Both builds run the same Rust code, so they make the same allocations. They make almost the same `memcpy` calls: the musl build makes about 100,000 more (0.3%), from calls inside the C libraries, not from Oska's code. For example, musl's `realloc` calls `memcpy` 1.3 million times.

| | glibc | musl |
| --- | ---: | ---: |
| Interpreter: all instructions | 58.00 billion | 63.26 billion |
| Interpreter: allocator instructions | 12.35 billion | 18.03 billion |
| Interpreter: allocations (`__rust_alloc` calls) | 52,370,811 | 52,370,810 |
| Interpreter: `memcpy` calls | 39,194,168 | 39,294,690 |
| Compile: all instructions | 469.5 million | 493.3 million |
| Compile: allocations | 689,410 | 689,408 |
| Compile: `memcpy` calls | (not separable) | 638,452 |

So the musl build does the same work with 9% more instructions, and takes 70% more time. Most of the gap is cost per call that instructions do not show: atomic lock operations, system calls, page faults and the start-up of `rep movsq`.

## 3. The causes, with evidence

### Cause 1: mallocng costs more per call, and locks once a thread exists

- In the interpreter, musl's allocator takes 18.03 billion instructions: `__libc_malloc_impl` 7.07, `__libc_free` 6.93, `alloc_slot` 1.25, `__lock` and `__unlock` 1.27, `nontrivial_free` 0.48, `__libc_realloc` 0.19, and Rust's allocation wrappers and musl's `malloc` and `free` entry points 0.83. glibc's allocator, counted the same way, takes 12.35 billion. That is 46% more instructions for the same 52 million allocations.
- `__lock` and `__unlock` take 944 ms of the musl run (10% of it) and nothing in the compile. The interpreter runs the program on a thread of its own with a 256 MiB stack (`on_interpreter_stack` in [crates/oska-run/src/lib.rs](../../crates/oska-run/src/lib.rs)). **Inference:** musl skips its allocator lock while a process has one thread and takes it once there are two, which is what the profiles show.
- The small timing program agrees. Allocating and freeing a 64-byte `Vec` in a loop costs 11 ns with glibc and 23 ns with musl after a thread has started.

### Cause 2: mallocng gives memory back and asks for it again

- `strace -c` of the interpreted site build: musl makes 81,309 `mmap`, 81,264 `munmap` and 117 `mremap` calls. glibc makes 17 `mmap`, 6 `munmap` and 4 `mremap` calls, and grows its heap with `brk` (36 calls). The compile shows the same: 3,359 and 3,316 calls against 14 and 2.
- The mappings are small: 38,914 of 8 KiB, 17,602 of 36 KiB, 6,427 of 28 KiB and 3,659 of 32 KiB, and fewer of other sizes. **Inference:** these are mallocng's slot groups. When the last slot in a group is freed, the group goes back to the kernel, and the next allocation of that size maps a new one. The musl 1.2.1 release notes say mallocng returns memory "on a much finer granularity" ([musl releases](https://musl.libc.org/releases.html)).
- Each new mapping is touched page by page, so the musl run takes 345,726 page faults against 4,129. The kernel's time grows from 63 ms to 795 ms, in `exc_page_fault`, `flush_tlb_mm_range` and the `mmap` and `munmap` system calls.
- The timing program shows the worst case. Allocating and freeing one 16-byte `Vec` in a loop on one thread costs 14 ns with glibc and 203 ns with musl. `strace` shows 338,144 `mmap` and 338,140 `munmap` calls for that program.

### Cause 3: musl's x86_64 `memcpy` has a high fixed cost

- The `memcpy` in the musl binary disassembles to: copy single bytes until the destination is 8-byte aligned, `rep movsq` for the 8-byte words, then copy the last bytes one at a time. glibc picks an AVX-512 or AVX2 routine that copies small sizes with a few vector moves.
- Timed on warm buffers: 16 to 256 bytes cost 13 to 14 ns with musl and 2 to 3.5 ns with glibc. At 1 KiB it is 19 ns against 10 ns. From 4 KiB up the two are about equal.
- The interpreter calls `memcpy` 39 million times, mostly to copy a short text into a new `Rc<str>` (section 4). The compile calls it 638,452 times, mostly from `String::clone` and `Vec<FlagVar>::clone`. Copies are short, so nearly every call pays the fixed cost.
- aarch64 differs. The `memcpy` in Rust's `aarch64-unknown-linux-musl` `libc.a` branches on size and copies with paired loads and stores (`ldp` and `stp`), with no `rep` instruction. **Inference:** this cause is much smaller on aarch64, and causes 1 and 2, which are in musl's portable C allocator, apply there the same way.

### Cause 4: the rest

Oska's own code takes 592 ms longer under musl (4,528 ms against 3,936 ms, a 15% rise, and 16% of the gap) for the same instructions. **Inference:** freshly mapped pages and a different heap layout cost cache and TLB misses. This VM reports hardware counters as not supported, so this cannot be proven here.

### Not a cause: `memcmp`

musl's `memcmp` and `bcmp` take less time than glibc's here (238 ms against 531 ms). The hot caller is the interpreter's intrinsic lookup (below), which compares short names that differ early.

## 4. Where the allocations and copies come from

### The interpreter (52,370,811 allocations on the site build)

Callers of `__rust_alloc`, from callgrind on the glibc build. The musl build makes the same calls.

| Caller | Allocations | Share | `memcpy` calls | What it makes |
| --- | ---: | ---: | ---: | --- |
| `Interpreter::eval_other` | 17,265,486 | 33% | 17,336,507 | A new `Rc<str>` for each string literal and interpolation it evaluates. `ExprKind::Str` calls `memory::text`, which copies the literal's `String` each time ([crates/oska-run/src/interp.rs](../../crates/oska-run/src/interp.rs), [crates/oska-run/src/memory.rs](../../crates/oska-run/src/memory.rs)). |
| `Interpreter::eval_all` | 15,551,238 | 30% | none | The `Vec<Value>` of each call's arguments. `binding` then collects a second `Vec` for the frame's slots. |
| `data::string_chars` | 13,534,408 | 26% | 13,534,408 | One `Rc<str>` for each character of a text, for `String.chars` ([crates/oska-run/src/data.rs](../../crates/oska-run/src/data.rs)). |
| All other callers | 6,019,679 | 11% | | |

Three sites make 88% of the allocations and 79% of the `memcpy` calls. `RawVecInner::finish_grow` adds 1,321,167 `realloc` calls: vectors and strings that grow one push at a time.

A bigger cost that is not musl's: `intrinsics::lookup` runs on each of 14.7 million intrinsic calls. It scans every intrinsic table by name and splits each standard module path with `rsplit('.')`. With the `rsplit` and the `memcmp` calls it makes, it is about 25% of the glibc interpreter's instructions (`lookup` 11.5%, `SplitInternal::next_back` 5.4%, `memrchr` 4.8%, and most of `memcmp`'s 4.7%).

### The compile (689,410 allocations on the site build)

| Caller | Allocations | Share | What it makes |
| --- | ---: | ---: | --- |
| `Vec<FlagVar>::clone` | 326,347 | 47% | `Infer::flow` clones the whole inference state (`let saved = self.clone()`) before each of its 46,040 calls, so that it can undo a failed attempt. The state holds two `Flags` tables, each a `Vec` of `FlagVar` with two `Vec<u32>` each ([crates/oska-check/src/infer.rs](../../crates/oska-check/src/infer.rs), [crates/oska-check/src/infer/flags.rs](../../crates/oska-check/src/infer/flags.rs)). |
| `String::clone` | 272,724 | 40% | 80% of these come from `Ty::clone`: a type holds its name as a `String`, so each copy of a type copies its name. |
| All other callers | 90,339 | 13% | |

Separately, `Sha256::compress` is 24% of the compile's instructions in both builds. It does not explain the gap, but it is the compile's biggest single cost.

## 5. aarch64

### Measured on 2026-11-21 (ticket #506)

- **Machine:** a GitHub `ubuntu-24.04-arm` runner, Arm Neoverse-N2, 4 CPUs. Unlike the x86_64 VM above, it has hardware counters, so the profiles sample CPU cycles (`armv8_pmuv3_0/cycles`) and `perf stat` counts instructions, cycles and cache misses.
- **Builds:** both release builds of `oskac` at `12f5797` (the integration branch at `91aa329` plus a temporary job in `bench.yml`, since reverted), Rust 1.100.0, built by the bench script.
- **Method:** the commands in "How to measure it" below, plus `strace -c` of the compile and one `perf stat` run with hardware counters, run by that temporary job (workflow run 44888617764). `perf` is 7.0.14 from Ubuntu's `linux-tools`, with `kernel.perf_event_paranoid=-1`, so the profiles include kernel samples. One sample is about 1 ms. Each profile's text was cut at its first 80 lines, so the groups below count the symbols listed there (every symbol of 0.07% and more in the interpreter, 0.2% and more in the compile), and the smaller ones fall in "the rest".

#### The bench script

| Workload | glibc interpreter | musl interpreter | musl / glibc | glibc native | musl native |
| --- | ---: | ---: | ---: | ---: | ---: |
| loop | 710 ms | 704 ms | 0.99x | 15 ms | 15 ms |
| text | 126 ms | 187 ms | 1.48x | 5 ms | 5 ms |
| bulk-text | 27 ms | 28 ms | 1.04x | 24 ms | 24 ms |
| lists | 552 ms | 629 ms | 1.14x | 13 ms | 12 ms |
| recursion | 534 ms | 936 ms | 1.75x | 6 ms | 6 ms |
| site | 5,833 ms | 8,058 ms | 1.38x | 198 ms | 197 ms |

The same pattern as on x86_64: no gap for `loop`, the largest for the workloads that allocate on each step, and native executables at the same speed from either build. The glibc bench job of the same run, on another runner, measured within 3% of these glibc figures (site 5,906 ms).

#### The site generator

`perf stat`, best of 3 by `task-clock`.

| Command | glibc | musl | musl / glibc | glibc page faults | musl page faults |
| --- | ---: | ---: | ---: | ---: | ---: |
| `oskac build site/build.osk` | 42 ms | 66 ms | 1.56x | 2,911 | 8,487 |
| `oskac run --interpret site/build.osk` | 5,839 ms | 8,065 ms | 1.38x | 4,414 | 373,783 |

Of the interpreter's 8,065 ms under musl, 776 ms is system time; glibc's is 48 ms. One more interpreted run with hardware counters:

| Counter | glibc | musl | musl / glibc |
| --- | ---: | ---: | ---: |
| Instructions | 64.78 billion | 72.18 billion | 1.11x |
| Cycles | 19.74 billion | 27.34 billion | 1.38x |
| Instructions per cycle | 3.28 | 2.64 | 0.80x |
| Cache misses | 121.6 million | 215.4 million | 1.77x |
| dTLB load misses | not supported | not supported | |

As on x86_64, the musl build runs about 10% more instructions and takes much longer: its instructions run slower, and it misses the cache 77% more often.

#### System calls

`strace -f -c`.

| Call | glibc interpreter | musl interpreter | glibc compile | musl compile |
| --- | ---: | ---: | ---: | ---: |
| `mmap` | 15 | 85,655 | 12 | 4,226 |
| `munmap` | 10 | 85,612 | 6 | 4,183 |
| `mremap` | 4 | 122 | 4 | 9 |
| `brk` | 37 | 19 | 39 | 19 |
| All calls | 4,900 | 176,028 | 163 | 8,513 |

#### Profiles

The interpreted site build, sampled at 999 Hz (glibc 5,870 samples, musl 8,206):

| Group | glibc | musl | Change | Share of the gap |
| --- | ---: | ---: | ---: | ---: |
| Allocator: `malloc`, `free` and their helpers | 1,406 | 3,040 | +1,634 | 70% |
| of which locks: musl's `__lock` and `__unlock`, glibc's atomic helpers (`__aarch64_swp4_rel`, `__aarch64_cas8_rel` and others) | 212 | 1,106 | +894 | 38% |
| Kernel (TLB flushes, page faults, `mmap`, `munmap`) | 0 listed | 462 | +462 | 20% |
| `memcpy` | 75 | 104 | +29 | 1% |
| `memcmp` and `bcmp` | 224 | 324 | +100 | 4% |
| Oska's Rust code and the rest | 4,165 | 4,276 | +111 | 5% |
| Total | 5,870 | 8,206 | +2,336 | 100% |

The kernel row counts only the kernel symbols the cut profile lists. `perf stat`'s system time (776 ms against 48 ms) puts the kernel's real share near 31%. The kernel samples that the cut leaves out fall in "the rest", so Oska's own code takes about the same time, or a little less, under musl. Its hottest symbols agree: `intrinsics::lookup` takes 822 samples under musl and 895 under glibc, and `Interpreter::binary` 301 and 394.

The compile, profiled over 10 runs at 1999 Hz (only the `oskac` process: glibc about 880 samples, musl about 1,392):

| Group | glibc | musl | Change | Share of the gap |
| --- | ---: | ---: | ---: | ---: |
| Allocator | 352 | 413 | +61 | 12% |
| Kernel | 46 | 402 | +356 | 70% |
| `memcpy` and `memcmp` | 19 | 27 | +8 | 2% |
| Oska's Rust code and the rest | 463 | 550 | +87 | 17% |
| Total | 880 | 1,392 | +512 | 100% |

The compile runs on one thread, and no `__lock` or `__unlock` sample shows in it. The kernel's largest symbol is `tlb_flush_mmu_tlbonly` (121 samples), the TLB flush after each `munmap`.

#### Do the x86_64 causes hold on aarch64?

| Cause | x86_64 (interpreter's gap) | aarch64 (interpreter's gap) | Holds? |
| --- | ---: | ---: | --- |
| 1. mallocng's cost per call and its lock | 44%, lock 26% | 70%, lock 38% | **Yes, and larger.** `__lock` and `__unlock` take 1,106 ms, 13.5% of the musl run, and only in the threaded interpreter. glibc on aarch64 also spends 212 ms in atomic helpers, which x86_64 inlines. |
| 2. mmap churn | 20% (81,309 `mmap`, 345,726 faults) | 20% to 31% (85,655 `mmap`, 373,783 faults) | **Yes, the same.** It is the largest cause of the compile's gap (70%). |
| 3. musl's `memcpy` | 28% | 1% | **No.** musl's aarch64 `memcpy` (paired `ldp` and `stp`, no `rep`) costs about what glibc's `__memcpy_sve` does. Section 3's inference is confirmed. |
| 4. Oska's code running slower | 16% | about 0% | **No clear sign.** The cache misses rise 77%, but the time in Oska's own code does not grow. `memcmp` is a little slower under musl here, where it was faster on x86_64. |

So on aarch64 the gap is smaller (1.38x for the interpreted site build against 1.7x) because `memcpy` drops out, and it comes almost all from the allocator: its lock, its code, and the mappings it gives back and takes again. Fixes 1, 2 and 6 in section 6 attack exactly that, by allocating less. Fix 7 (a size-class allocator) would remove causes 1 and 2, which on aarch64 are almost the whole gap.

#### Instruction counts

`valgrind --tool=callgrind` on the interpreted site build, in a second job of the same run. Only the first 80 lines of `callgrind_annotate --inclusive=no` were kept, so the allocator and copy rows add up the functions listed there (each 0.08% of the run or more) and are lower bounds.

| | glibc | musl | musl / glibc |
| --- | ---: | ---: | ---: |
| Interpreter: all user-space instructions | 64.82 billion | 67.64 billion | 1.04x |
| Allocator instructions (listed functions) | 14.84 billion | 19.21 billion | 1.29x |
| of which musl's `__lock` and `__unlock` | | 1.74 billion | |
| `memcpy` instructions | 1.07 billion | 1.16 billion | 1.09x |
| `memcmp` and `bcmp` instructions | 3.02 billion | 3.36 billion | 1.11x |

The user-space instruction counts differ by 4%. `perf stat` counted 11% more instructions under musl because it also counts the kernel's, spent on musl's `mmap`, `munmap` and page faults. Oska's own functions run the same instruction counts in both builds (for example `Interpreter::eval` 4.38 billion and `ops::binary` 1.43 billion in each).

### How to measure it (written 2026-11-20)

Not measured. This machine is x86_64 only, and no figure in this note is from aarch64. To fill in sections 1 to 3 for aarch64, run these on an aarch64 Linux machine (the maintainer's Raspberry Pi 5, or a GitHub `ubuntu-24.04-arm` runner) from the repository root, with `perf`, `strace` and `valgrind` installed (`sudo apt-get install -y linux-tools-generic strace valgrind` on Ubuntu; on a runner, `perf` may need `sudo sysctl kernel.perf_event_paranoid=1`):

```sh
export CARGO_INCREMENTAL=0
rustup target add aarch64-unknown-linux-musl
gnu=target/release/oskac
musl=target/aarch64-unknown-linux-musl/release/oskac

# The bench script, once with each build.
scripts/bench-native.sh | tee bench-gnu.txt
OSKA_BENCH_TARGET=aarch64-unknown-linux-musl scripts/bench-native.sh | tee bench-musl.txt

for b in "$gnu" "$musl"; do
  name=$(basename "$(dirname "$(dirname "$b")")")
  # Site timings and page faults (repeat 3 times, keep the best).
  perf stat -e task-clock,page-faults "$b" build site/build.osk -o /tmp/site-exe
  perf stat -e task-clock,page-faults "$b" run --interpret site/build.osk >/dev/null
  # System calls.
  strace -f -c -o "strace-$name.txt" "$b" run --interpret site/build.osk >/dev/null
  # Profiles: the interpreter once, the compile ten times.
  perf record -F 999 -o "interp-$name.data" -- "$b" run --interpret site/build.osk >/dev/null
  perf report -i "interp-$name.data" --no-children --sort dso,sym -n --stdio | head -60
  perf record -F 1999 -o "build-$name.data" -- bash -c \
    "for i in 1 2 3 4 5 6 7 8 9 10; do $b build site/build.osk -o /tmp/site-exe; done"
  perf report -i "build-$name.data" --comm oskac --no-children --sort dso,sym -n --stdio | head -60
done

# Exact instruction and call counts (slow: about 10 minutes each).
for b in "$gnu" "$musl"; do
  name=$(basename "$(dirname "$(dirname "$b")")")
  valgrind --tool=callgrind --callgrind-out-file="cg-$name.out" "$b" run --interpret site/build.osk >/dev/null
  callgrind_annotate --threshold=100 --tree=caller --inclusive=no "cg-$name.out" >"cg-$name.txt"
done
```

What to look for: the `__lock` and `__unlock` share, the `mmap` and `munmap` counts and page faults (causes 1 and 2), and whether `memcpy` still costs much more under musl (cause 3, expected to be small on aarch64).

## 6. Fixes, cheapest and safest first

Each of fixes 1 to 6 is safe Rust in an existing crate. It helps both builds and adds no `unsafe` and no dependency. The savings are **Inference** from the counts above. Each fix's ticket must measure them with the bench script on both builds.

| # | Fix | Removes (site build) | Cost | Risk |
| --- | --- | --- | --- | --- |
| 1 | **Make each string literal's `Rc<str>` once.** Lower `ExprKind::Str` to hold an `Rc<str>`, or keep a table of them, and clone the `Rc` when it is evaluated. A metered run (`--max-memory`) must still charge for the text as it does today. | Up to 17 million of 52 million interpreter allocations, and as many `memcpy` calls. | Small: `oska-check`'s program type and `oska-run`'s `eval_other`. | Low. The metering tests in `oska-run` cover the charge. |
| 2 | **Share one-character texts in `String.chars`.** Keep a table of `Rc<str>` for the 128 ASCII characters, made once for each interpreter, and clone from it. | Up to 13.5 million allocations and `memcpy` calls. | Small: `data::string_chars`. | Low, if metering still charges each text. |
| 3 | **Resolve intrinsics once.** Look each intrinsic up when the program is lowered, or once for each name in a map, not on every call. | About 25% of the interpreter's instructions on both builds. Not a musl cause, but the cheapest large win. | Small to medium: `Callee::Intrinsic` holds the resolved function. | Low. |
| 4 | **Stop `Infer::flow` cloning the inference state on every call.** Record the changes an attempt makes (a trail) and undo only those, or clone only when the first attempt fails. | Up to 326,000 of 689,000 compile allocations. | Medium: `oska-check`'s `infer`. | Medium: the checker's correctness. The checker's tests cover it. |
| 5 | **Share type names.** Hold `Ty`'s names as `Rc<str>`, so cloning a type does not copy its name. | Up to 218,000 compile allocations. | Medium: `Ty` is used widely in `oska-check`. | Low to medium. |
| 6 | **One `Vec` per call, not two.** Bind arguments into the frame's slots without collecting a second `Vec` in `binding`, and give `Vec`s that grow one push at a time a capacity. | Up to 15.5 million allocations, and 1.3 million `realloc` calls. | Medium: `call_with`, `eval_all`. | Low. |
| 7 | **Only if the musl build is still too slow after 1 to 6:** a size-class allocator in `oska-sys`, used only by the musl build. | Causes 1 and 2 entirely. Not cause 3. | Days. New `unsafe` code under ADR 0009, and a security review. | High. |

Not recommended:

- **Running the interpreter on the main thread** to avoid musl's lock. The main thread's stack is limited (8 MiB by default), and deep Oska recursion needs the 256 MiB thread stack. It would remove only the lock (26% of the interpreter's gap).
- **A third-party allocator crate.** The workspace has no external crates (the check step "no external crates").
- **Replacing `memcpy`.** It needs `unsafe` code. Fixes 1 and 2 remove most of the calls instead.

Follow-up tickets: #500 (string literals), #501 (`String.chars`), #502 (intrinsic lookup), #503 (`Infer::flow`), #504 (`Ty` names), #505 (call `Vec`s), #506 (aarch64 measurement), #507 (allocator decision, last).

## 7. Re-measured after fixes 1 to 6 (2026-11-21, ticket #507)

Spec #521's threshold: on each `scripts/bench-native.sh` workload and on the site build, interpreter and `oskac build`, the musl build's CPU time is at most 1.10x the glibc build's, on x86_64 and on aarch64.

- **Commit:** `fb86bc4b`, which is the integration branch `spec/521-musl-only` at `5592ce14` (fixes 1 to 6, #500 to #505, merged) plus the `bench.yml` change below. Rust 1.100.0.
- **Workflow:** `.github/workflows/bench.yml`, which now runs on each Linux runner, in one job: the bench script with the glibc build, the bench script with `OSKA_BENCH_TARGET=<arch>-unknown-linux-musl`, then `oskac build site/build.osk` with each build, best CPU time (user plus system) of 5. So each ratio below compares two figures from the same machine.
- **Runs:** three dispatches, workflow runs 44925905934 (A), 44926133879 (B) and 44926380054 (C). Each figure is the best of 5 runs. The second and third were dispatched because some ratios were within 1.05x to 1.15x.
- **Runners:** `ubuntu-24.04-arm` was an Arm Neoverse-N2 with 4 CPUs in all three runs. `ubuntu-latest` was a different x86_64 processor each time: AMD EPYC 9V45 (A), Intel Xeon 6975P-C (B) and AMD EPYC 9V74 (C), 4 CPUs each.
- The "site" row is `oskac run --interpret site/build.osk`, which includes checking the program. "`oskac build`" is the compile of the site.

### aarch64 (Neoverse-N2)

| Workload | glibc (A / B / C) | musl (A / B / C) | musl / glibc (A / B / C) | At most 1.10x? |
| --- | ---: | ---: | ---: | --- |
| loop | 669 / 673 / 670 ms | 685 / 686 / 683 ms | 1.02 / 1.02 / 1.02 | Yes |
| text | 64 / 64 / 64 ms | 88 / 88 / 88 ms | 1.38 / 1.38 / 1.38 | No, by 0.28 |
| bulk-text | 27 / 26 / 27 ms | 30 / 28 / 30 ms | 1.11 / 1.08 / 1.11 | No in 2 of 3 runs, by 0.01 (3 ms in 27 ms, at the 1 ms resolution of the timer) |
| lists | 343 / 342 / 343 ms | 356 / 358 / 358 ms | 1.04 / 1.05 / 1.04 | Yes |
| recursion | 520 / 521 / 520 ms | 640 / 640 / 640 ms | 1.23 / 1.23 / 1.23 | No, by 0.13 |
| site (interpreter) | 3,158 / 3,159 / 3,160 ms | 3,846 / 3,859 / 3,876 ms | 1.22 / 1.22 / 1.23 | No, by 0.12 |
| `oskac build` of the site | 34 / 34 / 34 ms | 43 / 43 / 44 ms | 1.26 / 1.26 / 1.29 | No, by 0.16 to 0.19 |

The three runs agree to within 1%. Against ticket #506's figures on the same runner type (section 5), fixes 1 to 6 took the glibc site build from 5,833 ms to 3,158 ms and the musl one from 8,058 ms to 3,846 ms, and the gap from 1.38x to 1.22x. `text` went from 1.48x to 1.38x, `recursion` from 1.75x to 1.23x, and `oskac build` from 1.56x (`perf stat`) to 1.26x.

### x86_64

| Workload | glibc (A / B / C) | musl (A / B / C) | musl / glibc (A / B / C) | At most 1.10x? |
| --- | ---: | ---: | ---: | --- |
| loop | 533 / 577 / 985 ms | 523 / 579 / 986 ms | 0.98 / 1.00 / 1.00 | Yes |
| text | 42 / 45 / 78 ms | 78 / 123 / 113 ms | 1.86 / 2.73 / 1.45 | No, by 0.35 to 1.63 |
| bulk-text | 26 / 23 / 41 ms | 43 / 45 / 57 ms | 1.65 / 1.96 / 1.39 | No, by 0.29 to 0.86 |
| lists | 279 / 304 / 523 ms | 372 / 314 / 530 ms | 1.33 / 1.03 / 1.01 | Yes in 2 of 3 runs; 1.33 in run A |
| recursion | 388 / 420 / 775 ms | 440 / 476 / 828 ms | 1.13 / 1.13 / 1.07 | No in 2 of 3 runs, by 0.03 |
| site (interpreter) | 2,569 / 2,639 / 4,321 ms | 3,295 / 2,233 / 5,592 ms | 1.28 / 0.85 / 1.29 | No on both AMD runners, by 0.18 to 0.19; 0.85 on the Intel one |
| `oskac build` of the site | 30 / 34 / 48 ms | 44 / 46 / 67 ms | 1.47 / 1.35 / 1.40 | No, by 0.25 to 0.37 |

The x86_64 runners differ from run to run, so their absolute times do too (run C's are about 1.7x run A's), and the ratios are noisier than on aarch64. Two results do not fit the rest and this note does not explain them: `lists` at 1.33x in run A, and the musl site build faster than the glibc one (0.85x) on the Intel runner of run B. Against section 1 (a different machine, so only roughly), the site gap fell from 1.59x to about 1.28x and `recursion` from 1.58x to about 1.1x, while `text` (2.58x then, 1.45x to 2.73x now) and `bulk-text` (2.00x then, 1.39x to 1.96x now) moved less.

### Verdict

The musl build does not meet spec #521's threshold (1.10x).

- **Meets it on both processors:** `loop`, and `lists` (except x86_64 run A).
- **Misses it on both processors:** `text` (1.38x on aarch64, 1.45x to 2.73x on x86_64), the site build in the interpreter (1.22x on aarch64, 1.28x on two of three x86_64 runners), and `oskac build` of the site (1.26x to 1.29x on aarch64, 1.35x to 1.47x on x86_64).
- **Misses it on one processor:** `recursion` (1.23x on aarch64; 1.07x to 1.13x on x86_64, a miss in 2 of 3 runs) and `bulk-text` (1.39x to 1.96x on x86_64; 1.08x to 1.11x on aarch64, a miss by one timer tick in 2 of 3 runs).

The largest misses are the workloads that allocate a text on each step (`text`, `bulk-text` on x86_64) and the compile, which is where causes 1 to 3 of section 3 act. Fixes 1 to 6 narrowed the gap but did not close it, so by spec #521 fix 7 (an allocator for the musl build) applied. Fix 7 removes causes 1 and 2 but not cause 3 (`memcpy` on x86_64), which **(Inference)** `bulk-text` and `text`, the workloads that copy the most text, are most exposed to.

## 8. Re-measured with the musl build's allocator (2026-11-21, ticket #523)

Fix 7 of section 6: `oska_sys::SizeClassAlloc` ([crates/oska-sys/src/heap.rs](../../crates/oska-sys/src/heap.rs), [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)), the musl build's global allocator. Requests of up to 1 KiB come from size classes kept for the life of the process, with no lock on a thread's fast path; larger ones go to musl's `malloc`. It removes causes 1 and 2 of section 3 (mallocng's lock, and its `mmap` and `munmap` churn) and leaves cause 3 (`memcpy`).

- **Commit:** `0ed216f5`, which is the integration branch at `380944dd` (section 7's code) plus the allocator. Rust 1.100.0.
- **Workflow and method:** as in section 7: `.github/workflows/bench.yml`, both builds on the same runner in one job, best CPU time (user plus system) of 5 runs.
- **Runs:** three dispatches on the ticket branch, workflow runs 44930171428 (D), 44930202833 (E) and 44930897672 (F).
- **Runners:** `ubuntu-24.04-arm` was an Arm Neoverse-N2 with 4 CPUs in all three runs. `ubuntu-latest` was an Intel Xeon Platinum 8573C in runs D and F and an AMD EPYC 9V74 in run E, 4 CPUs each.

### aarch64 (Neoverse-N2)

| Workload | glibc (D / E / F) | musl (D / E / F) | musl / glibc (D / E / F) | Section 7 | At most 1.10x? |
| --- | ---: | ---: | ---: | ---: | --- |
| loop | 673 / 676 / 674 ms | 665 / 668 / 665 ms | 0.99 / 0.99 / 0.99 | 1.02 | Yes |
| text | 64 / 65 / 64 ms | 57 / 57 / 57 ms | 0.89 / 0.88 / 0.89 | 1.38 | Yes |
| bulk-text | 27 / 27 / 27 ms | 28 / 28 / 30 ms | 1.04 / 1.04 / 1.11 | 1.08 to 1.11 | Yes in 2 of 3 runs; 1.11 in run F (3 ms in 27 ms, at the timer's 1 ms resolution) |
| lists | 343 / 344 / 346 ms | 351 / 352 / 353 ms | 1.02 / 1.02 / 1.02 | 1.04 to 1.05 | Yes |
| recursion | 520 / 521 / 521 ms | 505 / 505 / 505 ms | 0.97 / 0.97 / 0.97 | 1.23 | Yes |
| site (interpreter) | 3,158 / 3,239 / 3,184 ms | 3,452 / 3,571 / 3,530 ms | 1.09 / 1.10 / 1.11 | 1.22 to 1.23 | Only in run D; runs E (1.103) and F (1.109) are just over |
| `oskac build` of the site | 33 / 36 / 41 ms | 36 / 40 / 39 ms | 1.09 / 1.11 / 0.95 | 1.26 to 1.29 | Yes in 2 of 3 runs; 1.11 in run E (4 ms in 36 ms) |

The allocator closed most of the aarch64 gap. `text` went from 1.38x to 0.89x and `recursion` from 1.23x to 0.97x: the musl build is now the faster one on both. The site build went from 1.22x to 1.09x to 1.11x, and `oskac build` from 1.26x to 1.29x to 0.95x to 1.11x. What is left of the site gap is 290 to 350 ms, and this note did not profile it.

### x86_64

| Workload | glibc (D / E / F) | musl (D / E / F) | musl / glibc (D / E / F) | Section 7 | At most 1.10x? |
| --- | ---: | ---: | ---: | ---: | --- |
| loop | 628 / 998 / 731 ms | 630 / (5) / 734 ms | 1.00 / — / 1.00 | 0.98 to 1.00 | Yes |
| text | 49 / 77 / 57 ms | 108 / 92 / 127 ms | 2.20 / 1.19 / 2.23 | 1.45 to 2.73 | No, by 0.09 to 1.13 |
| bulk-text | 26 / 41 / 31 ms | 52 / 57 / 62 ms | 2.00 / 1.39 / 2.00 | 1.39 to 1.96 | No, by 0.29 to 0.90 |
| lists | 334 / 526 / 384 ms | 323 / 517 / 385 ms | 0.97 / 0.98 / 1.00 | 1.01 to 1.33 | Yes |
| recursion | 458 / 781 / 532 ms | 461 / 767 / 541 ms | 1.01 / 0.98 / 1.02 | 1.07 to 1.13 | Yes |
| site (interpreter) | 2,921 / 4,418 / 3,408 ms | 3,336 / 5,146 / 3,929 ms | 1.14 / 1.16 / 1.15 | 1.28 to 1.29 (0.85 once) | No, by 0.04 to 0.06 |
| `oskac build` of the site | 34 / 46 / 44 ms | 40 / 55 / 52 ms | 1.18 / 1.20 / 1.18 | 1.35 to 1.47 | No, by 0.08 to 0.10 |

Run E's musl `loop` figure, 5 ms against 998 ms for the glibc build, is not a run time this note can explain. `scripts/bench-native.sh` checks that both backends print the same output once before timing, but does not check the exit status of the timed runs, so one of the five runs may have stopped early. It was not reproduced: on this session's x86_64 VM, the musl build of `0ed216f5` ran `loop.osk` 40 times, `examples/hello.osk` 500 times (100 of them two at a time) and `site/build.osk` 12 times, and every run exited 0 with the right output. The row is left out of the verdict. The wall time of that run matched the glibc build's, and the security review found that it was not an allocator failure, so it is not read as a speed result of the musl build.

The allocator removed the allocation-bound part of the x86_64 gap too: `recursion` went from about 1.1x to 1.0x, `lists` stays at about 1.0x, the site build from about 1.28x to about 1.15x, and `oskac build` from about 1.4x to about 1.19x. The workloads that copy the most short texts did not move: `text` is 2.20x and 2.23x and `bulk-text` 2.00x on the two Intel runners, as before (section 7's Intel run had 2.73x and 1.96x), and 1.19x and 1.39x on the AMD runner. **Inference:** this is cause 3, musl's x86_64 `memcpy` (`rep movsq` with byte loops around it), which costs most on these Intel processors. The same workloads on aarch64, where musl's `memcpy` is not slow (section 5), are now at 0.89x and 1.04x, so the copies, not the allocations, are what is left.

### Memory

The security review of ticket #523 found two ways the allocator makes the musl build use more memory than the glibc build, with two repro programs. Each was re-measured on this session's x86_64 VM, as peak resident memory of one run, with the security review's glibc and musl builds of the ticket branch before the fix below, and the musl build with the fix (Rust 1.100.0, release profile).

| Repro | glibc | musl, before the fix | musl, with the exit hook |
| --- | ---: | ---: | ---: |
| `oskac test --interpret` of 3,000 test blocks, each filling lists of texts in 63 sizes | 22 MiB | 229 MiB | 20 MiB |
| `oskac run --interpret --max-memory=150000000` of twenty lists of 100,000 texts, one size class at a time | 144 MiB | 698 MiB | 698 MiB |

- **Threads that end.** `oskac test` runs each test block on a thread of its own, and an ending thread left the blocks on its free lists unused, up to about 140 KiB a thread (about 70 KiB a thread in the first repro). The allocator now gives an ending thread's lists back to the shared pool through an exit hook (`heap.rs`, "A thread's exit gives its lists back"), and the first repro is back at the glibc build's figure. `crates/oska-sys/tests/allocator_threads.rs` is its test: 400 threads, one after another, took 128 regions of 256 KiB without the hook.
- **One class after another.** A freed block stays in its size class, so the heap holds the sum of each class's peak, not the most the program holds at once: about 4.8 times the glibc build in the second repro, and up to 20 times (the number of classes) in the worst case. `--max-memory` charges values, not the heap, so it no longer bounds the musl build's memory by itself. This is documented in `heap.rs` and [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md), not fixed: bounding it means giving whole regions back or moving free memory between classes, which needs a different allocator design.

### Verdict

With the allocator, the musl build still does not meet spec #521's 1.10x threshold on every workload.

- **aarch64: at the threshold, not clearly under it.** `loop`, `text`, `lists` and `recursion` meet it in every run. `bulk-text` and `oskac build` meet it in two of three runs, each missing once by one timer tick. The site build in the interpreter is at 1.09x, 1.10x and 1.11x: under the threshold in one run and just over it in two.
- **x86_64: misses it.** `loop`, `lists` and `recursion` meet it. `text` (1.19x to 2.23x), `bulk-text` (1.39x to 2.00x), the site build (1.14x to 1.16x) and `oskac build` (1.18x to 1.20x) miss it.
- **What is left is mostly `memcpy` on x86_64 (Inference).** The allocator took the allocation causes (1 and 2) out: `loop`, `lists` and `recursion`, the workloads that mostly allocate, are now at 0.97x to 1.02x on both processors. The workloads that also copy much text are not: `text` is 1.19x to 2.23x and `bulk-text` 1.39x to 2.00x on x86_64 (section 9 has the figures after the short-copy fixes). Spec #521 rules out replacing `memcpy` (section 3 of [2026-11-oskac-without-glibc.md](2026-11-oskac-without-glibc.md), "2026-11-21"). The next step there is step 6 of its plan: cut more short copies in Oska, for example by keeping texts as shared slices, so `text` and `bulk-text` copy less. That needs its own ticket, and so does profiling the 290 to 350 ms left in the aarch64 site build.

## 9. Short copies on x86_64 (2026-11-21, ticket #572)

Section 8 left the x86_64 gap on `text`, `bulk-text`, the site build and `oskac build`, and put it, without measuring, on musl's `memcpy`. This section measures it, cuts the short copies the profile names, and re-measures. musl's `memcpy` itself is not replaced or wrapped (spec #521).

- **Commits:** the profile below is of `5d2868e8`, the integration branch `spec/521-musl-only` with the allocator (section 8). The call counts and bench figures after it compare `be4008f1`, the integration branch once it merged `main` (spec #209), with the ticket branch merged with it, `6c47e617`. Rust 1.100.0.
- **Machine for the profiles:** this session's VM, an Intel Xeon @ 2.10GHz with 4 CPUs, as in sections 1 to 3, shared with other agents (load 4 to 6), so only ratios from the same tool and run are compared. Both release builds, `cargo build --release --locked -p oskac` with and without `--target x86_64-unknown-linux-musl`. `perf record` (CPU-clock sampling): `text` and `bulk-text` 10 runs each at 4,999 Hz, the interpreted site build once at 999 Hz, `oskac build site/build.osk` 20 runs at 4,999 Hz with a warm build cache of its own (`OSKA_CACHE_DIR`). Call counts from `valgrind --tool=callgrind`, by direct caller of `memcpy`.

### How much of the gap is `memcpy` (measured)

Samples of the musl build with its allocator, against the glibc build, grouped by symbol. "Gap" is the musl build's extra samples; each group's share is its extra samples over the gap.

| Workload | glibc samples | musl samples | Gap | `memcpy` (glibc / musl) | `memcpy`'s share of the gap | Kernel's share | Allocator's share |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| text | 3,826 | 7,017 | 3,191 | 231 / 3,316 | 97% | 0% | 6% |
| bulk-text | 1,754 | 3,839 | 2,085 | 256 / 1,908 | 79% | 9% | 1% |
| site (interpreter) | 3,487 | 4,203 | 716 | 38 / 306 | 37% | 41% | −1% |
| `oskac build` of the site | 4,888 | 5,594 | 706 | 119 / 632 | 73% | 68% | −46% |

So the inference of section 8 holds for `text` and `bulk-text`, where `memcpy` is almost the whole gap, and for the compile, where it is the largest part. In the site build it is about a third: as much again is the kernel, and the rest is Oska's own code running slower (cause 4). (The site's `memcmp` is 120 samples faster under musl, −17%.)

Where the kernel's share comes from: requests over 1 KiB still go to musl's `malloc` (section 8), and mallocng still maps and unmaps their groups. The interpreted site build makes 23,857 `mmap` and 23,816 `munmap` calls and takes 181,606 page faults under musl, against 5,896 page faults under glibc; the compile makes 967 and 933 calls and takes 5,834 faults against 3,760. This ticket does not change the allocator.

**Why short copies cost so much.** musl's x86_64 `memcpy` is a `rep movsq` with single `movsb` instructions before and after it (section 3). Timed on warm buffers on this Xeon, with a small Rust program like section 3's (2 million copies of each size through `copy_from_slice`, built for both targets; not in the repository), a copy costs:

| Bytes | 1 | 3 | 5 | 8 | 16 | 32 | 256 | 1,024 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| glibc | 4 to 5 ns | 3 to 6 ns | 4 to 5 ns | 4 to 5 ns | 3 to 4 ns | 4 ns | 4 to 7 ns | 11 to 16 ns |
| musl | 24 to 27 ns | 27 to 31 ns | 33 to 34 ns | 14 to 15 ns | 14 ns | 14 ns | 15 to 16 ns | 20 to 29 ns |

The shortest copies are the dearest: under 8 bytes, `rep movsq` runs with a count of zero and each byte is a microcoded `movsb`. Copies of one to five bytes, a character or a few digits, cost six to eight times glibc's.

### Where the copies came from

| Workload | `memcpy` calls | Biggest callers |
| --- | ---: | --- |
| text | 1,804,972 | Interpolation (5 a line: two literal pieces, two holes, the result), and each `Int` hole made into a text and copied again (4 a line) |
| bulk-text | 885,214 | `String.replace` (880,080): `str::replace` copies the piece before each match and the replacement, 2 copies for each of 11,000 matches |
| site (interpreter) | 8,470,797 | `String.join` (4,967,054, 59%): mostly `"".join(...)` of one-character texts from `String.chars`; `Html.concat` 551,231; making struct values' type names 521,339 and variants' 159,000; `Html.raw` and `TrustedHtml.new` 453,768; `String.trim` 225,194 |
| `oskac build` | 220,449 | Cloning the inference state in `impl_in` (92,701, from 5,936 of its 7,032 clones), `String::clone` 32,324 (half of them the parser cloning tokens), `format!` 24,222 |

### What changed

Each change is safe Rust in `oska-run`, `oska-check` or `oska-syntax`, with no new `unsafe`, and none of them avoids `memcpy` for a piece it does copy: each copies fewer pieces, or none. Under `--max-memory` every interpreter path below still makes and charges its own text, as before; `crates/oska-run/tests/short_copies.rs` checks the output of each path, and that an unchanged `trim`, `slice` or `replace`, `Html.raw`, `TrustedHtml.new` and an interpolated `Int` or `String` hole are each still charged as a text.

- **`String.replace`** shares the text when nothing matches, and, when the replacement is as long as what it replaces, copies the text once and overwrites each match in place (`String::replace_range` of the same length moves nothing after it), where `str::replace` made two short copies for each match.
- **`String.trim` and `String.slice`** share the text when they cut nothing.
- **`Html.raw` and `TrustedHtml.new`** share their markup instead of copying it.
- **An `Int` or `String` hole** in an interpolation is written straight into the text being built, instead of being made into a text value of its own and copied again.
- **A struct or variant value** shares its type's and variant's names, made once in a run (and let go of when the run ends), instead of copying them into each value.
- **The checker** skips an `impl` whose target names another type before it clones the inference state to try it, the parser moves an identifier's or literal's text out of its token instead of cloning the token, and two messages that were formatted for every call are formatted only when reported.

A first version also had `String.join` push a part of one character as that character rather than copy it, which took the site build's `memcpy` calls down 74% (to 2,296,476) and was in bench runs G to J below. Review dropped it: not calling `memcpy` for a short piece is a way around `memcpy`, which spec #521 rules out. `String.join` copies each part as before.

### `memcpy` calls before and after

Counted with `callgrind` on the musl build, by the integration branch after it merged `main` (`be4008f1`, before) and the ticket branch merged with it (after, at `9760b76e`). The site's generated inputs and a warm build cache were the same for both.

| Workload | Before | After | Change |
| --- | ---: | ---: | ---: |
| text | 1,808,325 | 1,005,976 | −44% |
| bulk-text | 888,573 | 6,217 | −99% |
| site (interpreter) | 8,904,132 | 7,333,502 | −18% |
| `oskac build` of the site | 228,089 | 135,216 | −41% |

What is left: in `text`, five copies a line (two literal pieces, two holes' digits and the finished text); in the site build, `String.join` (5,231,400, mostly one-character parts from `String.chars`), `Html.concat` (587,749), vectors growing (311,141), interpolation (246,072) and `split` and `lines` pieces (369,945), each a piece of text the program keeps; in the compile, the inference-state clones left (27,188), `format!` (16,484) and the lexer (15,660). The site's 558 generated files were byte-for-byte the same from the glibc build before the change and the musl build after it.

On this session's Xeon (best CPU time of 5, load 4 to 6), the musl/glibc ratios went from 2.13x to 1.79x on `text`, 1.86x to 0.94x on `bulk-text`, 1.08x to 1.10x on the site build (within this machine's noise) and 1.12x to 1.05x on `oskac build`. The glibc build got faster too: `text` from 54 to 43 ms.

### Bench figures

`.github/workflows/bench.yml`, method as in sections 7 and 8: both builds on the same runner, best CPU time of 5.

- **Final, at `9760b76e`** (the change as merged, without the `join` change): runs 44962557884 (K) and 44962563980 (L). Their x86_64 runners were an Intel Xeon 6975P-C (K) and an Intel Xeon Platinum 8573C (L), the model of section 8's runs D and F.
- **With the `join` change, at `6c47e617`:** runs 44956570028 (G), 44956576981 (H), 44957555281 (I) and 44957561940 (J), on an AMD EPYC 9V74 (G), an AMD EPYC 7763 (H and J) and an Intel Xeon Platinum 8573C (I).
- **Before, at `be4008f1`:** run 44956583874 (B), on an AMD EPYC 9V45.

Every aarch64 runner was a Neoverse-N2.

#### aarch64 (Neoverse-N2)

| Workload | glibc (K / L) | musl (K / L) | musl / glibc (K / L) | G to J | B (before) | Section 8 | At most 1.10x? |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| loop | 661 / 660 ms | 658 / 666 ms | 1.00 / 1.01 | 0.94 to 0.96 | 0.99 | 0.99 | Yes |
| text | 45 / 45 ms | 45 / 45 ms | 1.00 / 1.00 | 0.96 to 0.98 | 0.92 | 0.88 to 0.89 | Yes |
| bulk-text | 32 / 33 ms | 34 / 34 ms | 1.06 / 1.03 | 1.03 to 1.06 | 1.07 | 1.04 to 1.11 | Yes |
| lists | 346 / 347 ms | 349 / 347 ms | 1.01 / 1.00 | 0.97 to 0.98 | 1.01 | 1.02 | Yes |
| recursion | 520 / 519 ms | 510 / 510 ms | 0.98 / 0.98 | 0.98 | 0.97 | 0.97 | Yes |
| site (interpreter) | 3,355 / 3,401 ms | 3,683 / 3,746 ms | 1.098 / 1.101 | 1.097 to 1.104 | 1.110 | 1.09 to 1.11 | At it: under in K, over by 0.001 in L |
| `oskac build` of the site | 30 / 32 ms | 34 / 35 ms | 1.13 / 1.09 | 1.10 to 1.16 | 1.09 | 0.95 to 1.11 | In L; 4 ms over in K, at the timer's 1 ms resolution |

The glibc build's `text` went from 65 ms (B) to 45 ms, and the musl build's from 60 to 45 ms. Dropping the `join` change moved no aarch64 ratio by more than the runs vary: musl's `memcpy` there is not slow (section 5).

#### x86_64

| Workload | glibc (K / L) | musl (K / L) | musl / glibc (K / L) | G to J (I: Intel) | B (before, AMD) | Section 8 (Intel) | At most 1.10x? |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| loop | 574 / 710 ms | 576 / 715 ms | 1.00 / 1.01 | 1.00 to 1.02 | 0.98 | 1.00 | Yes |
| text | 35 / 44 ms | 65 / 81 ms | 1.86 / 1.84 | 1.09 to 1.15 (I: 1.86) | 1.44 | 2.20 to 2.23 | No |
| bulk-text | 26 / 34 ms | 26 / 34 ms | 1.00 / 1.00 | 1.14 to 1.15 (I: 1.00) | 1.63 | 2.00 | Yes on Intel; 1.14 to 1.15 on AMD (G to J) |
| lists | 288 / 369 ms | 299 / 416 ms | 1.04 / 1.13 | 1.01 to 1.02 | 1.00 | 0.97 to 1.00 | In K; 1.13 in L, unexplained, as section 7's run A |
| recursion | 416 / 518 ms | 421 / 524 ms | 1.01 / 1.01 | 0.99 to 1.06 | 1.03 | 0.98 to 1.02 | Yes |
| site (interpreter) | 2,812 / 3,549 ms | 3,078 / 3,969 ms | 1.095 / 1.118 | 1.07 to 1.16 (I: 1.066) | 1.14 | 1.14 to 1.15 | In K; 1.12 in L |
| `oskac build` of the site | 28 / 37 ms | 32 / 43 ms | 1.14 / 1.16 | 1.10 to 1.26 (I: 1.10) | 1.28 | 1.18 | No |

On the Intel model of section 8 (runs D and F, then I and L), the change took `bulk-text` from 2.00x to 1.00x, the site build from 1.14x and 1.15x to 1.12x (1.07x in run I with the `join` change, so that change was worth about 0.05 of the site's ratio there) and `oskac build` from 1.18x to 1.10x to 1.16x; `text` went from 2.20x and 2.23x to 1.84x to 1.86x. The AMD runners, measured only with the `join` change, had `text` at 1.09x to 1.15x and `bulk-text` at 1.14x to 1.15x.

### Verdict

The musl build still misses the 1.10x threshold of spec #521 on x86_64, and is at it, not under it, on aarch64.

- **aarch64:** `loop`, `text`, `bulk-text`, `lists` and `recursion` meet it in every run. The site build is at 1.098x to 1.104x over six runs (G to L), some a hair over. `oskac build` is at 1.09x to 1.16x, a miss of a few milliseconds in some runs.
- **x86_64:** `loop` and `recursion` meet it, and `lists` except in run L. `bulk-text` meets it on Intel and missed by 0.04 to 0.05 on AMD. `text` misses (1.84x to 1.86x on Intel), as does `oskac build` (1.14x to 1.16x). The site build is at 1.095x and 1.118x.
- **What is left is not only `memcpy`.** On `bulk-text` almost no copies remain (6,217), yet the AMD runners showed 1.15x. **Inference:** that is musl's `malloc` mapping and unmapping each 520 KB text, which is over the allocator's 1 KiB classes; this was not profiled on those runners. The same `mmap` and `munmap` churn is the kernel's 41% of the site's gap and 68% of the compile's in the profile above. Of the copies left, the shortest (the one-character `join` parts in the site build, the five pieces a line in `text`) can only be cut by not calling `memcpy` for them, which is the same as replacing it, and spec #521 rules that out.
- **Next steps, each its own ticket (at the time of writing; section 9's Decision below says which were taken):** keep requests over 1 KiB from reaching mallocng's `mmap` and `munmap` (an allocator change, so a security review), which the site build and the compile need; and decide whether `text` and the site's `join` of characters, where short copies alone still cost on Intel, are acceptable or need a different text representation, such as building an interpolated text in one piece or `String.chars` and `join` handing out slices of one text.

### Decision (2026-11-22)

Both Linux builds stay. [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md) stands. Tickets #524 to #526, which would retire the glibc build, are closed as not planned. The musl build is at spec #521's threshold on aarch64 and above it on x86_64 (1.07x to 1.16x on the site build, more on text-heavy work), so the installer keeps choosing the glibc build where it can run.

What remains:

- musl's `mmap` and `munmap` for requests over 1 KiB, which is most of the kernel's share of the x86_64 gap.
- Very short copies on Intel, where musl's `memcpy` costs most.
- Replacing `memcpy` is out of scope per spec #521. It needs its own decision.

### Tests on the musl build

At `9760b76e`, on this session's x86_64 VM, with the musl target: `cargo test --target x86_64-unknown-linux-musl -p oska-sys -p oska-run` (the `oska-run` tests, `short_copies.rs` among them, and the allocator's stress tests) and `cargo test --target x86_64-unknown-linux-musl -p oskac --test native_files --test native_parity --test native_default` (the differential suite, which runs this `oskac`'s interpreter against its native executables) passed, as they did at `2a14a06d`. CI's `musl` job runs the same commands; it passed at `6c47e617` and at `9760b76e`.

## 10. Release profile: fat LTO and one codegen unit (2026-11-22, spec #577, ticket #580)

The workspace now sets `lto = "fat"` and `codegen-units = 1` in `[profile.release]`. It was empty. This is a new run on the same kind of x86_64 KVM container as the memcpy note, but a separate run: the figures differ a little from that note (for example binary size, and build time of 24 s there against 20 s here). Use this table. The tree is `0a7a786`, before #570. Times are the best CPU time of 3 runs from `scripts/bench-native.sh 3`, with noise of about ±10%. Each row gives the value without the setting, then with it, for glibc and for musl.

| Measure | glibc before | glibc after | musl before | musl after |
| --- | ---: | ---: | ---: | ---: |
| `cargo build --release -p oskac`, clean | 20 s | 48 s | 25 s | 50 s |
| `oskac` binary size | 6.14 MiB | 4.45 MiB | 6.12 MiB | 4.58 MiB |
| Interpreter, `loop` | 677 ms | 446 ms | 593 ms | 452 ms |
| Interpreter, `text` | 102 ms | 95 ms | 241 ms | 232 ms |
| Interpreter, `lists` | 631 ms | 499 ms | 622 ms | 672 ms |
| Interpreter, `recursion` | 488 ms | 487 ms | 1192 ms | 1327 ms |
| Interpreter, `site` | 6063 ms | 5482 ms | 10688 ms | 10386 ms |
| Native `site` build | 236 ms | 257 ms | 236 ms | 242 ms |

What it shows:

- **Size and build time:** the binary is 27% smaller on glibc and 25% smaller on musl. A clean release build takes about 2.4 times (glibc) and 2 times (musl) as long. The release workflow's `build` job has `timeout-minutes: 20`. A clean x86_64 Linux build takes under a minute here. The aarch64, macOS and Windows jobs also get the setting and were not timed.
- **Speed:** LTO helps glibc more than musl. The glibc interpreter is 10% faster on the site build and 34% faster on `loop`. The musl site build is only 3% faster. musl `lists` (+8%) and `recursion` (+11%) are slightly slower, which is at or past the noise. The memcpy note measured a larger gain (22% on glibc) on a different run, so treat the gain as 10% or more on glibc, and small on musl.
- **The musl to glibc ratio gets worse, not better:** the interpreter on the site build was 1.76x before (10688 against 6063 ms) and is 1.89x after (10386 against 5482 ms). The 1.10x threshold from #521 therefore has to name the profile: compare musl and glibc built with the same settings. Both columns here were measured on one pre-#570 tree and ticket #579 repeats them after #570.
- **Native output:** the generated executables do not change. The native `site` build's small differences (236 against 257 ms) are noise, because LTO touches `oskac` itself, not the code it writes.

Not done: no `panic = "abort"` (ticket #583), no PGO, no `target-cpu` (see the memcpy note).

## 11. Re-measured on main with LTO and the short-copy fixes (2026-11-22, spec #577, ticket #579)

Spec #577 asks whether the musl build still misses spec #521's threshold (musl CPU time at most 1.10x glibc) once #570 (allocator, short copies) and the release profile of section 10 are both in. This section answers it, and says what the `memcpy` override of ticket #581 would add.

- **Tree:** `a2b86a7` (main after PR #584, which includes #570). Rust 1.100.0, fat LTO and one codegen unit for every build below. Same kind of x86_64 KVM container as before (Intel Xeon @ 2.10GHz, 4 CPUs, load about 0 at the start). `scripts/bench-native.sh 5`: best CPU time of 5 runs, noise about ±10%.
- **Override build:** the scratch research patch of [the memcpy note](2026-11-memcpy-replacement.md) (see its "How this was measured"), not code in the repo. It defines `memcpy`, `memmove`, `memset`, `memcmp`, `bcmp` and `memchr` for x86_64 musl, so it is more than the two functions of ticket #581. I did not build a version with only `memcpy` and `memmove`.

### Interpreter (`oskac run --interpret`), CPU time

| Workload | glibc | musl | musl / glibc | musl with the override | override / glibc |
| --- | ---: | ---: | ---: | ---: | ---: |
| loop | 569 ms | 506 ms | 0.89x | 537 ms | 0.94x |
| text | 36 ms | 69 ms | 1.92x | 39 ms | 1.08x |
| bulk-text | 31 ms | 33 ms | 1.06x | 34 ms | 1.10x |
| lists | 332 ms | 356 ms | 1.07x | 312 ms | 0.94x |
| recursion | 464 ms | 447 ms | 0.96x | 473 ms | 1.02x |
| site | 2815 ms | 3498 ms | **1.24x** | 3336 ms | **1.19x** |

The native executables take about the same time on both builds (the `Native` column of the same `bench-native.sh` runs; the `site` row is 286 ms on glibc and 279 ms on musl). The raw outputs are not committed.

### What this shows

- **The threshold still fails, on two rows.** The musl interpreter is 1.24x on the site build and 1.92x on `text`. The other four rows are at or under 1.10x. Compared with section 10 (before #570), the site ratio fell from 1.89x to 1.24x, `recursion` from 2.7x to 0.96x, and `text` from 2.4x to 1.92x. #570 (the allocator of section 8 and the short-copy fixes of section 9) did most of the work.
- **The override fixes `text` and does little else.** `text` goes from 1.92x to 1.08x, because the interpolation work is made of 1 to 5 byte copies. The site build gains 5% (3498 to 3336 ms) and stays at 1.19x. No row gets worse beyond the noise.
- **The rest of the site gap is not `memcpy`.** Section 9 put the kernel at about as much of the gap as `memcpy`: musl's `malloc` still maps and unmaps groups for requests over 1 KiB, which costs page faults. That is the next lever, and it is an allocator change, not a copy change. This section did not re-measure it.

### `mem*` calls on the musl build, by function and size (counting build, same tree)

| Workload | `memcpy` calls | of which 1 byte | `memcmp` and `bcmp` calls | `memmove` | `memset` | `memchr` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| site (interpreter) | 7,494,215 | 5,260,480 (70%) | 47,669,956 | 1,000 | 153 | 14 |
| `oskac build` of the site | 1,199,279 | 316,012 (26%) | 631,100 | 43,045 | 28,814 | 1 |

`memcpy` calls by size, same two runs (each class is a power of two bucket of the counting build; the last one holds everything above 2 KiB):

| Bytes | 0 | 1 | 2 | 3 to 4 | 5 to 8 | 9 to 16 | 17 to 32 | 33 to 64 | 65 to 128 | 129 to 256 | 257 to 512 | 513 to 1 K | 1 K to 2 K | over 2 K |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| site (interpreter) | 125,186 | 5,260,480 | 115,571 | 230,581 | 335,546 | 226,123 | 221,676 | 178,332 | 131,493 | 289,696 | 176,023 | 94,430 | 52,721 | 56,357 |
| `oskac build` | 349 | 316,012 | 16,486 | 346,301 | 87,673 | 146,073 | 63,159 | 50,683 | 13,981 | 107,857 | 32,087 | 5,617 | 4,372 | 8,629 |

`memcmp` and `bcmp` are counted together. The counting build used the same fat LTO profile as the other builds.

Section 9 counted 7,333,502 `memcpy` calls for the interpreted site build after its fixes, so the count has not moved (7,494,215 now, 2.2% more). 70% of the calls left are 1-byte copies, from `String.join` of one-character texts (the fix that review dropped, section 9). The `oskac build` row is about nine times section 9's 135,216; I counted `oskac build site/build.osk -o` with a counting build, section 9 used `callgrind`, and I did not find the reason, so do not compare the two. `memcmp` is the most called function (47.7 million calls), and the override replaces it too, without closing the site gap.

### Verdict for ticket #581

Do not build #581 now. It fixes the `text` row (1.92x to 1.08x), and the site build stays above 1.10x without it and with it. Two cheaper steps come first: stop the 1-byte `String.join` copies in a way that review accepts (a spec #521 decision), and keep requests over 1 KiB from reaching mallocng's `mmap` and `munmap` (the kernel share; section 9 notes that change needs a security review). Re-measure after each. If `text` is the only row left, then #581 is a good fit.

Not measured: aarch64 (ticket #582), a build with only `memcpy` and `memmove` replaced, a CPU without ERMS.

## 12. `panic = "abort"` (2026-11-22, spec #577, ticket #583)

A new `dist` profile (`inherits = "release"`, `panic = "abort"`) builds the shipped `oskac`: the release job, the musl job of CI, the benchmark workflow and `scripts/bench-native.sh` use it (`cargo build --profile dist`, binary in `target/dist/`). `cargo build --release` still unwinds. The `oskac` has a panic hook (`crates/oskac/src/crash.rs`) that prints `oskac: internal error: <message>`, a request to report it, removes the temporary files `oskac` made (the native executable of `oskac run` and its directory, which `Drop` removed before) and exits 101, the code an unwinding panic gave. Tree: main with sections 10 and 11. Same container as section 11, `scripts/bench-native.sh 5`, noise about ±10%. The "unwind" columns are section 11's figures (fat LTO, unwinding); the "abort" columns are new.

| Interpreter, CPU time | glibc unwind | glibc abort | change | musl unwind | musl abort | change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| loop | 569 ms | 515 ms | -9% | 506 ms | 493 ms | -3% |
| text | 36 ms | 36 ms | 0% | 69 ms | 62 ms | -10% |
| lists | 332 ms | 300 ms | -10% | 356 ms | 296 ms | -17% |
| recursion | 464 ms | 374 ms | -19% | 447 ms | 341 ms | -24% |
| site | 2815 ms | 2871 ms | +2% | 3498 ms | 3594 ms | +3% |

- **Speed:** 3 to 24% faster on loops, lists and recursion, no change on the site build (the +2% and +3% are inside the noise). The earlier test measured 6% on the site build with a different tree, and this run does not repeat it. `bulk-text` on glibc went from 31 to 14 ms, which I do not trust as an effect of this setting (the interpreter runs it in Rust code that the setting does not touch) and did not repeat.
- **The musl to glibc ratio on the site build stays at 1.25x** (3594 against 2871 ms), so this setting does not close the gap of section 11.
- **Size:** the glibc `oskac` is 4.08 MiB, against 4.45 MiB with LTO alone (-8%) and 6.14 MiB with neither (-34%).
- **Crash behaviour:** `oskac check --format=json p.osk > /dev/full`, a failed write to standard output, which is the one panic `oskac` has on purpose, ends with the new message and exit code 101 in the abort build, as in the test build (`crates/oskac/tests/crash_handler.rs`). Before, it printed Rust's default message with a stack trace note.
- **What `cargo test` does not cover:** the abort build itself. `cargo test` ignores the `panic` setting, so tests unwind and the hook ends the process with 101. The abort build was run by hand on the case above.
- **`oska-eval`:** it catches a task's panic (`catch_unwind`) to keep a batch going. It is built with the release profile, which still unwinds, so it keeps doing so (Adam chose a separate profile over accepting the loss).

## 13. Kernel-share baseline for spec #612 (2026-11-22, ticket #613)

Spec #612 asks whether keeping large allocations in the process, instead of mapping and unmapping them each time, closes the rest of the musl gap. This section records the numbers that change has to beat. It changes no code.

- **Tree:** `eabb226` (main after PR #607), the `dist` profile of section 12 for both builds (`cargo build --profile dist -p oskac`, with and without `--target x86_64-unknown-linux-musl`). Same kind of x86_64 KVM container as before (Intel Xeon @ 2.10GHz, 4 CPUs). Another agent's `cargo` build ran at the same time (load 3 to 4), so CPU times are noisy (about ±10% or more) and the counts are the figures to trust.
- **Method:** `strace -f -c` for the syscall counts, one run each. A small Python script ran each command 3 times and took the run with the least CPU time, with `getrusage` of the children for minor page faults and user and system CPU. Inputs as `scripts/bench-native.sh` prepares them (`site/authority.txt`, `site/api.json`). The script is not in the repo.

| Command | Build | `mmap` | `munmap` | `brk` | Minor page faults | User CPU | System CPU |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| interpreted site build | glibc | 20 | 9 | 52 | 9,074 | 2.56 s | 0.03 s |
| interpreted site build | musl | 21,811 | 21,730 | 4 | 211,827 | 2.77 s | 0.29 s |
| `oskac build` of the site | glibc | 16 | 4 | 54 | 4,480 | 0.02 s | 0.01 s |
| `oskac build` of the site | musl | 598 | 521 | 3 | 7,201 | 0.03 s | 0.01 s |

- **The kernel share is real for the interpreter.** musl makes about 1,100 times as many `mmap` calls as glibc (21,811 against 20) and takes 23 times as many page faults (211,827 against 9,074). Its system CPU is 0.29 s against 0.03 s. The CPU gap on the interpreted site build is 0.47 s (3.06 s against 2.59 s), and 0.26 s of it, 55%, is system time. The other 0.21 s is user time. That fits sections 9 and 11, which put the kernel at about half of the site gap.
- **The compile is a small case.** The `oskac build` of the site takes about 0.04 s on both builds here, with 598 `mmap` calls under musl against 16 under glibc. It will not move the ratio much. (The build ran with the build cache warm, so it is not the cold 79 to 108 ms of earlier sections.)
- **What the allocator change should do:** if the large-block reuse works, the musl `mmap` and `munmap` counts and the minor faults of the interpreted site build fall toward glibc's, and the system CPU with them. Ticket #615 checks that, and reverts the change if the site build gains under 5%.

## 14. Sizing `String.join` once does not remove copies (2026-11-22, ticket #616, not merged)

Ticket #616 of spec #612 asked `String.join` to size its result once and copy each part once, expecting fewer `memcpy` calls on the interpreted site build. The change was built (the interpreter's `string_join` reserves the total length and pushes each part) and its tests passed, including that a join-heavy program stops at the same place under `--max-memory`. It was measured with the counting build of section 11 (musl, `cargo build --release`, same tree, the change applied to it):

| Interpreted site build | `memcpy` calls | of which 1 byte |
| --- | ---: | ---: |
| before | 7,494,215 | 5,260,480 |
| after | 7,675,294 | 5,453,169 |

The count went up by 2.4%, not down. Two reasons, both in the code: the standard library's `join` already sizes its result once, so there was nothing to save there; and for a short separator it copies the separator with an unrolled, inlined path and no `memcpy` call, while `push_str` of the separator calls `memcpy` each time. Each part is one `memcpy` either way. The one-character parts come from `String.chars` in the Oska program being joined, so the only way to make fewer calls is to avoid copying a short piece, which spec #521 rules out.

The ticket's criterion (the count falls) is not met, so the change is not merged. The branch `ticket/616-join-sizing` stays local in the session. The one-byte copies are left for ticket #581 (the `memcpy` override), which is the change that makes each of them cheap.

## Open questions

- ~~The aarch64 profiles (section 5).~~ Measured on 2026-11-21: section 5.
- How much of the 592 ms in Oska's own code (cause 4) comes from cache and TLB misses. A machine with hardware counters (`perf stat -e cache-misses,dTLB-load-misses`) can answer it.
- ~~After fixes 1 to 6, does the musl build meet a threshold? The 2026-11-19 research note suggested at most 10% slower than glibc. Decide it before fix 7.~~ Spec #521 set the threshold at 1.10x. Measured on 2026-11-21: the musl build misses it (section 7).
- Where the last 290 to 350 ms of the aarch64 site build go with the allocator in place (section 8). ~~How much of the x86_64 gap left is `memcpy`.~~ Measured on 2026-11-21: section 9.
- How much of the x86_64 site and compile gaps musl's `mmap` and `munmap` of requests over 1 KiB cause, measured on GitHub's runners rather than inferred from this VM's profile (section 9).
- Run E's 5 ms musl `loop` figure (section 8): `scripts/bench-native.sh` does not check the exit status of its timed runs. Left open; the row is excluded from section 8's verdict because the wall time matched and the security review found no allocator failure.

## How this was measured

- Builds: `cargo build --release --locked -p oskac` and `cargo build --release --locked -p oskac --target x86_64-unknown-linux-musl` at `36167da`, Rust 1.100.0, with `CARGO_INCREMENTAL=0`.
- Tools: `perf` 6.8 from Ubuntu's `linux-tools-generic` (software CPU-clock events only; hardware events are not supported in this VM), `strace`, `valgrind` with `callgrind_annotate --threshold=100 --tree=caller`, and `objdump -d` on the binaries and on `memcpy.lo` from the toolchain's `aarch64-unknown-linux-musl` `libc.a`.
- The timing program for `memcpy` and allocation copies a slice of each size (with `black_box`, so it calls `memcpy`) and allocates and frees a `Vec<u64>` in a loop, first on the main thread alone, then after one thread has started and ended. It was built for both targets with the same toolchain. It is not in the repository.
- The commands for each run are those in section 5, with the x86_64 target.
- Related: [2026-11-oskac-without-glibc.md](2026-11-oskac-without-glibc.md) §4 (allocator performance), [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md), [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md), [scripts/bench-native.sh](../../scripts/bench-native.sh).
