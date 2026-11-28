# How can Oska reduce the amount of `unsafe` code and panics in its Rust source?

Date of access: 2026-11-28. Rust pinned by `rust-toolchain.toml`: 1.100.0 (`channel = "1.100.0"`). Workspace edition is 2021 (`Cargo.toml`, `[workspace.package]`).

Scope note: the Oska language has its own stops, the `P00xx` panic codes. They are a language feature and are not part of this question. `crates/oskac/src/crash.rs` (lines 1-4) says the same: a Rust panic inside `oskac` is a bug in `oskac`, never a program's `P00xx` stop. "Panic" below means a Rust panic in the compiler, runtime or tooling source.

Method: repo counts come from `grep -rE` over `crates/*/src` (commands given with each table). They are rough counts of text, not of semantics. I did not run `cargo clippy` or any build, so lint baselines (how many hits a new lint would give) are open.

## 1. Short answer

- **`unsafe` is already small and fenced.** Every crate forbids it except `oska-sys` (ADR 0009). A guard compiles the others with `-F unsafe_code` in CI. The gain left is inside `oska-sys` (about 109 blocks) and in making its safety rules stricter, not in the other six crates.
- **Panics are the larger open surface.** `unwrap()` is blocked by clippy, but about 78 `expect(` calls, 65 `unreachable!` and 15 or so `panic!` calls sit in non-test code, and no lint covers indexing, arithmetic or `expect`.
- **Ranked plan (section 6):** (1) count and ratchet, (2) turn on `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented` as warnings with a baseline, (3) tighten `oska-sys` lints, (4) Miri on `oska-sys`, (5) indexing next, (6) arithmetic last, (7) do not change `panic = "abort"`.

## 2. What the repo does today

### 2.1 Policy and enforcement

- Workspace lints: `[workspace.lints.rust] unsafe_code = "forbid"` and `[workspace.lints.clippy] unwrap_used = "warn"`. Source: `Cargo.toml`, `grep -n -A8 'workspace.lints' Cargo.toml`.
- Six crates opt in with `[lints] workspace = true`: `oska-check`, `oska-eval`, `oska-native`, `oska-run`, `oska-syntax`, `oskac`. Source: `grep -n -A2 '^\[lints' crates/*/Cargo.toml`.
- `oska-sys` does not take the workspace lints. It sets `unsafe_op_in_unsafe_fn = "deny"`, `clippy::unwrap_used = "warn"`, `undocumented_unsafe_blocks = "deny"`, `multiple_unsafe_ops_per_block = "deny"`. Source: `crates/oska-sys/Cargo.toml` lines 15-21.
- `scripts/check.sh` runs `cargo clippy --all-targets -- -D warnings` (line 814), so the `unwrap_used` warning fails CI. `CODING_STANDARDS.md` (lines 5-7) lists "clippy with `unwrap_used` denied" and "`unsafe_code` forbidden" as mechanical rules.
- Unsafe guard: step "unsafe only in oska-sys" runs `scripts/unsafe-guard/guard.sh` (`scripts/check.sh` lines 727-732). ADR 0009 says it builds every other crate with `-F unsafe_code` last on the rustc line, refuses `--cap-lints`, refuses checked-in cargo config and workflow rustc flags, and scans source the build skips (such as `#[cfg(windows)]`). `scripts/unsafe-guard/test.sh` tries each known bypass.
- Panic mode: `[profile.dist]` has `panic = "abort"`; `release` unwinds because `oska-eval` catches task panics (`Cargo.toml` comments on `[profile.dist]`). `oskac` installs a hook that removes temp files and exits 101 (`crates/oskac/src/crash.rs`, lines 1-20 and `set_hook` at line 87). `oska-eval` uses `catch_unwind` (`crates/oska-eval/src/validate.rs` line 121). Those are the only two sites (`grep -rn 'catch_unwind\|set_hook' crates/*/src`).
- Dev and test builds keep overflow checks on; the workspace comment says "Debug assertions and overflow checks stay on; release builds are unaffected" (`Cargo.toml`, `[profile.dev.package.*]` comment). Cargo documents `overflow-checks = false` for `release`, so the shipped `oskac` wraps on integer overflow rather than panicking ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). `grep -n overflow Cargo.toml` shows no override. Inference: arithmetic bugs in the shipped binary would give wrong values, not a panic; only debug and test builds surface them.
- No Miri, cargo-geiger or fuzz step exists in `scripts/` or `.github`: `grep -rli 'miri\|geiger\|fuzz\|proptest'` found only ADR 0012 and a skill file, neither of which runs them.

