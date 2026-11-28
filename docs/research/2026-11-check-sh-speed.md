# Why is scripts/check.sh slow, and how can it be sped up?

Date of access: 2026-11-27

**Status after this note was written.** Every `scripts/check.sh` and `ci.yml` line number below refers to `main` at commit 63c8432, before PR #877. That PR, from spec #810, built fix 5 (a per-step timer and a slowest-steps summary), fix 6 (the cheap static checks now run before `cargo test`) and the first half of fix 3 (the `scripts` group runs its safe suites concurrently; `OSKA_CHECK_JOBS` sets the number). On 4 cores `--group scripts` went from 233 s to 70 s. The statements "no per-step timer" and "no `OSKA_*` timing variable" were true at 63c8432 and are now out of date. Fixes 1 and 2 (the macOS arm64 shard and `rest-1`) and fix 4 stay open in spec #711.

Related ground (CI time, flaky tests, release, process) is in [2026-11-bottlenecks-dev-test-release.md](2026-11-bottlenecks-dev-test-release.md); this note does not repeat it.

## Answer in brief

Inference: `check.sh` is slow because it is one serial list of 43 steps (`scripts/check.sh --list`) and its dominant step, `cargo test`, runs about 200 test binaries one after another ([bottlenecks note, "The test run"](2026-11-bottlenecks-dev-test-release.md)). CI has already split the script into 13 parallel `test` jobs (`.github/workflows/ci.yml:78-90`). Run to run, the slowest Linux shard (`rest-1`, 568 s) now sets the Linux time. The whole workflow is held up by the single-shard macOS arm64 job (1007 s). Locally there is no step timer, and the full run is serial.

## Method and limits

- Steps listed with `scripts/check.sh --list` (43 steps). The script has no per-step timer or `OSKA_*` timing variable (checked the header and top of `scripts/check.sh`; only `OSKA_BUILD_SLOTS` and `OSKA_TEST_SHARD` exist). I timestamped each `== NAME` header line of `--group lint` and `--group scripts` runs.
- I did not run the full `cargo test` suite or `--group formal`, so those rows use CI numbers.
- Local numbers are from a 4-core cloud sandbox (`nproc` = 4) with a partly warm `target`. GitHub's public-repo `ubuntu-latest` and `ubuntu-24.04-arm` runners are also 4 CPUs / 16 GB ([GitHub Docs: hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)), but disk, CPU model and cache state differ, so treat local seconds as a rough guide. The sandbox lacked `pwsh` and the Windows target, so those steps skipped.
- CI numbers: green run 45918508128 of `ci.yml` on main (push, 2026-11-27 00:44 to 01:04 UTC), job and step timestamps from the GitHub Actions REST API (list jobs for a workflow run). One run, no variance estimate.

## Where the time goes

### CI jobs, run 45918508128 (wall seconds)

| Job (what check.sh part it runs) | Total s | `check.sh` step s | Cache restore s |
| --- | --- | --- | --- |
| macos arm64, `all` shard (`ci.yml:272`) | 1007 | not split out | n/a |
| musl rest | 769 | not split out | n/a |
| Linux x86_64 tests rest-1 | 568 | 470 | 41 |
| Linux arm64 tests rest-1 | 547 | 428 | 56 |
| Linux arm64 tests native-2 | 444 | 389 | 49 |
| Linux x86_64 tests rest-2 | 436 | 336 | 73 |
| Linux x86_64 tests native-1 / native-2 | 391 / 394 | 297 / 300 | 79 / 79 |
| Linux x86_64 tests site | 293 | 201 | 77 |
| `scripts` group | 304 | 294 | none |
| `formal` group | 215 | 199 | none |
| `lint` group | 96 | 65 | none |

Source for every row: the job list of run 45918508128 above. Whole workflow: 1185 s (created 00:44:40, updated 01:04:25, same run). Shares for the Linux x86_64 `rest-1` job: check.sh 83 percent (470/568), cache restore 7 percent, cache save about 7 percent (40 s), rest setup (same job step list).

### Local timings of the cheap groups (sandbox, seconds)

`--group lint`, about 42 s in total, of which clippy about 15 s, `unsafe only in oska-sys` 15 s, SPDX 4 s, fmt 3 s; the other steps are 0 to 2 s (local run, `/tmp` log, not committed). In CI this group takes 65 s because it builds from scratch (`ci.yml:114-122`).

`--group scripts`, about 253 s in total (CI: 294 s). Share is of the 253 s.

| Step | s | Share |
| --- | --- | --- |
| installer tests (`scripts/install/test.sh`) | 59 | 23% |
| release script tests (`scripts/release/test.sh`) | 50 | 20% |
| eval task set (builds with cargo) | 32 | 13% |
| eval batch driver tests | 28 | 11% |
| check step registry tests | 20 | 8% |
| docs-test list tests | 16 | 6% |
| release image push tests | 12 | 5% |
| pass stamp tests | 10 | 4% |
| test shard tests | 8 | 3% |
| the other 13 suites combined | about 18 | 7% |

Source: local timestamped run of `scripts/check.sh --group scripts`; step names from the `if step` blocks of `scripts/check.sh`.

### The cargo test step

Not re-timed here. In an earlier single-job `check.sh` run, `cargo test` was about 9 m of about 17 m, scripts about 2 m, formal 3 m 12 s ([bottlenecks note, phase table](2026-11-bottlenecks-dev-test-release.md)). The suite has grown since, hence the shards.

## Why it is slow

