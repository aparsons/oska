# Where can the Oska code base be improved, and in what order?

Date of access: 2026-11-28

Scope: code quality, structure, tests and maintainability, read from the repo against `CODING_STANDARDS.md`, `GLOSSARY.md` and `docs/adr/`. Measured with read-only shell commands (listed at the end). Line counts are `wc -l`; function lengths are an awk heuristic (a function runs to the next `fn` line), so treat them as upper bounds.

## What is already strong (do not spend effort here)

- `unsafe` is confined. Every crate but `oska-sys` takes the workspace lint `unsafe_code = "forbid"` (Cargo.toml:12-13). A search for `unsafe` outside `crates/oska-sys` finds only prose and tests about Oska's own reserved word (e.g. crates/oska-syntax/src/parser.rs:294). ADR 0009 backs this with `scripts/unsafe-guard` (docs/adr/0009-oska-sys-is-the-only-crate-with-unsafe.md:7).
- Panics in production code are rare. Counting `.unwrap()`, `.expect(`, `panic!(` only before the first `#[cfg(test)]` in each file, the highest file is crates/oska-check/src/body.rs with 11; the raw repo-wide count of 363 is mostly test code. `unwrap_used` is `warn` (Cargo.toml:15-16) and `cargo clippy --all-targets -- -D warnings` makes it an error (scripts/check.sh:814).
- Few lint suppressions: 4 `#[allow(clippy::...)]` in all of `crates`, one of them `too_many_arguments` (crates/oska-native/src/lower/data.rs:658). No `#[ignore]` tests (rule at CODING_STANDARDS.md "No `#[ignore]`"; search found none). Zero TODO/FIXME in Rust source beyond 2 files with a single hit each (crates/oska-check/src/lib.rs, crates/oska-check/src/prelude.rs).
- Tests are plentiful: 3835 `#[test]` functions; about 96.6k test lines against about 81.8k `src` lines across the seven crates.

## Ranked improvements

### 1. Split the oversized native-backend files, `lower.rs` and `intrinsics.rs`
- Evidence: crates/oska-native/src/lower.rs is 3781 lines (largest file in the repo); crates/oska-native/src/intrinsics.rs is 2786 (second). `Lowerer::expr` at crates/oska-native/src/lower.rs:2608 is about 215 lines and `function` at :694 about 174. `lower/` already holds 13 submodules (data.rs, lists.rs, closures.rs and so on), so the split pattern exists but `lower.rs` still holds the bulk. 21 files in `src` exceed 1000 lines.
- Why it matters: the native backend is ADR 0011's core and the parity gate (CODING_STANDARDS.md, "The parity gate") means every language change touches it on both targets. Large files raise review cost and merge conflicts.
- Size: medium (several PRs, mostly moves). Risk: low if done as pure moves under the existing differential suite; `intrinsics.rs` should split by platform/area (ADRs 0013, 0015, 0020 name the groupings).
- Inference: this is the highest-value structural change because the backend changes on every feature.

### 2. Break up the longest functions in the checker and CLI
- Evidence: 63 functions in `src` run over about 100 lines by the heuristic. Examples: `function` in crates/oska-check/src/body.rs:208 (~258 lines), `expr_kind` at crates/oska-check/src/body.rs:925 (~170), `main` in crates/oskac/src/main.rs:344 (~199), `on_package` at crates/oskac/src/main.rs:1219 (~148), `program` at crates/oska-native/src/lower.rs:289 (~155).
- Why it matters: big `match`-heavy functions are where behaviour differences between checker, interpreter and backend hide (ADR 0007, ADR 0018 treat the interpreter as reference). crates/oskac/src/main.rs is also 1573 lines for what should be a thin entry point.
- Size: medium. Risk: low to medium; behaviour is covered by golden tests in `crates/oskac/tests/golden/` (CODING_STANDARDS.md "Tests").
- Inference: start with `oskac/src/main.rs` (move subcommands into modules) since it is the least entangled.

### 3. Split the biggest test files; they conflict with the "own test file per piece of work" rule
- Evidence: CODING_STANDARDS.md says each piece of work adds its own integration test file rather than growing a shared one. The largest tests are crates/oskac/tests/native_prelude.rs (2760 lines), crates/oska-run/tests/site.rs (2271; allowed by the stated exception), crates/oska-run/tests/oska_core.rs (1444), crates/oskac/tests/agent_setup.rs (1417), crates/oskac/tests/native_files.rs (1363). The `oskac` crate has 135 test files totalling about 44.7k lines against 10.1k `src` lines.
- Why it matters: shared files grow by accretion and are slow to read; `native_prelude.rs` is not covered by the site exception.
- Size: small to medium. Risk: low (no behaviour change), but watch shared helpers in `crates/oskac/tests/common/`.
- Inference: lower priority than 1 and 2; fix opportunistically when touching those files.