### 2.2 ADRs that touch the topic

- ADR 0009 (`docs/adr/0009-oska-sys-is-the-only-crate-with-unsafe.md`): `oska-sys` is the only crate with `unsafe`; safe public API; one unsafe operation per block with a `// SAFETY:` comment; amended 2026-11-21 (allocator) and 2026-11-27 (`memcpy`/`memmove`). Rejected alternative: allow `unsafe` in the runtime crate (line 24).
- ADR 0022 lines 27-29: rejects `no_std` oskac because of "weeks of new `unsafe` code". The workspace takes no external crates (ADR 0009 "adds no dependencies"; `CODING_STANDARDS.md` line 9).
- ADR 0013: system calls only in `std.rt` (Oska code), not Rust. ADR 0011: the runtime is Oska code, compiled into programs.
- ADR 0002 line 13 and ADR 0034 lines 13 and 38: these are about Oska-language panics (`unwrap`/`expect` are absent from the language). Not a rule for Rust source. Inference: ADR 0034's reasoning ("a missing value turns into a panic at a place the type does not show") is a good argument for the same stance in Rust, but it is not binding there.
- No ADR forbids, requires or sets a policy on Rust `expect`, `panic!`, indexing or arithmetic. `grep -il 'panic\|unsafe' docs/adr/*` lists ten ADRs; none states a Rust-source panic rule. `CODING_STANDARDS.md` has no panic rule beyond `unwrap_used`.

### 2.3 Unsafe by file (all `crates/*/src`)

Command: `grep -cE 'unsafe \{'`, `'unsafe fn'`, `'unsafe impl'` per file.

| File | `unsafe {` blocks | `unsafe fn` | `unsafe impl` | Lines |
|---|---|---|---|---|
| `oska-sys/src/mem.rs` | 47 | 11 | 0 | 405 |
| `oska-sys/src/heap.rs` | 38 | 11 | 2 | 979 |
| `oska-sys/src/unix.rs` | 13 | 0 | 0 | 713 |
| `oska-sys/src/windows.rs` | 11 | 0 | 0 | 1017 |
| other `oska-sys` files | 0 | 0 | 0 | |
| all six other crates | 0 | 0 | 0 | |
| Total | 109 | 22 | 2 | |

Also 6 lines with `unsafe extern` or `extern "C"` in `oska-sys`. Five `#[allow(` sites in `oska-sys` (`mem.rs` 345 and 362, `heap.rs` 401 and 403, `unix.rs` 216) and one in `oska-native` (`lower/data.rs` 658, `too_many_arguments`). Inference: by function the unsafe splits into three jobs: the allocator (`heap.rs`), `memcpy`/`memmove` replacements (`mem.rs`) and OS calls for confined file access (`unix.rs`, `windows.rs`). The first two are required by the musl performance goal (ADR 0009 amendments); the third exists because `std` has no `openat` (ADR 0009 "Why").

### 2.4 Panic-capable constructs by crate

Counts of text matches in `crates/<c>/src`. "Non-test" counts lines before the first `#[cfg(test)]` in each file (crude: files named `tests.rs` or in `tests/` dirs are only counted in the "all" columns, and code after a test module is missed). Commands: `grep -rE '\.expect\(|panic!\(|unreachable!\('`, and the awk-free loop in my notes using `head -n <first cfg(test) line>`.

