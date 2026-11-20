# What are the bottlenecks in the development, testing, and release of Oska, and which changes would have the biggest impact on them?

Accessed: 2026-11-20. Every source below was read on 2026-11-20.

- **Method.** I read the repo at `bd2f129` (`main`, 2026-11-20). I read the measured data in `/mnt/project-files/oska-report/out/` (collected 2026-11-20 04:25 to 04:27 UTC). I read GitHub data for `aparsons/oska` with the GitHub MCP tools: the last 100 closed pull requests, CI and Release runs, job lists and two job logs. I did not run a build or a test. I read 5 external pages (GitHub Docs, the Cargo Book).
- **Earlier notes.** This note builds on [`2026-11-development-throughput.md`](2026-11-development-throughput.md) (2026-11-15, the Pi's CPU) and [`2026-11-claude-code-software-factory.md`](2026-11-claude-code-software-factory.md) (2026-11-12). It does not repeat them. "Earlier findings" below says which of their findings still hold.
- My own conclusions are marked **Inference:**.

## Summary

The machine work is no longer the main brake. **Waiting for Adam is.** Pull requests that agents may merge themselves are open for a median of 0.5 hours. Pull requests that need Adam wait 15 to 19 hours. Every release waits about 1 to 2 hours for Adam to push a tag, and the release itself then takes 3 to 4 minutes.

On the machine side, CI's critical path is the `test` job's `scripts/check.sh`: 10 to 17 minutes. It has grown back since PR #437. A whole CI run takes a median of 13.6 minutes. The cloud sessions cannot run several checks that CI runs, so some failures show up only in CI, 12 or more minutes after the push.

Ranked by expected impact for the effort:

| # | Bottleneck | Evidence | Change | Expected impact | Who must act |
| --- | --- | --- | --- | --- | --- |
| 1 | **Human-gated merges wait overnight** | #445 18.9 h, #460 16.9 h, #474 15.6 h open; agent-merged median 0.5 h (closed PR list) | Ask for Adam's decision when the PR opens, not at the end. Post one daily "Adam queue" of cards. Record the decision so the agent merges (as on #474) | Removes most of a 15 to 19 h wait for about 1 to 3 PRs a day (Inference) | Agents (cards); Adam (answers) |
| 2 | **Merge gate cannot run in cloud** | `scripts/expected-workflows/check.sh:26` uses `gh pr checks`; cloud `gh` fails on GraphQL (`docs/agents/issue-tracker.md:131`); PR #460 comment: "the script needs GraphQL and can't run here" | Rewrite the check to read check runs through the REST API | Every agent merge (98 of the last 100 PRs merged) gets its gate back, with no hand check | Agent; reversible |
| 3 | **CI critical path 14 to 17 min; formal step 3.2 min on one TLC worker** | Run 44667696497: `check.sh` 16 m 45 s on ubuntu-latest, formal 03:40:04 to 03:43:16, log line "with 1 worker on 4 cores" | Run TLC with more workers; run cheap lint steps (shellcheck, fmt) first | About 2 min off each `test` job; fail-fast for lint errors (Inference) | Agent (`scripts/formal/`, `scripts/check.sh` order); reversible |
| 4 | **Cloud sessions skip steps that CI runs** | PR #445: Windows type-check, Lean and PowerShell skipped locally; run 44615868898 failed on ShellCheck SC1087 after 12.6 min | Install ShellCheck, pwsh, Lean/TLC and the Windows target in the cloud environment's setup | Failures surface in the local check, not a CI round trip later | Adam (environment settings); reversible |
| 5 | **Release tag needs Adam** | Bump merge to tag run: 56 min (v0.4.0), 1 h 44 min (v0.5.0); release runs 2 to 4 min | Agent posts the exact tag command as a card the moment the bump merges; Adam keeps the tag | Cuts the wait, keeps the security boundary (Inference) | Agent + Adam |
| 6 | **The test run itself: 534 s of tests run one binary at a time** | Run 44667696497: 200 test binaries, 2,519 tests, 534 s summed; `site.rs` 66 s, `native_prelude` 43 s, `lean_reference` 35 s | Move `formal` and the script tests to a parallel CI job; keep `check.sh` one script | About 5 min off the `test` jobs' wall time (Inference) | Adam (workflow change) |
| 7 | **Long integration branches** | PR #445: 46 commits, 186 files, 3 cancelled CI runs, 1 failure after merging `main` (run 44548029815) | Keep one PR per ticket to `main` in cloud too | Fewer conflicts and merge-skew failures (Inference) | Coordinator |
| 8 | **Target cache (PR #472) is unmeasured** | Draft since 2026-11-19 10:05 UTC; "CI times … to be added"; compile is 1 m 47 s of 16 m 45 s | Measure first; merge only if it saves more than about 1 min a job | Small: at most the compile time (Inference) | Agent measures; Adam merges |
| 9 | **Follow-up and triage debt** | 22 "Follow-ups from the PR" issues since 2026-11-17, 13 still open; 12 open `needs-triage` issues (search API) | One triage pass a day | Not a speed limit now; keeps spec churn down (Inference) | Coordinator |

**Reversible without Adam:** 2, 3, 7, 9 and the measuring half of 8. **Need Adam:** 1 (his answers), 4 (environment settings), 5 (the tag), 6 and the merge of 8 (workflow changes, `docs/agents/merge-policy.md:28`).

## Earlier findings: what still holds and what changed

- **The Pi's CPU is no longer the binding constraint.** *Changed.* The 2026-11-15 note found the Pi oversubscribed. Since 2026-11-18, the pull requests come from cloud sessions: every PR head since #383 is a `claude/project-thread-*` branch (closed PR list). The cloud machine is "cloud x86_64, 4 cores, noisy" (`out/bench_cloud.json`, field `machine`). Inference: Pi load matters only for work that still runs there.
- **The same check runs 2 to 3 times.** *Fixed.* The pass stamp shipped (PR #299, PR #341, PR #383; `docs/agents/implementer.md:52-64`).
- **Other agents' `pkill`.** *Fixed.* A kill guard hook refuses it (`docs/agents/implementer.md:44-50`).
- **The test phase dominates `check.sh`.** *Still holds.* The test run is about 9 of 16.7 minutes on ubuntu-latest (run 44667696497). The `opt-level = 1` override was adopted (`Cargo.toml:18-32`, PR #306).
- **nextest.** *Not adopted.* A related idea, PR #444 (run test binaries side by side), was closed: on CI the test phase went from 207 s to 238 s, and `site.rs` from 27 s to 120 s (PR #444 comment).
- **Late verification.** *Partly changed.* `ci.yml` now runs on `ticket/**` pushes (`.github/workflows/ci.yml:7-8`). But cloud sessions "cannot push `spec/` or `ticket/` branches" (PR #445 body), so in cloud only the pull-request trigger runs.
- **Refreshing every open PR after each merge.** *Fixed.* The refresh moved to CI with `gh pr update-branch` (`docs/wiki/problem-refresh-after-merge.md:9`; `docs/wiki/handoff-2026-11-18.md:6`).
- **Job timeouts.** *Fixed.* Every CI job has `timeout-minutes` (`.github/workflows/ci.yml:34,80,102,116`), and a guard checks it (`out/security.json`, `workflow_guard` passes).
- **A ruleset that requires CI on `main`.** *Open.* I could not read the repository settings. `docs/agents/merge-policy.md:49` still warns that a base branch may have no required checks.

## Part 1: Development inner loop

- **The loop is documented and lean.** Iterate on one crate, push early, run the full check once (`docs/agents/implementer.md:5-10`). A fast tier exists (`docs/agents/implementer.md:14-16`).
- **Locally the full check is about 4 to 5 minutes in cloud.** PR #444 measured `scripts/check.sh` at 260 s locally on 4 cores after #437 (PR #444 body). PR #437 measured `cargo test --all` falling from 333 s to 192 s (PR #437 body).
- **But the cloud check skips steps CI runs.** PR #445 says the local check skipped "the Windows GNU type-check (target not installed), the Lean reference (Lean not installed), and the PowerShell parse" (PR #445 body). `check.sh` skips ShellCheck, pwsh, actionlint, Lean/TLC and the Windows target when they are missing (`scripts/check.sh:517,641,661,684,705`).
- **Those gaps cost CI round trips.** In the last 50 CI runs, 2 failed (`out/ci.json`). Both were real failures, not flakes:
  - Run 44615868898: ShellCheck SC1087 in `scripts/formal/test.sh`, on both Linux jobs. The fix is commit `aa6f489`. The error appeared at 21:21:50, about 12.6 min after the run started at 21:09:15 (job log; `out/ci.json`).
  - Run 44548029815: `run_limits_steps_native` failed on every platform after a merge of `main` (job logs). PR #445 later says the digests were "recorded again after merging main" (PR #445 body).
  - Inference: with ShellCheck installed in the cloud environment, the first failure would have shown locally in seconds.
- **Shell habits cost turns.** The permission check refuses heredocs, compound git commands, `git -c` and any command containing "eval" (`docs/agents/implementer.md:28-34`; `docs/wiki/problem-worktree-guard.md:3-15`). Agents work around this with script files. Inference: a small, steady tax, not a top bottleneck.
- **Agents stop while waiting.** A foreground call is cut off after about 180 s (`docs/agents/implementer.md:34`). Agents were "made to report" while their check still ran (`docs/wiki/problem-agent-repeat-handbacks.md:11`). The working fix is a watcher that resumes them.
- **Duplicate work across threads.** Two tickets were dispatched twice on 2026-11-18 (#355; #381 against #389) (`docs/wiki/problem-duplicate-dispatch.md:3-9`). A search before dispatch is now the rule.
- **The oskac build cache cannot speed up CI.** *Lead confirmed.* The cache key includes the SHA-256 of the running `oskac` (`crates/oskac/src/cache.rs:11-17`), and each CI job builds a new `oskac`. The differential tests clear the environment (`crates/oskac/tests/differential/mod.rs:411`). Inference: a CI job never sees an earlier job's entries.

## Part 2: Testing and CI

### Where CI's time goes

- **Run time.** Median 13.6 min over 44 completed, non-cancelled CI runs (range 10.1 to 25.1 min) (`out/ci.json`, 2026-11-19 10:07 to 2026-11-20 03:26 UTC). Runs before 17:00 UTC on 2026-11-19 had a median of 13.1 min; later runs 14.5 min.
- **Critical path.** The `test` job on Linux. In the newest `main` run, `test (ubuntu-24.04-arm)` took 15.2 min, `test (ubuntu-latest)` 10.8 min, `macos (macos-15-intel)` 8.8 min (`out/ci_jobs.json`, run 44665540295). In PR run 44667696497, `check.sh` took 16 m 45 s on ubuntu-latest and 14 m 21 s on arm64; Intel Mac `cargo test` took 12 m 20 s (job list).
- **Windows is not the critical path.** Windows runs took 8 to 11 min (windows.yml runs 44677913529, 44676383400, 44667696466).
- **One `check.sh` on ubuntu-latest, run 44667696497 (job 136060926398):**

| Phase | Time | Source |
| --- | --- | --- |
| Setup, toolchain, Windows target, ShellCheck, Lean/TLC install | about 32 s | job steps 1 to 8 |
| Guards, fmt, clippy, Windows type-check, compile of the test profile | about 2 m 18 s (compile 1 m 47 s) | log: "Finished `test` profile [unoptimized + debuginfo] … in 1m 47s" at 03:29:02 |
| `cargo test` run | about 9 m 05 s (03:29:02 to 03:38:07) | log |
| Script test steps | about 2 m (03:38:07 to 03:40:04) | log |
| `formal` (Lean build 38 s, then 8 TLA+ models) | 3 m 12 s (03:40:04 to 03:43:16) | log |
| Remaining steps, ShellCheck, pwsh | about 13 s | log |

- **The test run.** 200 test binaries, 2,519 tests, 534 s of summed run time. The slowest: `site.rs` 66 s, `native_prelude.rs` 43 s, `lean_reference.rs` 35 s, `native_corpus.rs` 30 s. The top 8 are 248 s (46%) (log of job 136060926398). Cargo runs the binaries "serially" ([cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)).
- **The suite grows.** 1,638 tests on x86_64 on 2026-11-15 (earlier note, Part 1) against 2,519 now. `lean_reference.rs` (PR #473) and the `formal` step (PR #441) are new since PR #437.
- **CI builds with debug info.** The log says "[unoptimized + debuginfo]". `dev` defaults to `debug = true` ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). The lean environment turns it off only locally (`docs/agents/implementer.md:18-24`). Inference: setting `CARGO_PROFILE_DEV_DEBUG=0` in CI may shave part of the 1 m 47 s compile. Not measured.
- **TLC runs one worker.** Each model logs "Running breadth-first search Model-Checking … with 1 worker on 4 cores" (job 136060926398 log). Three models took about 40, 40 and 60 s. Inference: more workers could cut the formal step. Not measured. TLC's own docs are outside the sources I was allowed, so the flag is an open question.
- **Cheap checks run last.** ShellCheck is step 675 of 709 lines in `check.sh` (`scripts/check.sh:675`). Run 44615868898 learned of a ShellCheck error 12.6 min after the push.

### What the earlier leads said, checked

- **"Ubuntu test ~12 min before #437, ~7 after."** *Partly right.* PR #437 gives a median of 10.2 min (8.9 to 12.4) before and 7.4 min in one run after (PR #437 body). Since then the job is back at 10.8 to 16.7 min (above).
- **"PR #471 cut Intel Mac cargo test 7m45 to 5m34."** *Confirmed* (PR #471 body). Intel Mac `cargo test` is now 9 m 58 s to 12 m 20 s (runs 44633034880, 44667696497). Inference: suite growth ate the gain.
- **"macos-15-intel waits ~4 min for a runner."** *Not seen.* In runs 44633034880 and 44667696497 every job started within 6 s of creation. In run 44665540295 the Apple-silicon job started about 1.4 min after the others (`out/ci_jobs.json`). Runner waits vary; two samples are not enough to rule them out.
- **"PR #444 was slower on 4-core runners."** *Confirmed* (PR #444 comment).
- **"PR #472 is a draft that needs a human merge."** *Confirmed.* It changes `ci.yml` (PR #472 body; `docs/agents/merge-policy.md:28`). It has no CI timings yet.

### Runners and caching (external facts)

- Public-repo Linux runners have 4 CPUs and 16 GB; `macos-latest` has 3 (M1) and 7 GB; `macos-15-intel` has 4 and 14 GB ([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
- "Larger runners are available for organizations and enterprises on GitHub Team and GitHub Enterprise Cloud plans" (same page). Inference: not open to a user-owned repo without a plan change.
- The Actions cache limit is 10 GB per repository by default, and entries unused for 7 days are removed. A pull-request run can restore caches from its base branch ([dependency caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)). PR #472's design (only `main` saves) fits this.
- Inference on #472: the compile is about 1 m 47 s of a 16 m 45 s job, so a perfect cache saves at most that. The risk (a stale build) is real. Measure before merging.

### Flaky tests

- Flakes are not the bottleneck now. Neither CI failure in the last 50 runs was a flake (above). One known flake is open: #395 (search API). Earlier flakes #270, #271, #327 and #344 are closed.

## Part 3: Release

- **The machine part is fast and reliable.** All 5 tag runs of `release.yml` succeeded on the first attempt, in 2 to 4 min (runs 42754230070, 42908062622, 43338676580, 43700791052, 44457483839).
- **The human part is slow.** Only repository admins may create `v*` tags (`docs/agents/release.md:91`).

| Release | Bump PR merged | Tag run started | Wait | Published | Docs PR merged |
| --- | --- | --- | --- | --- | --- |
| v0.4.0 | 08:49:49 UTC, 2026-11-15 (#297) | 09:45:43 | 56 min | 09:48:22 | (not checked) |
| v0.5.0 | 02:34:52 UTC, 2026-11-19 (#414) | 04:19:09 | 1 h 44 min | 04:22:41 | 04:51:21 (#417) |

- **The lead "cloud sessions get 403 on tag push" is not directly verified here.** I found no log of it. What I can cite: the tag ruleset limits tags to admins (`docs/agents/release.md:91`), and the handoff says releases "need the user's yes" (`docs/wiki/handoff-2026-11-18.md:6`). Either way, the tag waits for Adam.
- **Cadence.** Five releases in nine days: v0.1.0 2026-11-10, v0.2.0 2026-11-10, v0.3.0 2026-11-13, v0.4.0 2026-11-15, v0.5.0 2026-11-19 (GitHub releases list).
- **v0.6.0 waits on human-gated work.** The notes say the Windows workflow PR for #179 "still needs Adam to merge" (`out/notes.json`, "Critical path"). #179 to #182 are open (`out/milestone.json`). The forecast is "within about a day once the workflow PR is merged" (`out/notes.json`, "Forecast").

## Part 4: Process and human gates

### Merge latency

- **Agent-merged PRs are fast.** Of the last 100 closed PRs, 98 merged. Open-to-merge time: median 0.50 h, 75th percentile 0.96 h, 90th percentile 3.47 h (closed PR list).
- **Human-gated PRs wait overnight.**
  - #445, a spec PR with a new ADR (a one-way door): 18.9 h (PR #445 body: "a human merges it").
  - #460, ADR 0022: 16.9 h. Merged "because Adam explicitly approved it" (PR #460 comment).
  - #474, Windows native: 15.6 h. Merged by an agent "after Adam chose 'Merge now, fix argv0 in #475' on a decision card" (PR #474 comment).
  - #472 (workflow change): a draft, open since 2026-11-19 10:05 UTC.
- **The exceptions are broad.** A human merges any PR that changes workflows, release or install scripts, guards, hooks or settings, weakens a test, or is a one-way door (`docs/agents/merge-policy.md:24-33`).
- Inference: the gates are there for good reasons. The cost is the wait, not the gate. #474 shows a faster pattern: a decision card, then an agent merge.

### The merge gate in cloud

- Merge condition 2 is `scripts/expected-workflows/check.sh` (`docs/agents/merge-policy.md:16`). It calls `gh pr view`, `gh pr diff` and `gh pr checks` (`scripts/expected-workflows/check.sh:22-26`).
- In cloud, `gh` can fail on GraphQL calls (`docs/agents/issue-tracker.md:131`). On PR #460 the agent wrote: "Expected workflows were checked by hand, because the script needs GraphQL and can't run here" (PR #460 comment).
- Inference: each of about 30 merges a day (`out/daily.json`: 29 on 2026-11-18, 32 on 2026-11-19) relies on a hand check. A REST version (check runs for the head SHA) would restore the gate.

### Branches and integration

- Cloud sessions cannot push `spec/` or `ticket/` branches (PR #445 body). The docs still say to push `ticket/<n>-<slug>` (`docs/agents/implementer.md:9,84-86`). Inference: the docs and the cloud setup disagree.
- PR #445 carried a whole spec: 46 commits, 186 files, 8,797 added lines (PR #445). Its thread had 3 cancelled CI runs and 1 failure after merging `main` (`out/ci.json`, branch `claude/project-thread-2p2cnc`; run 44548029815).
- The coordinator's rule is "One PR per ticket, straight to main" (`docs/wiki/handoff-2026-11-18.md:7`). Long branches conflicted in 3 of 4 merges earlier (`docs/wiki/goal-drain-backlog.md:72`).
- Cancelled runs are 6 of 50 (`out/ci.json`). They come from `cancel-in-progress` for pull requests (`.github/workflows/ci.yml:17-19`). That is by design and saves runner time.

### Ticket-spec churn and follow-up debt

- PR #445 found its tickets wrong in places: "Ticket #448 said 'as today', which was wrong", and "Proof commands differ from the tickets" (PR #445 body).
- 22 "Follow-ups from the PR" issues were filed since 2026-11-17; 13 are open (search API, `is:issue "Follow-ups from the PR"`). 12 open issues carry `needs-triage` (search API).
- Open issues: 114; issues opened on 2026-11-19: 42, closed: 21 (`out/snapshot.json`; `out/daily.json`).
- Inference: the backlog grows faster than it closes on busy days. That is a planning cost more than a speed limit.

## Part 5: Changes, ranked by impact for effort

1. **Decide early, merge late (human gates).** When an agent opens a PR that `merge-policy.md` gives to Adam, it posts the decision card at once, with the merge conditions it will check. Adam answers once; the agent merges when CI is green, as on #474. Batch cards into one daily queue. *Impact:* removes most of a 15 to 19 h wait on 1 to 3 PRs a day (Inference). *Effort:* low. *Reversible:* yes. *Needs Adam:* only to answer. A change to `merge-policy.md` itself needs his merge (`docs/agents/merge-policy.md:29`).
2. **REST version of `expected-workflows/check.sh`.** Read the PR's files and base with `gh api repos/.../pulls/N`, and check runs with `gh api repos/.../commits/SHA/check-runs`. *Impact:* restores an automatic merge gate for every merge in cloud. *Effort:* low. *Reversible:* yes. *Needs Adam:* no (it is not a guard directory under `merge-policy.md:29`).
3. **Shorten the `test` job without a workflow change.** (a) TLC with more workers in `scripts/formal/`; (b) move fmt and ShellCheck early in `check.sh`. *Impact:* about 2 min a run, and a lint failure in under a minute (Inference; measure). *Effort:* low. *Reversible:* yes. *Needs Adam:* no, as long as no step is removed or weakened (`docs/agents/merge-policy.md:30`).
4. **Close the local/CI parity gap in cloud.** Install ShellCheck, pwsh, Lean and TLC (`scripts/formal/install.sh`), and the Windows target in the cloud environment's setup. *Impact:* the two CI failures seen would have shown locally (Inference). *Effort:* low. *Reversible:* yes. *Needs Adam:* yes, the environment settings are his.
5. **Release: shorten the wait for the tag.** When the bump PR merges, the agent posts the exact `git tag -a … && git push origin vX.Y.Z` command (`docs/agents/release.md:98-103`) as a card. Keep the admin-only tag. *Impact:* the 56 to 104 min wait shrinks to Adam's response time (Inference). *Effort:* trivial. *Reversible:* yes. A tag workflow that lets Actions bypass the ruleset would remove the wait but widen who can release; that is a one-way door for Adam.
6. **Split the `test` job in CI.** Run `formal` and the script tests in a second job, in parallel with the cargo steps. Keep `check.sh` the single local entry point. *Impact:* about 5 min off the critical path (Inference, from the phase table). *Effort:* medium. *Reversible:* yes. *Needs Adam:* yes (workflow change).
7. **One PR per ticket in cloud too.** Do not carry a spec on one long `claude/*` branch. *Impact:* fewer conflicts and merge-skew failures (Inference). *Effort:* low. *Reversible:* yes. *Needs Adam:* no. Also fix the docs that still say `ticket/**` (`docs/agents/implementer.md:9,84-86`).
8. **#472, measured.** Finish the two-run measurement, then decide. *Impact:* at most the compile time, about 1.5 min a job (Inference). *Effort:* done, apart from the numbers. *Needs Adam:* yes (workflow change).
9. **CI debug info off.** Set `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` in `ci.yml`, as locally. *Impact:* unknown, bounded by the 1 m 47 s compile. *Needs Adam:* yes (workflow change). Measure on a PR first.
10. **Daily triage of follow-ups.** *Impact:* keeps spec churn and the backlog in check. *Effort:* low. *Needs Adam:* no.

Not recommended now: larger runners (need a GitHub Team or Enterprise plan for an organization, per [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)); a merge queue (only for organization-owned repositories, per [merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)); running test binaries side by side (PR #444 was slower on CI).

## Open questions

- Whether `main` has a ruleset with required checks. I could not read the settings.
- Whether a cloud session's tag push fails with 403. Not seen in any source I could read.
- How many more TLC workers help, and what CI debug-info-off saves. Neither is measured.
- Runner queue times over more than three runs.
- Agent token cost per merged PR. `out/notes.json` has no figure for 2026-11-19 or 2026-11-20.

## Sources

Internal (all read 2026-11-20):

- `CLAUDE.md:1-3`; `Cargo.toml:1-32`; `rust-toolchain.toml`
- `scripts/check.sh:460-705`; `scripts/expected-workflows/check.sh:12-26`
- `.github/workflows/ci.yml:1-124`; `.github/workflows/windows.yml`, `confinement.yml`, `release.yml`, `installers.yml`, `pages.yml`, `resign-index.yml`, `rust-stable.yml`, `bench.yml` (triggers and timeouts)
- `docs/agents/implementer.md:5-105`; `docs/agents/merge-policy.md:1-51`; `docs/agents/release.md:85-109`; `docs/agents/rust.md`; `docs/agents/issue-tracker.md:125-137`
- `docs/wiki/index.md`, `handoff-2026-11-18.md:6-8`, `goal-drain-backlog.md`, `forecast-2026-11-18.md`, `problem-*.md`
- `crates/oskac/src/cache.rs:1-42`; `crates/oskac/tests/differential/mod.rs:411`
- `docs/research/2026-11-development-throughput.md`; `docs/research/2026-11-claude-code-software-factory.md`
- `/mnt/project-files/oska-report/out/ci.json`, `ci_jobs.json`, `daily.json`, `merged.json`, `milestone.json`, `notes.json`, `snapshot.json`, `security.json`, `bench_cloud.json`; `/mnt/project-files/oska-report/refresh.md`

GitHub, `aparsons/oska` (read through the GitHub MCP tools and `gh api search/issues`):

- PRs #297, #414, #417, #437, #444, #445, #460, #471, #472, #474, #491; the last 100 closed PRs
- CI runs 44667696497 (jobs, log of job 136060926398), 44665540295, 44633034880, 44615868898 (failed-job logs), 44548029815 (failed-job logs)
- Release runs 42754230070, 42908062622, 43338676580, 43700791052, 44457483839; releases v0.1.0 to v0.5.0
- Windows runs 44677913529, 44676383400, 44667696466
- Issue searches: open issues; "Follow-ups from the PR"; `flaky`; `needs-triage`

External (5 pages):

- https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue
- https://doc.rust-lang.org/cargo/commands/cargo-test.html
- https://doc.rust-lang.org/cargo/reference/profiles.html

Instructions found inside data: none were aimed at this task. `refresh.md` is a runbook for the coordinator's hourly refresh; I read it as data and did not follow it.