1. **Serial by construction.** Steps are `if step ...; then ... fi` blocks run in order and stop at the first failure (`scripts/check.sh:7`, header comment; the step blocks start at `scripts/check.sh:499`). A plain `scripts/check.sh` is therefore the sum of all groups: from the tables, about 42 + 253 + 199 + the cargo test step (hundreds of seconds, more than 800 s for Linux rest-1 and rest-2 together given 470 s + 336 s above). Inference: a local full run takes well over 15 minutes, which matches the 16 m 45 s in the earlier note.
2. **Cargo runs test binaries serially.** [`cargo test` runs test binaries one after another](https://doc.rust-lang.org/cargo/commands/cargo-test.html); the Linux `rest` set is "some 14 minutes of test binaries, one after another" (`scripts/test-shard.sh:56`).
3. **CI shards are unbalanced.** `rest-1` pins the heaviest binaries (`scripts/test-shard.sh:57-62`) and also saves the cache (`ci.yml:176-187`): 470 s of check.sh against 201 to 336 s for the other Linux x86_64 shards.
4. **macOS arm64 is one shard.** `all` "runs the whole set in one shard (the macOS arm64 job)" (`scripts/test-shard.sh:27`; `ci.yml:272`). It is the slowest job at 1007 s, so it, not check.sh on Linux, sets the workflow time.
5. **Each shard pays a restore and a rebuild.** Cache restore costs 41 to 79 s (up to 25 percent of a shard) ([`ci.yml:123-131`](../../.github/workflows/ci.yml)); each shard builds its own binaries ("trades runner minutes for wall time", `ci.yml:197-202`).
6. **The scripts group is a long tail of many suites.** Four suites make up 67 percent of it (table above), and it does not share a build with other jobs.
7. **Debug info is on in CI.** "[unoptimized + debuginfo]" in the earlier log; [`dev` defaults to `debug = true`](https://doc.rust-lang.org/cargo/reference/profiles.html) (see the [bottlenecks note](2026-11-bottlenecks-dev-test-release.md)).

## Ranked fixes

| # | Fix | Estimated gain | Risk |
| --- | --- | --- | --- |
| 1 | Split the macOS arm64 `all` shard into the same shards the Intel job uses (native-1, native-2, rest) (`ci.yml:272`, `test-shard.sh:27`). Inference: the Intel shards take 588 to 670 s versus 1007 s (run 45918508128 job list), so the workflow could drop from about 1185 s to about 800 s, the next critical job being musl rest (769 s) or Linux rest-1 (568 s). | up to about 5 to 6 min of wall time per CI run | More macOS runner minutes, which are costlier; arm64 may need its own balance check |
| 2 | Rebalance Linux `rest-1` (470 s) against `rest-2` (336 s) and native (about 300 s): move one or two pinned binaries out of `rest-1` (`test-shard.sh:56-62`), or move cache save to a lighter shard. Inference: levelling to about 340 s saves about 2 min on the Linux critical path. | about 2 min Linux; none for total until #1 is done | Shard times drift as tests change; `scripts/test-shard/test.sh` already checks coverage |
| 3 | Run independent script suites concurrently inside the `scripts` group (installer 59 s, release 50 s, eval 32 s, driver 28 s are the long ones), or split it into two CI jobs. Inference: with 4 cores, about 250 s could drop to about 100 to 130 s. | about 2 min on the scripts job; about 2 min for local full runs | Suites share a build slot (`check.sh:75`), temp dirs and the `target` lock; need testing for isolation. Keeps the `check.sh` contract that one step list is the truth (`scripts/check-steps`) |
| 4 | Build CI test profiles without debug info (`CARGO_PROFILE_DEV_DEBUG=0` in the `ci.yml` env), which also shrinks the cached `target` and its 41 to 79 s restore. Inference only; not measured. Do not trust the gain until A/B timed. | unknown, maybe 20 to 60 s per shard | Backtraces lose line detail on test failures; `RUST_BACKTRACE=1` is set deliberately (`check.sh:83`) |
| 5 | Locally, use `scripts/check.sh --fast` for the inner loop; the pre-push hook already does so for ticket branches (`check.sh:22-27`). A step timer (print elapsed per `== NAME`) would make this note's work repeatable. Inference: a ten-line change in `step`/`finish`. | removes the need for hand-timing | Very low |
| 6 | Move `shellcheck` and cheap lint ahead of `cargo test` locally so a failure shows early (the earlier note found a ShellCheck error 12.6 min after a push; [bottlenecks note](2026-11-bottlenecks-dev-test-release.md)). | faster failure, not faster pass | Step order is part of `--list`; registry tests will need updating |

## What not to do

- Do not re-propose running test binaries in parallel on CI: PR #444 was slower on 4-core runners (task brief), and I have no new evidence.
- Do not restore the `target` cache for `formal`, `lint` or `scripts` groups: a stale cached build once broke every `checks` job on main (`ci.yml:114-122`).
- Do not skip `--cargo` steps in CI to save time: CI must always run them all (`check.sh:62-70`, comment on cargo_plan).
- Do not treat local sandbox seconds as CI numbers (see Method and limits).

## Sources

1. `scripts/check.sh` (header, `step`, step blocks) and `scripts/check.sh --list`
2. `.github/workflows/ci.yml`
3. `scripts/test-shard.sh`
4. `rust-toolchain.toml` (Rust 1.100.0, profile minimal)
5. GitHub Actions run 45918508128, job and step timestamps: https://github.com/aparsons/oska/actions/runs/45918508128
6. [GitHub Docs: GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
7. [Cargo Book: cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
8. [Cargo Book: profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
9. `docs/research/2026-11-bottlenecks-dev-test-release.md`
10. Local timestamped runs of `check.sh --group lint` and `--group scripts` (sandbox, not saved in the repo)