| Crate | `.unwrap()` | `.expect(` all / non-test | `panic!(` all / non-test | `unreachable!(` all / non-test | `todo!`/`unimplemented!` | `assert*!(` all |
|---|---|---|---|---|---|---|
| oska-check | 0 | 48 / 39 | 3 / 3 | 4 / 4 | 0 | 71 |
| oska-eval | 0 | 28 / 0 | 0 / 0 | 0 / 0 | 0 | 66 |
| oska-native | 0 | 31 / 8 | 15 / 5 | 1 / 1 | 0 | 116 |
| oska-run | 0 | 35 / 25 | 2 / 1 | 65 / 65 | 0 | 70 |
| oska-syntax | 2 | 11 / 0 | 6 / 0 | 2 / 2 | 0 | 161 |
| oska-sys | 0 | 37 / 0 | 2 / 2 | 0 / 0 | 0 | 50 |
| oskac | 0 | 70 / 9 | 1 / 1 | 0 / 0 | 0 | 170 |

Caveats:
- Some `.expect(` hits in `oska-check` are a method named `expect` on the checker (`self.expect(&ty, expected, span)` in `crates/oska-check/src/body.rs` lines 818, 979), not `Option::expect`. Real `Option::expect` there includes `body.rs` lines 1055 and 1413 ("the block's own frame"). So the 39 is an upper bound.
- The two `.unwrap()` hits in `oska-syntax` pass clippy, so they are probably `unwrap_or*` false matches or in a doc/comment; not checked.
- 65 of `oska-run`'s `unreachable!` are the same pattern: the checker has already ruled a case out, for example `crates/oska-run/src/interp.rs` lines 552, 587, 681, 737 ("the checker keeps `break` and `continue` inside loops"). Inference: these are invariants between two crates, and the right fix is to encode them in types (see 5.5), not to delete them.
- `oska-native` `panic!` hits are mostly in compiler tables ("`std.rt` has no `...`", `crates/oska-native/src/intrinsics.rs` 1718, 1740, 1822) or test helpers.

### 2.5 Indexing, casts, arithmetic

- Indexing: a crude regex for `x[...]` (`[a-z_\)]\[[^]]*\]`, non-test lines) gives oska-run 291, oska-check 156, oska-native 84, oskac 60, oska-sys 43, oska-syntax 32, oska-eval 23. It also matches types such as `Vec<[u8; 4]>` and attributes, so treat it as an order of magnitude. Inference: indexing is the biggest unlinted panic source by count.
- `as` casts: `grep -rnE 'as (u8|u16|u32|i32|i64|usize)\b' crates/*/src` gives 303 lines. `as` truncates silently; it does not panic.
- Explicit checked, wrapping or saturating arithmetic: 248 lines (`grep -rnE 'wrapping_|checked_|saturating_'`). Inference: the code already uses explicit arithmetic in places, so `arithmetic_side_effects` has a partial base to build on, but I did not measure plain `+ - *`.

## 3. What the ecosystem offers

### 3.1 rustc lints

