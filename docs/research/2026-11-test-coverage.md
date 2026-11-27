# Could Oska tests automatically detect and report on test coverage?

Date of access: 2026-11-26

Short answer: yes, for both (a) Oska programs and (b) the `oskac` Rust code. (b) is cheap and uses off-the-shelf tools. (a) needs new work in the compiler. The interpreter is the cheapest place to start. No Oska coverage feature exists today: a grep of `docs/` (top level, `adr/`, `guide/`) and `scripts/` finds no coverage mention except an unrelated ADR 0024 line about differential-suite coverage.

## 1. How Oska tests run today (repo facts)

- `oskac test [path] [--format=json] [--integration] [--interpret] [--no-cache]` runs the `test` blocks of a source file (`/home/user/oska/crates/oskac/src/main.rs`, usage text and `test_file`).
- Default path on Linux and macOS: the program's tests compile into one native test executable via `oska_native::build_tests_hooked`. It runs once per test, one process per test, so a panic stops only its own test. Stdout carries only snapshot records and the panic report (`/home/user/oska/crates/oskac/src/native_test.rs`).
- `--interpret` uses the interpreter runner `oska_run::test` (`/home/user/oska/crates/oska-run/src/testing.rs`).
- Both paths produce the same `oska_run::TestReport`, which `finish_test` prints as text or JSON. A coverage section could therefore be added in one place (the report type and its printer).
- `oska-native` has its own IR, register allocator, and ELF, Mach-O and PE writers (`crates/oska-native/src/{ir,lower,regalloc,elf,macho,pe}.rs`). There is no LLVM, so LLVM source-based coverage cannot be reused for Oska programs.
- CI runs `scripts/check.sh`, whose `cargo test --all` step covers the Rust code (`/home/user/oska/scripts/check.sh`, around lines 562-584, also wrapped by `scripts/temp-leak/run.sh`).

## 2. How coverage works elsewhere (primary sources)