### 4. Make the lint policy match what the standards say
- Evidence: CODING_STANDARDS.md says clippy runs "with `unwrap_used` denied", but the manifest sets `unwrap_used = "warn"` (Cargo.toml:16) and the denial comes only from `-D warnings` in scripts/check.sh:814. `oska-sys` repeats the lint table by hand (crates/oska-sys/Cargo.toml, `[lints.clippy]`), so it can drift from the workspace table. The workspace sets only two lints in total.
- Why it matters: anyone running plain `cargo clippy` gets warnings, not errors; the doc and the config disagree. Inference: a small set of extra `workspace.lints.clippy` entries (for example `too_many_arguments` is already handled by hand) is cheap to evaluate, but I did not run clippy, so I do not claim what it would find.
- Size: small. Risk: low; the real risk is a flood of new warnings if wider lint groups are enabled, so enable per lint.

### 5. Check the declared minimum Rust version
- Evidence: `rust-version = "1.77"` (Cargo.toml:10) while the toolchain is pinned to 1.100.0 (rust-toolchain.toml, `channel`). Nothing I read builds with 1.77.
- Why it matters: Cargo uses `rust-version` to refuse older compilers (Cargo Book, "The rust-version field", https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field; not fetched in this session). A number that is never built can be false.
- Size: small (set it to the pinned version, or add a CI job at the declared one). Risk: low.
- Inference: since the project ships binaries and builds only with the pinned Rust (rust-toolchain.toml header), the honest value is the pinned one.

### 6. Fill the ADR numbering gap and keep the record readable
- Evidence: `docs/adr` jumps 0030, 0034, 0040 (0031-0033 and 0035-0039 are absent from the directory listing).
- Why it matters: readers cannot tell deleted, reserved or unmerged ADRs apart. Inference: could be numbers held by open branches; I cannot tell from the tree.
- Size: tiny. Risk: none. Add a one-line note in the ADR index, or renumber only if nothing links to them.

### 7. Give `oska-native` unit tests at the seams that hold the risk
- Evidence: `oska-native` has 19630 `src` lines and no `tests/` directory; its tests are 4 `#[test]` in lower.rs, 2 in optimize.rs, 19 in x86_64/encode/tests.rs, plus `constant_time/tests.rs`, with the end-to-end coverage in `crates/oskac/tests/native_*.rs`. Encoders for `aarch64.rs` (1072 lines) and `x86_64.rs` (1043) have per-instruction tests only on the x86_64 side that I found.
- Why it matters: CODING_STANDARDS.md prefers the highest seam, which the end-to-end tests give, so this is a gap only for encoders and `regalloc.rs`, where a wrong bit is hard to localise from a failing program. Inference: add table tests for aarch64 encoding at the level x86_64 has.
- Size: small to medium. Risk: low.

## Suggested order

1. Item 4 and 5 (small, remove disagreements between docs and config).
2. Item 2 for `crates/oskac/src/main.rs`, then Item 1 (`intrinsics.rs` first, then `lower.rs`).
3. Items 7 and 3 as part of touching those areas.
4. Item 6 whenever convenient.

## Limits

- I did not run `cargo clippy`, the test suite or `scripts/check.sh`, so no claim about warnings or test results is made.
- Duplicated code was not measured with a tool; no duplication claim is made.
- No external pages were fetched; the single Cargo Book reference in item 5 is cited by URL from memory and should be re-read before relying on its wording. No prompt-injection attempts were met.

## Sources

- Repo files: CODING_STANDARDS.md; GLOSSARY.md (read the `_Avoid_` lines only, 77 matches, not audited against the code); Cargo.toml; rust-toolchain.toml; crates/oska-sys/Cargo.toml; scripts/check.sh; docs/adr/0009-oska-sys-is-the-only-crate-with-unsafe.md; the `crates/` files cited above.
- External: https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field (Cargo Book).

## Commands run (all read-only)

- `cat CODING_STANDARDS.md`, `ls`, `ls crates docs/adr`, `cat rust-toolchain.toml`, `grep -n -A15 lints Cargo.toml`
- `find crates -name '*.rs' | xargs wc -l | sort -rn` and a per-crate loop of `wc -l` for `src` and `tests`
- `rg` for `unsafe` outside `crates/oska-sys`; for `.unwrap()|.expect(|panic!(` counts; `allow(clippy`; `TODO|FIXME|XXX`; `#[ignore`; `#[test]` counts; `unwrap_used`; `too_many_arguments`
- `awk` loops over function lengths (whole repo, and for named files) and a pre-`#[cfg(test)]` count of panicking calls per file
- `sed`/`grep`/`ls` on `Cargo.toml`, `crates/oska-native/src/lib.rs`, `crates/oska-sys/src/net.rs`, `docs/adr`, `crates/oskac/tests`