- `unsafe_code` is allow by default and "is intended to restrict the usage of `unsafe` blocks and other constructs (including ... `no_mangle`, `link_section` and `export_name` attributes)" ([rustc lint listing, allowed-by-default](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html)). Oska already sets it to `forbid`.
- `forbid` is `deny` that "also forbids changing the lint level afterwards"; an inner `#[allow]` is an error ([Reference, diagnostics attributes](https://doc.rust-lang.org/reference/attributes/diagnostics.html)). That is why the workspace uses `forbid`, not `deny`.
- `unsafe_op_in_unsafe_fn`: "allow by default on editions up to 2021, from 2024 it is warn by default" ([rustc lint listing](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html)). Oska is edition 2021 and sets it to `deny` in `oska-sys` only, which is correct there.
- `#[expect(lint)]` suppresses a lint and emits `unfulfilled_lint_expectations` if the lint no longer fires ([Reference, diagnostics attributes](https://doc.rust-lang.org/reference/attributes/diagnostics.html)). Inference: for ratchets in source, `#[expect(clippy::indexing_slicing, reason = "...")]` is better than `#[allow]` because it fails when the code is fixed and the exception goes stale. The `reason` field is not verified here.
- Cargo's `[lints]` and `[workspace.lints]` set levels per package; `lints.workspace = true` inherits. Cargo applies them to the current package only, not dependencies ([Cargo manifest, `[lints]`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)). The `[workspace.lints]` page itself I did not read; Oska's use of it is working evidence.

### 3.2 Clippy lints

- Clippy's restriction group is meant to be cherry-picked: "It is **not** recommended to enable the whole group, but rather cherry-pick lints that are useful" ([Clippy lints](https://doc.rust-lang.org/clippy/lints.html)).
- `arithmetic_side_effects`: "Checks any kind of arithmetic operation of any type." Group restriction, allow by default ([Clippy lint index](https://clippy.rust-lang.org/stable/index.html)). Option `arithmetic-side-effects-allowed` suppresses named types, default `[]` ([Clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)).
- Test relaxations in `clippy.toml`: `allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-indexing-slicing-in-tests`, `allow-panic-in-tests`, all default `false` ([Clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)). Inference: this matters because Oska runs clippy with `--all-targets`; with defaults a new `expect_used` would also hit about 150 test-side `expect` calls. Repo check: `ls /home/user/oska/clippy.toml` finds none (`ls` at repo root shows no such file).
- `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo`, `unimplemented`, `missing_panics_doc`, `panic_in_result_fn`, `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`: I could not read their individual descriptions from the lint index (the fetch returned only `arithmetic_side_effects`). Their names and the idea that they live in the restriction/pedantic groups are from training knowledge and are unverified here. The repo does use `unwrap_used`, `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` successfully (section 2.1). Open item: confirm the other names and groups on the Clippy index before adding them to `Cargo.toml`; a wrong name only produces an `unknown_lints` warning, which `-D warnings` turns into a CI failure, so the mistake is loud.

### 3.3 Isolating unsafe behind safe APIs

- The Rustonomicon: "all the unsafe parts are kept exclusively behind the `unsafe` boundary" and "No matter what, Safe Rust clients can't cause Undefined Behavior" ([Nomicon, How Safe and Unsafe Interact](https://doc.rust-lang.org/nomicon/safe-unsafe-meaning.html)). The same page says unsafe code "cannot trust Safe Rust chosen or supplied by its clients", citing `BTreeMap` and non-total `Ord`.
- Inference: ADR 0009 already follows this: one internal crate, a safe public API, SAFETY comments. The remaining work is to shrink what is behind the boundary and to test that the boundary holds (Miri, 3.4).

### 3.4 Miri

- Miri detects out-of-bounds access, use-after-free, uninitialised reads, misaligned access, invalid values and data races; run with `rustup +nightly component add miri` then `cargo miri test`. It "has no access to most platform-specific APIs or FFI", "networking is not supported", Windows is less well supported, and it "does not catch every violation" ([Miri README](https://github.com/rust-lang/miri)).
- Inference: `oska-sys` calls `openat`, `NtCreateFile` and installs a `memcpy` symbol, so most of `unix.rs`, `windows.rs` and the `memcpy`/`memmove` symbols cannot run under Miri. The allocator logic in `heap.rs` (38 blocks) and the pure-pointer parts of `mem.rs` (47 blocks) probably can, if their OS calls (for example `mmap`) can be stubbed or are supported. I did not check `heap.rs` for which calls it makes. Miri needs nightly; the repo pins stable 1.100.0, so a Miri job would be a separate, non-gating or separately pinned step. Whether that fits `docs/agents/rust.md` is open (I did not read it).

### 3.5 cargo-geiger

- "Detects usage of unsafe Rust in a Rust crate and its dependencies" and prints statistics ([cargo-geiger README](https://github.com/geiger-rs/cargo-geiger)). The README fetch did not state maintenance status or confirm a forbid-only flag.
- Inference: Oska has no dependencies and an exact guard already; geiger adds little. A 10-line grep count of `unsafe` tokens in `oska-sys` (section 5.1) gives the same number without installing external code. The workspace "takes no crate from outside the repo" (`CODING_STANDARDS.md` line 9) refers to dependencies, but installing a tool in CI would also need the pinned-action rules; I did not look at how CI installs tools.

### 3.6 `panic = "abort"` and `catch_unwind`

- Cargo: `dev` and `release` default to `unwind`; "Tests, benchmarks, build scripts, and proc macros ignore the `panic` setting" ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). That matches the `Cargo.toml` comment "`cargo test` ignores `panic`".
- `catch_unwind` "might not catch all Rust panics"; it only catches unwinding panics, and "is not recommended ... for a general try/catch mechanism. The `Result` type is more appropriate" ([std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)).
- Inference: `oska-eval` relies on unwinding (it must stay in `release`, as `Cargo.toml` says). The `dist` profile's abort gave 3 to 24% speed on some workloads (`Cargo.toml` comment). Neither setting reduces the number of panic sites; abort only changes what a panic does. Do not use either as a way to "reduce panics".

### 3.7 Result-based error handling

- The std guidance is `Result` for failures that "can fail on a regular basis" (same `catch_unwind` page).
- Repo evidence it already works: `crashes.rs` documents that the package writer's steps each "return an `io::Result`", so nothing there can panic (`crates/oskac/src/crash.rs` lines 11-14).

## 4. Fit with Oska's own rules

- The workspace-forbids-unsafe design and the guard are binding (ADR 0009). A plan must not weaken `forbid` or let a second crate hold `unsafe`. Moving code out of `oska-sys` into safe Rust is always allowed.
- "The workspace adds no external crates" (`CODING_STANDARDS.md` line 9; ADR 0009). So `bytemuck`, `libc`, `rustix`, `zerocopy`, `nix` and similar crates that would remove unsafe are ruled out. ADR 0009 rejects `libc` and `rustix` explicitly (line 18, "ruled out").
- A new Rust-source panic rule belongs in `CODING_STANDARDS.md` under mechanical rules, with the check in `scripts/check.sh`; and an ADR if it constrains design, because ADR 0009 is the precedent for the unsafe rule. Process for ADRs: `docs/agents/domain.md` (not read).
- Parity gate (ADR 0014, `CODING_STANDARDS.md` lines 13-14): not touched by any step below, since they change Rust source only.

## 5. Where the safe gains are

### 5.1 `oska-sys` (109 blocks)

- Count is the first ratchet: `grep -rcE 'unsafe [{f]|unsafe impl' crates/oska-sys/src` summed. A new `unsafe` in `oska-sys` should need a justified bump.
- `heap.rs` and `mem.rs` hold 85 of 109 blocks. These are musl-only and x86_64-only in parts (ADR 0009 amendments). Open question: how much of `mem.rs` can be written with `core::ptr::copy`/`copy_nonoverlapping` safe wrappers; a `memcpy` that calls `copy_nonoverlapping` is a clash risk the ADR already records ("a likely link clash with musl's `memcpy`", ADR 0009 line 42). Not resolved here.
- `unix.rs` (13) and `windows.rs` (11) are small and bounded; their safe wrappers are the public API already.
- Cheap hardening lints for `oska-sys` that the repo already half uses: add `missing_docs`-style `# Safety` doc lint (`clippy::missing_safety_doc`, name unverified, see 3.2) for the 22 `unsafe fn`s, and `unsafe_op_in_unsafe_fn` is already `deny`.

### 5.2 `expect`, `panic!`, `todo!`, `unimplemented!`

- Roughly 78 non-test `expect` calls (39 + 8 + 25 + 9 + 0 + 0 + 0 = 81 counting `oska-check` upper bound; section 2.4) and about 12 non-test `panic!` calls. Each is either a real invariant (keep, document with `#[expect(..., reason)]`) or a missing `Result`. Inference: the 0 `todo!`/`unimplemented!` and 0 `.unwrap()` suggest the team already acts on this, so a ratchet will be cheap to introduce.

### 5.3 Indexing

- About 700 non-test indexing-looking sites. Turning on `indexing_slicing` for everything would be a very large diff. Inference: apply it per crate in order of risk, starting with `oska-syntax` (32) and `oska-sys` (43), because the parser reads untrusted source text, and leave `oska-run` (291) to last. This is a judgement; I measured counts, not exposure to untrusted input. Using `.get()` / iterators / `split_first` is the safe form.

### 5.4 Arithmetic

- Release builds wrap silently (3.1 repo bullet). Inference: this is the opposite of a panic problem. `arithmetic_side_effects` would force every `+` onto `checked_*` or `wrapping_*` and add many sites. Value is lowest for a compiler that already tests with overflow checks on; it is highest in `oska-sys` (sizes, alignment, `mem.rs` and `heap.rs` pointer maths) where a wrong size is memory-unsafe. So: enable it in `oska-sys` first, not the workspace.

### 5.5 `unreachable!` in `oska-run` (65)

- These record "the checker proved X". Inference: removing them means making the checker's output a type the interpreter cannot get wrong, for example splitting `ExprKind` into a checked tree. That is a design change (see `codebase-design` skill) and costly; leave it until after the lint ratchet, and document each one rather than delete. Prefer `debug_assert!` plus a `Result`/diagnostic for cases with user-reachable paths.

## 6. Ranked plan

1. **Count ratchet, no source change (hours).** Add a small script `scripts/panic-ratchet/` and a step in `scripts/check.sh`, in the style of the existing `unsafe-guard` and `intrinsic-guard`, that counts per crate in non-test code: `unsafe` tokens, `.expect(`, `panic!(`, `unreachable!(`, `todo!`, `unimplemented!`, and `#[allow(`/`#[expect(` of those lints; compares them to a checked-in `scripts/panic-ratchet/baseline.txt`; fails if any count rises; and fails with a message to lower the baseline when a count falls (so gains are locked in). Inference: grep counting is crude (section 2.4 shows false matches), so this is a stopgap for step 2, not the end state. It would also need to ignore `tests/` and `#[cfg(test)]` the same way the numbers above do, or use clippy output.
2. **Clippy lints with a baseline (days).** Add `clippy::expect_used`, `clippy::panic`, `clippy::unreachable`, `clippy::todo`, `clippy::unimplemented` at `warn` in `[workspace.lints.clippy]` (the same form as `unwrap_used`), add `clippy.toml` with `allow-expect-in-tests = true` and `allow-panic-in-tests = true`, and for existing legitimate sites use `#[expect(clippy::..., reason = "...")]`. `-D warnings` in `scripts/check.sh` line 814 then gates it for free, with no new script, and the ratchet is "no new un-annotated sites". Do `oska-syntax`, `oska-sys`, `oska-eval` (already at 0 non-test) first; then `oska-native`, `oskac`, `oska-check`; `oska-run` last because of its 65 `unreachable!`. Confirm each lint name against the Clippy index first (3.2). Record the rule in `CODING_STANDARDS.md` and add an ADR if the team wants it as policy.
3. **Strengthen `oska-sys` lints (hours).** Add `clippy::arithmetic_side_effects` and `clippy::indexing_slicing` to `crates/oska-sys/Cargo.toml` only, with `expect` annotations where needed. Optionally cap the unsafe-token count there with the step 1 script (a number that can only go down). Document in ADR 0009's rules section if adopted.
4. **Miri on a subset of `oska-sys` (days, uncertain).** Run `cargo +nightly miri test -p oska-sys` on Linux for tests that need no FFI, as a non-blocking scheduled job first (nightly is not the pinned toolchain; see 3.4). Value: it checks the 47 + 38 blocks in `mem.rs` and `heap.rs` for out-of-bounds and misalignment. Risk: allocator and `#[global_allocator]` code may not run under Miri. Open: needs a spike to see how much of `oska-sys` tests under Miri.
5. **Indexing ratchet per crate (weeks).** Enable `indexing_slicing` crate by crate starting with `oska-syntax`, converting to `get`/iterators or annotating with `#[expect]` plus a reason.
6. **Typed invariants for `oska-run`'s `unreachable!` (weeks, design).** Only if step 2 shows the count is a maintenance cost. Needs a design decision.
7. **Leave alone:** `panic = "abort"` in `dist` (speed measured; no panic count change), `catch_unwind` in `oska-eval` (needed), `forbid(unsafe_code)` and the guard, and the no-external-crates rule. Do not adopt `cargo-geiger`: the guard and a grep count already answer the same question without new tooling.

CI gating summary: steps 1 and 2 reuse `scripts/check.sh` (the whole CI test job per `CLAUDE.md`), so no new workflow, no new third-party action and no pinned-SHA change. Step 4 would need a workflow job, with `timeout-minutes` and an official toolchain action per `CODING_STANDARDS.md` line 10-11.

## 7. Open questions

- Baseline counts of each proposed Clippy lint: not measured (no build run). The grep tables are only a proxy.
- Exact Clippy names, groups and default levels of `expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo`, `unimplemented`, `missing_safety_doc`: not confirmed from the Clippy index in this session.
- Whether Miri can run any useful part of `oska-sys` (allocator, `memcpy`): untested.
- How `docs/agents/rust.md` and the CI action allow-list treat a nightly toolchain job: not read.
- Whether `oska-syntax`'s two `.unwrap()` matches are real: not inspected (clippy passes, so probably not `Option::unwrap`).
- How many panic sites are reachable from untrusted input (source text, package files, network): not analysed. That decides the real ordering in steps 5 and 6.

## Sources

First-party (repo, read 2026-11-28):
- `/home/user/oska/Cargo.toml` (workspace lints, profiles)
- `/home/user/oska/rust-toolchain.toml` (1.100.0)
- `/home/user/oska/CODING_STANDARDS.md` (lines 1-13)
- `/home/user/oska/CLAUDE.md`
- `/home/user/oska/crates/*/Cargo.toml` (`[lints]` tables)
- `/home/user/oska/crates/oska-sys/Cargo.toml` and `src/*.rs`
- `/home/user/oska/crates/oskac/src/crash.rs`
- `/home/user/oska/crates/oska-eval/src/validate.rs`
- `/home/user/oska/crates/oska-run/src/interp.rs`
- `/home/user/oska/crates/oska-check/src/body.rs`
- `/home/user/oska/crates/oska-native/src/intrinsics.rs`
- `/home/user/oska/scripts/check.sh` (lines 727-732, 813-814)
- `/home/user/oska/scripts/unsafe-guard/guard.sh`
- ADRs 0002, 0009, 0011, 0013, 0022, 0034 under `/home/user/oska/docs/adr/`

External (accessed 2026-11-28; 11 fetches):
- https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html
- https://doc.rust-lang.org/reference/attributes/diagnostics.html
- https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section
- https://doc.rust-lang.org/cargo/reference/profiles.html
- https://doc.rust-lang.org/clippy/lints.html
- https://doc.rust-lang.org/clippy/lint_configuration.html
- https://clippy.rust-lang.org/stable/index.html
- https://doc.rust-lang.org/nomicon/safe-unsafe-meaning.html
- https://doc.rust-lang.org/std/panic/fn.catch_unwind.html
- https://github.com/rust-lang/miri
- https://github.com/geiger-rs/cargo-geiger

Version note: the external pages are the current stable docs, not a copy pinned to 1.100.0; I did not check that each statement is unchanged from 1.100.0.