- Rust and LLVM: `-C instrument-coverage` injects `llvm.instrprof.increment` counters and embeds a coverage map in the binary. At exit the counters go to a `.profraw` file, whose path comes from `LLVM_PROFILE_FILE`. `llvm-profdata merge` and `llvm-cov report`/`show` turn them into reports. Metrics are function, instantiation, line, and region coverage. [rustc book](https://doc.rust-lang.org/rustc/instrument-coverage.html)
- The LLVM mapping format stores, per function, source regions (file, start and end line and column) that point at a counter. A counter is a profile counter index, an add/subtract expression of other counters, or zero. Expressions let uninstrumented regions such as an `else` block be derived. It also has branch regions and decision regions (for MC/DC). [LLVM coverage mapping format](https://llvm.org/docs/CoverageMappingFormat.html)
- Rust branch coverage is behind unstable `-Z coverage-options=branch|condition`. The default is block coverage. [Unstable Book](https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/coverage-options.html)
- `llvm-cov export` emits JSON or lcov trace format from the profile plus the binaries. [llvm-cov](https://llvm.org/docs/CommandGuide/llvm-cov.html)
- `cargo-llvm-cov` wraps this for `cargo test`. It outputs lcov, Cobertura, Codecov JSON, JSON, text and HTML, and has `--fail-under-lines`, `--fail-under-regions` and `--fail-under-functions`. Installing it from source needs rustc 1.87 or newer. [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
- Go cover (the old design) rewrites source before compilation: each executable section gets a counter assignment. Modes are `set`, `count` and `atomic`. The article reports about 3% run-time overhead on a typical test (Go 1.2 era). A read-only table maps counters back to source. [Go blog](https://go.dev/blog/cover)
- Go 1.20 and later: `go build -cover`, then run with `GOCOVERDIR`. This writes `covmeta` files (static, written once) and `covcounters` files (one per run). `go tool covdata` has `percent`, `textfmt` and `merge` modes. Data is lost on an unrecovered panic or fatal signal. [Go build-cover](https://go.dev/doc/build-cover)
- Go's design: coverable units are roughly basic blocks, with one counter array per function. The metadata blob and the counter data are written to separate files, linked by a hash. Function-level coverage is offered as a lower-overhead option because it needs one counter per function. The proposal gives no overhead numbers ("No data available yet"). [Go design 51430](https://go.googlesource.com/proposal/+/master/design/51430-revamp-code-coverage.md)
- GCOV: `.gcno` holds the basic-block graph and line mapping (from `-ftest-coverage`). `.gcda` holds arc transition counts (from `-fprofile-arcs`). GCC discourages reading these files directly and points to `gcov --json-format`. [GCC docs](https://gcc.gnu.org/onlinedocs/gcc/Gcov-Data-Files.html)
- Reporting in GitHub Actions: a step can append Markdown to `GITHUB_STEP_SUMMARY`. The limit is 1 MiB per step and 20 step summaries shown per job. `::error|warning|notice file=,line=,endLine=,title=::msg` creates annotations tied to a file and line. The page states no cap on annotation count. [GitHub workflow commands](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands)

## 3. (a) Coverage of Oska programs by Oska tests

What to measure. Statement or line coverage per function, with an optional count mode (hit counts). Branch coverage is a later step. This matches Go's `set` and `count` split and LLVM's block level.

Approach A: interpreter instrumentation (`oskac test --interpret --coverage`).
- Mechanism: the evaluator bumps a counter per AST node or span, keyed by `Span`. The AST and spans already exist (`Span` is used in `TestReport`). The static set of coverable units comes from the checked program, so never-run functions show as 0 hits (Go does this with its metadata blob).
- Cost: low. It is a bounded change in `oska-eval` and `oska-run`, plus a report type. No effect on codegen or the parity gate.
- Weak point: the interpreter is not what ships. A parity gate exists (`tests/parity_gate.rs`), but coverage gathered on the interpreter measures source reachability, not native behavior. That is fine for "which Oska lines do my tests execute".

Approach B: native codegen counters (`oskac test --coverage` on the default path).
- Mechanism: in `oska-native` lowering, emit a counter increment at the start of each basic block, using a per-function counter array in a data section (Go's layout). Keep a static metadata table (file, line range, function) in another section. The native test process runs once per test, so each process writes its own counters at exit to a file (the `GOCOVERDIR` pattern: separate meta and counter files). `oskac` then merges them in the parent process and joins them with the program's spans.
- Cost: medium to high. The work covers three backends (aarch64 and x86_64, for ELF, Mach-O and PE), a runtime exit hook, a file format, and an atomics decision for threaded programs. Counters add run-time overhead, and the build cache key must include the flag. Panics and `exit` paths need a write-out, and Go loses data on fatal signals, so the same care applies.
- Cheaper variant: because there is one process per test, the child could print the counters on stdout beside its snapshot records, and the parent merges them. That avoids files, but it changes the test protocol.

Report formats. Output a human summary in `TestReport` text and JSON, plus an LCOV file (`SF`, `FN`, `FNDA`, `DA`, `LF`, `LH` records) for external tools. LCOV is the widely supported interchange (llvm-cov and cargo-llvm-cov emit it), and Cobertura can be derived from it as cargo-llvm-cov does. I could not fetch the LCOV tracefile specification (the geninfo man page fetch failed), so the exact record list above is unverified here. Check it before building the writer.

Gating. A `--fail-under=N` flag, modelled on cargo-llvm-cov's `--fail-under-lines`.

Inference: start with Approach A. It is the smallest change, it covers every platform at once (the interpreter is portable, while native coverage needs work per backend), and it fixes the report and LCOV format. Treat Approach B as a follow-up only if users need coverage of native-only behavior, because B needs the same metadata table that A creates.

## 4. (b) Coverage of the `oskac` Rust code by its test suite in CI

Feasible with standard tools. `RUSTFLAGS="-C instrument-coverage"` plus `cargo test` yields profraw files, and `cargo-llvm-cov` handles merge and export (sources above). Points specific to this repo:
- `scripts/check.sh` runs the tests under `scripts/temp-leak/run.sh` and in shards (`OSKA_TEST_SHARD`). Profraw files from sharded jobs would need `llvm-profdata merge` across jobs, or per-shard lcov files merged afterwards.
- Many `oskac` integration tests spawn the `oskac` binary and generated native executables (`native_tests.rs`, `native_default.rs` and others). Coverage of the spawned `oskac` binary needs `LLVM_PROFILE_FILE` to be inherited. The `%p` and `%m` patterns avoid file collisions (rustc book). Generated Oska executables are not LLVM-instrumented and need no handling.
- `oska-sys` is the only crate with `unsafe` (ADR 0009). Coverage there is informational only.
- The toolchain is pinned in `rust-toolchain.toml` and `docs/agents/rust.md`. Check that the `llvm-tools` component can be added to the pin, and that `cargo-llvm-cov` builds on the pinned Rust (it needs 1.87 or newer to install from source; I did not check the pin).
- Report in CI: write the summary table to `$GITHUB_STEP_SUMMARY`, upload the lcov file as an artifact, and fail under a threshold. Per-line annotations of uncovered code on a PR need a diff filter, since the docs state no annotation cap but a whole-repo dump would be noise.
- Cost: low. One new CI job (not part of `check.sh`, to keep the pre-push hook fast), about a day of work plus longer CI time for an instrumented build. I did not measure the slowdown.

Inference: add a non-blocking CI job that publishes the summary first, and add a threshold only after a baseline is known.

## 5. Recommendation (Inference)

1. Do (b) first as an optional CI job, because it needs no compiler work.
2. Build (a) as interpreter-based line coverage with `--coverage`, a text and JSON summary in `TestReport`, an LCOV file, and `--fail-under`.
3. Defer native counters until there is demand. When built, they should reuse the metadata table from step 2.
4. Open questions: exact LCOV record spec (not read); how coverage should treat `#[integration]` tests and tests skipped by `--filter`; whether tests in packages and modules should be reported per module; and the real overhead of native counters, which no source quantifies for Oska (Go's own design cites none).

## Sources (14 read; 3 further fetches failed or redirected)

1. https://doc.rust-lang.org/rustc/instrument-coverage.html
2. https://llvm.org/docs/CoverageMappingFormat.html
3. https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/coverage-options.html
4. https://llvm.org/docs/CommandGuide/llvm-cov.html
5. https://github.com/taiki-e/cargo-llvm-cov
6. https://go.dev/blog/cover
7. https://go.dev/doc/build-cover
8. https://go.googlesource.com/proposal/+/master/design/51430-revamp-code-coverage.md
9. https://gcc.gnu.org/onlinedocs/gcc/Gcov-Data-Files.html
10. https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands
11. Repo: /home/user/oska/crates/oskac/src/main.rs and /home/user/oska/crates/oskac/src/native_test.rs
12. Repo: /home/user/oska/crates/oska-run/src/testing.rs
13. Repo: /home/user/oska/scripts/check.sh
14. Repo: /home/user/oska/docs/adr/0009-oska-sys-is-the-only-crate-with-unsafe.md (cited by name from the grep; not read in full)

Not obtained: the LCOV geninfo man page (GitHub raw and the sourceforge mirror failed or redirected away), and the Cobertura schema (not searched).
