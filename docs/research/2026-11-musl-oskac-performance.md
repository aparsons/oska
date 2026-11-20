# Why is the musl build of `oskac` slower than the glibc build, and what is the cheapest fix?

- **Date:** 2026-11-20
- **Scope:** Ticket #469 of spec #461 ([ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). On 2026-11-19 the musl build took 0.32 s to `oskac build` the site generator against 0.20 s for the glibc build, and its interpreter took 5.8 s against 3.8 s. Find where the time goes, on x86_64 and aarch64, and rank the fixes. No allocator and no `unsafe` code in this ticket.
- **Status:** x86_64 measured here. **aarch64 is pending**: this machine is x86_64 only, and no aarch64 number below is measured. Section 5 gives the exact commands for an aarch64 runner.
- **Method:** Built both release binaries of `oskac` at `36167da` with the pinned Rust 1.100.0: `cargo build --release -p oskac` and the same with `--target x86_64-unknown-linux-musl`. Ran `scripts/bench-native.sh` once with each. Timed the site generator with `perf stat`. Profiled with `perf record` (CPU-clock sampling, as this VM has no hardware counters), `strace -c`, and `valgrind --tool=callgrind`, which counts instructions and calls exactly. Disassembled the C library routines in the binaries. Wrote a small Rust program that times `memcpy` and allocation on each C library.
- **Machine:** Intel Xeon @ 2.10GHz, 4 CPUs, a Linux 6.18 VM shared with one other agent. CPU times (user plus system) are used, as other work stretches the wall clock but not CPU time much. Each figure is the best of 3 to 5 runs. The ratios held in every run; the absolute times move by about 5%.
- **Sources:** what this note measured, plus the musl release notes. The musl source site and its GitHub mirror were not reachable from this session, so each claim about how musl works is either read from the disassembly of the shipped `libc.a` or marked **Inference**.

## Short answer

The musl build is slower because of musl's C library routines, not because of Oska's code. Its instruction count is only 9% higher (63.3 billion against 58.0 billion for the interpreted site build), but it takes 70% more CPU time. On x86_64 the gap has four causes:

1. **musl's allocator (mallocng): about 44% of the interpreter's gap.** It runs 46% more instructions per allocation. It also takes a lock on every `malloc` and `free` once a second thread exists, and the interpreter always runs on a second thread (for its 256 MiB stack). The lock alone is 26% of the gap.
2. **musl giving memory back to the kernel and taking it again: about 20% of the interpreter's gap and 35% of the compile's.** The interpreted site build makes 81,309 `mmap` and 81,264 `munmap` calls and takes 345,726 page faults. The glibc build makes 17 and 6 calls and takes 4,129 faults.
3. **musl's x86_64 `memcpy`: about 28% of the interpreter's gap and 37% of the compile's.** It is a `rep movsq` with byte loops before and after it. Each call costs about 13 ns, even for 16 bytes, where glibc's costs 2 ns. The interpreter calls it 39 million times.
4. **The rest, about 16%,** is Oska's own Rust code running slower on the same instructions. **Inference:** fresh pages and a different heap layout cost cache and TLB misses. This VM cannot count them. musl's `memcmp` is faster here, which takes back about 8%.

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

The ratios match the 2026-11-19 figures (1.6x and 1.5x) closely enough; this machine and its load differ.

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

`valgrind --tool=callgrind` counts each instruction and call. Both builds run the same Rust code, so they make the same calls.

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

- In the interpreter, musl's `__libc_malloc_impl` and `__libc_free` take 7.07 and 6.93 billion instructions. All of glibc's allocator takes 12.35 billion. That is 46% more instructions for the same 52 million allocations.
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

Oska's own code takes 592 ms (15%) longer under musl for the same instructions. **Inference:** freshly mapped pages and a different heap layout cost cache and TLB misses. This VM reports hardware counters as not supported, so this cannot be proven here.

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

## 5. aarch64: pending

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

## Open questions

- The aarch64 profiles (section 5).
- How much of the 592 ms in Oska's own code (cause 4) comes from cache and TLB misses. A machine with hardware counters (`perf stat -e cache-misses,dTLB-load-misses`) can answer it.
- After fixes 1 to 6, does the musl build meet a threshold? The 2026-11-19 research note suggested at most 10% slower than glibc. Decide it before fix 7.

## How this was measured

- Builds: `cargo build --release --locked -p oskac` and `cargo build --release --locked -p oskac --target x86_64-unknown-linux-musl` at `36167da`, Rust 1.100.0, with `CARGO_INCREMENTAL=0`.
- Tools: `perf` 6.8 from Ubuntu's `linux-tools-generic` (software CPU-clock events only; hardware events are not supported in this VM), `strace`, `valgrind` with `callgrind_annotate --threshold=100 --tree=caller`, and `objdump -d` on the binaries and on `memcpy.lo` from the toolchain's `aarch64-unknown-linux-musl` `libc.a`.
- The timing program for `memcpy` and allocation copies a slice of each size (with `black_box`, so it calls `memcpy`) and allocates and frees a `Vec<u64>` in a loop, first on the main thread alone, then after one thread has started and ended. It was built for both targets with the same toolchain. It is not in the repository.
- The commands for each run are those in section 5, with the x86_64 target.
- Related: [2026-11-oskac-without-glibc.md](2026-11-oskac-without-glibc.md) §4 (allocator performance), [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md), [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md), [scripts/bench-native.sh](../../scripts/bench-native.sh).
