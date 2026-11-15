# Research: keeping Claude and the development process fast

- **Date:** 2026-11-15
- **Method:**
  - **Measured here.** On the dev machine (Raspberry Pi 5, 4 cores, 15 GB RAM, SD card root) I sampled `vmstat`, `/proc`, the cgroup tree and `vcgencmd` while the load average was 12 to 15. I read the local Claude Code session transcripts of this session (counts, timestamps and exit codes only, no content). I pulled the step-by-step log of CI run `43686245247` (`gh run view --log`) and timed each `scripts/check.sh` step and each test binary on GitHub's runners. I did not run any build or test here, because the load average was above 6, so no Pi timing of `check.sh` steps was taken.
  - **From docs and source.** Claude Code docs downloaded as Markdown from `code.claude.com/docs/en/*.md` (the machine runs Claude Code 2.1.325). Cargo book and Cargo source (read at tag `0.101.0`, which I take to be Cargo 1.100.0, the pinned toolchain, from Cargo's version scheme). The `libtest` chapter of the rustc book, the `flock(1)` and `flock(2)` man pages, `systemd.resource-control(5)`, `sccache` and `cargo-nextest` docs, GitHub's docs, the Raspberry Pi docs. No blog posts.
  - Statements that are my reasoning and not a source's are marked *inference*.
- **Question:** What could be done to keep Claude and the development process fast, what are the current bottlenecks, and how can they best be addressed?

This note builds on [`2026-11-claude-code-software-factory.md`](2026-11-claude-code-software-factory.md), which covers the factory's architecture, gates and observability. It does not repeat that note. One of its findings changes: it said the binding constraint on this hardware is disk and `/tmp`. With the lean cargo environment in place, **the binding constraint is now CPU**.

## Summary

The Pi is not slow. A GitHub-hosted arm64 runner has the same shape (4 vCPU, 16 GB) and runs the whole `scripts/check.sh` in 4.5 minutes (x86_64) and 7.5 minutes (arm64). The Pi takes about 5 minutes idle (session notes). It takes 15 to 25 minutes because **several heavy runs share 4 cores**, and because **the same check runs two or three times per ticket**. Nothing in the evidence points at disk, memory, build caching or the network.

Ranked bottlenecks, with the action for each (expected impact is *inference* unless a number is cited):

| # | Bottleneck | Evidence | Action | Expected impact |
| --- | --- | --- | --- | --- |
| 1 | **CPU oversubscription.** 12 to 20 runnable tasks on 4 cores for hours; every cargo invocation sizes itself to all 4 cores and cannot see the others | Measured: 94 to 96% user CPU, 0% I/O wait, run queue 13 to 20, 5 to 9 thousand context switches a second (Part 1) | A machine-wide slot (a `flock`-based semaphore) that `scripts/check.sh` takes before its test phase, starting at 2 slots (Part 2.2) | Same total throughput, but each check finishes in roughly its idle time, so agents stop waiting 15 to 25 minutes; wall-clock-sensitive tests stop flaking under load |
| 2 | **The same check runs 2 to 3 times per ticket**, plus once more in the pre-push hook | Task brief; `.githooks/pre-push` is `exec scripts/check.sh` | Record a pass stamp keyed by the commit's tree; the hook accepts a stamp instead of re-running (Part 2.5) | Removes one to two of every three runs: 33 to 66% of the Pi's check CPU |
| 3 | **Other agents kill running checks with `pkill -f`.** This is the likely cause of the exit-144 cut-offs | Measured: Claude Code reports a SIGTERM'd shell as exit 144; four `pkill` runs by agents in the same window; one cut-off matched to the second (Part 2.3) | A `PreToolUse` hook that denies `pkill`/`killall` patterns not scoped to the caller's own children | Ends the reruns (3 to 4 cut-offs of 15 to 25 minutes each in one evening) and the `--no-verify` workaround |
| 4 | **The test phase is 88 to 94% of `check.sh`, and 6 to 8 test binaries are 79 to 96% of the test phase** | Measured on runners: 225 s of 256 s (x86_64), 414 s of 441 s (arm64) (Part 1) | Measure three experiments at idle: a higher `opt-level` for the interpreter crate under the test profile, `cargo nextest`, and a fast/slow test split (Part 2.1, Part 3) | Unknown until measured; this is the only lever that shrinks the bulk of the time |
| 5 | **Verification happens late and on one platform.** `ci.yml` runs only on pull requests and pushes to `main`; 3 of 4 merges of `main` into a spec branch conflicted | `ci.yml:3-6`; task brief | PR per ticket (spec #191 trial), `ticket/**` push triggers on `ci.yml`, a ruleset that requires CI, auto-merge (Part 2.5) | Platform failures surface within about 8 minutes of a push, not at the end of a spec; conflicts stay small |
| 6 | **The orchestrator session is large and polls.** Context grew from 37 thousand to 530 thousand tokens over 606 requests; 28 shell wrappers hang off it, at least three in `pgrep` loops that can never end | Measured (Part 1) | Hand off to a fresh session at spec boundaries, set `/autocompact`, wait with one blocking command or `Monitor` instead of polling (Part 2.4) | Faster, cheaper turns for the rest of the session; fewer idle wake-ups |

What does **not** help here, with the reason: a shared `CARGO_TARGET_DIR` (an exclusive build lock, and there are no third-party crates to share), `sccache` (compilation is about 15 s of a 4-minute run), faster storage (I/O wait is 0%), and a self-hosted runner (the repository is public; GitHub says not to). Details in Part 2.

## Part 1: measured here

All figures are from 2026-11-15, on the dev machine unless a CI run is named.

### The machine is CPU-bound and oversubscribed

- `vmstat 2 4`, three 2-second samples after the since-boot line: user CPU 94 to 96%, system 3 to 6%, idle 0%, I/O wait 0%, 13 to 20 tasks runnable, 5,088 to 8,607 context switches a second. Load average 12.3 to 15.4 over 1, 5 and 15 minutes.
- `free -m`: 16,218 MB total, 2,533 MB free, 7,426 MB cache, 8,369 MB available, 461 MB of 2 GB swap used (zram). Memory is not the limit.
- `vcgencmd get_throttled` reports `0x0`, the clock is 2.4 GHz (the maximum the kernel reports), the temperature is 65 °C, and the fan turns. The Pi is not thermally throttled.
- `ps` shows three `workerd` processes (another project's `npm run test`) and several `oskac` and test processes. The oska builds are not the only load. They share the machine with other sessions' work (the task brief says as much).
- The kernel has no pressure-stall information (`/proc/pressure` is absent), so a tool cannot read CPU or memory pressure from there.
- Every Claude session, its Bash tools and every build share one cgroup scope (`tmux-spawn-….scope` under `user@1000.service`). No per-session CPU or memory limit applies. The `cpu` controller is available to the user manager (`cgroup.controllers` lists `cpu pids`), so a limit could be set.

### `check.sh` on GitHub's runners (run `43686245247`, `main`, 2026-11-15)

Step times from the job logs; the script's steps are in [`scripts/check.sh`](../../scripts/check.sh) (lines 43 to 108).

| Step | x86_64 (`ubuntu-latest`) | arm64 (`ubuntu-24.04-arm`) |
| --- | --- | --- |
| Whole job | 4 m 26 s | 7 m 28 s |
| `cargo test` step (build, then run) | 225.2 s | 413.8 s |
| of which compiling the test profile | about 15 s | not measured |
| `unsafe only in oska-sys` guard (on a runner, mostly rustup installing the pinned toolchain) | 12.1 s | 10.6 s |
| `cargo clippy` | 4.3 s | 5.0 s |
| `cargo fmt` | 0.9 s | 1.0 s |
| all the guard and script tests, shellcheck | about 14 s | about 10 s |

- The test step is 88% (x86_64) and 94% (arm64) of the script.
- It runs 1,638 tests (x86_64) and 1,903 tests (arm64) in 137 test binaries. Cargo runs test binaries **one after another** and parallelises only inside each ([`cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html): "each target compiles to a special executable … and then is run serially"). The sum of the per-binary "finished in" times, 209.4 s, is nearly the whole step, which agrees.
- **Six binaries are 96% of the x86_64 test time:** `site.rs` 68.0 s (78 tests), `site_home.rs` 29.5 s, `agent_setup.rs` 28.6 s, `site_corpus.rs` 27.5 s, `site_links.rs` 25.9 s, `catalog.rs` 21.5 s. 130 of the 137 binaries finish in under a second. On arm64 eight binaries are 303 s of 385 s (79%), and they add the native-code differential suites (`native_corpus.rs` 30.9 s, `native_prelude.rs` 18.1 s).
- `site.rs` runs the site generator, `site/build.osk`, through the interpreter on a fake host ([`crates/oska-run/tests/site.rs:1-15`](../../crates/oska-run/tests/site.rs)). ADR 0011 says the interpreter runs "about 4 million simple operations a second on a Raspberry Pi 5" ([ADR 0011](../adr/0011-oska-generates-native-code-itself.md)). There is no `[profile]` in any `Cargo.toml`, so the tests run an interpreter built at Cargo's default `dev` optimisation. *Inference:* the slow binaries are slow because an unoptimised interpreter runs a large Oska program many times. Part 3 turns that into a measurement.

The workflows that a push to `main` triggers all finished within about 8 minutes (`gh run list`). Pull-request CI runs took 4.7 to 7.6 minutes.

### Why the Pi takes 15 to 25 minutes

- A CI job is the same shape as the Pi (4 vCPU) and takes 4.5 to 7.5 minutes. The session notes give about 5 minutes for the Pi idle. So the slowdown is load: 15 to 25 minutes is 3 to 5 times the idle time, and the Pi runs 3 to 5 heavy jobs at once.
- Cargo sizes itself to all cores. `build.jobs` defaults to "number of logical CPUs" ([config](https://doc.rust-lang.org/cargo/reference/config.html)), and the jobserver that enforces it is created by each cargo process unless one is inherited from the environment ([Cargo source, `job_queue/mod.rs`](https://github.com/rust-lang/cargo/blob/master/src/compiler/job_queue/mod.rs) states "the jobserver could come from either the environment (e.g., from a `make` invocation), or from Cargo creating its own"). *Inference:* four independent cargo invocations are four independent 4-job pools. Test execution, which is most of the time, is outside the jobserver anyway: `libtest` starts one thread per available core in every test binary ([rustc book: `--test-threads`](https://doc.rust-lang.org/rustc/tests/index.html)).

### The orchestrator session

From its transcript (counts only):

- 606 model requests over about 7.5 hours, all on Opus 5.5 at high effort. The context grew from 36,947 tokens at the first request to 529,978 at the last (234,271 at about 1.5 hours; 432,626 at about 4.7 hours).
- The requests read 187 million tokens from the prompt cache and wrote 2.4 million.
- 27 gaps between requests were longer than 5 minutes, and one was longer than an hour.
- 28 shell wrappers are children of the session process. At least three are `while pgrep -f "cargo test -p oska-run --test site"; do sleep 5; done` loops started about 4 hours ago and still alive. They can never finish: `pgrep -f` matches the loop's own `bash -c '…'` command line, which contains the pattern. I confirmed that `pgrep -af "cargo test -p oska-run --test site"` lists these wrappers.

## Part 2: findings

### 1. Build and test speed on this machine

**Compiling is not where the time goes.** On the x86_64 runner the whole test profile compiles in about 15 s, and `clippy` in 4 s (Part 1). *Inference:* on the Pi idle it is a minute or two, which is the "first run in a fresh worktree" cost and not the 5-minute figure. I did not time it. The way to settle it is `cargo build --tests --timings`, which writes an HTML report of every compiler invocation and the machine's concurrency ([timings](https://doc.rust-lang.org/cargo/reference/timings.html)). Run it once with the machine idle.

**Sharing `CARGO_TARGET_DIR` across worktrees would serialise the agents.**

- Cargo takes an exclusive lock on `.cargo-build-lock` in the build directory for the whole compile unless the unstable `-Z fine-grain-locking` is on ([Cargo source, `compiler/layout.rs` at tag `0.101.0`](https://github.com/rust-lang/cargo/blob/0.101.0/src/compiler/layout.rs), around lines 260 to 280). The same lock code prints "Blocking waiting for file lock on build directory" when contended ([`util/flock.rs`](https://github.com/rust-lang/cargo/blob/0.101.0/src/util/flock.rs)). On Unix it is an advisory `flock(2)` lock.
- `build.build-dir` and `build.target-dir` can be split, and the Cargo 1.100.0 layout of the build directory is documented as internal and changed in that release ([build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html), [config](https://doc.rust-lang.org/cargo/reference/config.html)). Neither changes the lock.
- There is nothing to share. `Cargo.lock` has no third-party crate (the first step of `check.sh` fails if one appears, `check.sh:43-48`), so there is no downloaded or compiled dependency to reuse. The compile is of the workspace's own 77,000 lines, which are different in each worktree.
- *Inference:* two worktrees in one target directory would also invalidate each other's artifacts on every switch. I did not test this. The recommendation does not depend on it.
- Keep the lean environment and a private `target/` per worktree (`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_*_DEBUG=0`; about 0.7 GB, session notes). Cargo's own default agrees for incremental: it is off when `CI` is set, because a build that starts from nothing gains nothing from it ([config: `build.incremental`](https://doc.rust-lang.org/cargo/reference/config.html)). *Suggestion:* put the debug-info setting in the repository (`[profile.dev] debug = 0` in the root `Cargo.toml`; check that `scripts/unsafe-guard` accepts it first) so a new agent does not depend on an environment variable it was never told. The workspace uses no other `[profile]`.

**`sccache` is not worth installing now.**

- It caches rustc invocations in a local disk cache. It cannot cache incremental builds, `bin` or `proc-macro` crates, and by default needs the same absolute paths for a hit, which worktrees under different directories do not have unless `SCCACHE_BASEDIRS` is set ([sccache README](https://github.com/mozilla/sccache/blob/main/README.md), [Rust notes](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)). It starts a server that exits after 10 idle minutes.
- The workspace is six crates that change in every ticket, so hits would be rare after the first change. *Inference:* the best case saves some seconds of a 15-second compile on x86_64.
- Revisit only if `--timings` shows a multi-minute compile on the Pi.

**Test parallelism: `--test-threads` and `RUST_TEST_THREADS`.**

- `libtest` defaults to the hardware's `available_parallelism`; `RUST_TEST_THREADS` is the same setting and is marked deprecated in the rustc book ([tests](https://doc.rust-lang.org/rustc/tests/index.html)). `cargo --jobs` controls building only: "does not affect how many threads are used when running the tests" ([cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)).
- Lowering the thread count is a mitigation for oversubscription, not a speed-up. With a slot (Part 2.2) that leaves one or two checks on the machine, keep the default.
- #270 (`ETXTBSY`) is a fork race between test threads in one process: a child forked by one thread inherits a write descriptor another thread has open, and `exec` of that file then fails. Fewer threads make it rarer without removing it. *Inference:* running each test in its own process removes the cross-thread fork, which is what `cargo nextest` does.

**`cargo nextest`: promising for this shape, but it needs a measurement and a policy decision.**

- It runs each test in its own process, with one orchestrator that schedules tests across **all** binaries at once, where `cargo test` runs binaries one at a time ([nextest: why process-per-test](https://nexte.st/docs/design/why-process-per-test/)). The default concurrency is the number of CPUs ([running tests](https://nexte.st/docs/running/)). A test group with `max-threads = 1` serialises chosen tests ([test groups](https://nexte.st/docs/configuration/test-groups/)). It does not run doctests ([running tests](https://nexte.st/docs/running/)). A prebuilt aarch64 Linux binary exists ([installation](https://nexte.st/docs/installation/pre-built-binaries/)).
- *Inference:* the gain depends on whether the six slow binaries leave cores idle. Inside `site.rs`, 78 tests run on four threads, so on a 4-core machine they probably already saturate it, and nextest would gain little there. It would gain where a binary has few tests or a long tail (`native_corpus.rs` has 6 tests in 30.9 s). Measure, do not assume.
- **Repository rule.** The "no external crates" rule is about the workspace's dependencies: `check.sh` fails if `Cargo.lock` has a `source =` line (`check.sh:43-48`), and ADR 0009 and ADR 0011 give the reason as "the workspace takes no crates from outside the repo". A tool installed outside the workspace does not touch `Cargo.lock`, so the rule does not forbid it. The constraint that does bite is the workflow guard: workflows may use only `actions/*` and the SHA-pinned `rust-toolchain` (`check.sh:62-69`). CI would have to fetch nextest with `cargo install` (a long compile) or a downloaded binary with a pinned checksum. And `ci.yml` says local and CI run the same script so they cannot drift (`ci.yml:40-42`), so a Pi-only nextest would break that. Treat nextest as a question for the maintainer (Part 4), not a default.

**Raise the interpreter's optimisation level under test (an experiment, not a finding).**

- Cargo lets a profile override one package: `[profile.dev.package.oska-run] opt-level = 2` ([profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)); `test` inherits `dev`. The page warns that raising a dependency's level can leave its generic functions unoptimised in the caller, and suggests `opt-level = 1` when experimenting.
- *Inference:* an unoptimised Rust interpreter running `site/build.osk` is the likely reason the site binaries take 25 to 84 s. A higher level costs compile time on the runners too, so the same measurement must be taken on a CI-shaped machine.

### 2. Machine-wide concurrency control

**Why oversubscription hurts.**

- *Throughput does not improve.* The run queue was 13 to 20 on 4 cores with no idle time and no I/O wait (Part 1). Extra concurrent jobs only share the same 4 cores, so each takes longer: 4 jobs take about 4 times as long. Nobody gets a result sooner than if they had run one after another. *Inference* (a CPU-bound system sharing time among jobs).
- *Latency gets worse for the agent that is waiting.* An agent idles while its check runs. Under sharing, every agent waits 15 to 25 minutes. In a queue, the first finishes in about 5 minutes. *Inference.*
- *It adds overhead and makes tests flaky.* 5 to 9 thousand context switches a second (Part 1). #271 failed because a test measured wall-clock time, and the fix (PR #276) measures CPU time instead. Timing-dependent tests are the first to fail on a loaded machine.
- *Cargo cannot see other cargo runs.* Each invocation sizes itself to all CPUs and the test binaries start a thread per core (Part 1). Nothing on the machine coordinates them.

**A `flock` slot at the one entry point.**

- `flock(1)` takes a lock file and runs a command under it, with `--nonblock` and `--timeout` options and a configurable exit status for a refused lock. The lock "is automatically dropped when the file is closed" ([flock(1)](https://man7.org/linux/man-pages/man1/flock.1.html)). A lock belongs to the open file description, so a forked child shares it and a stray background child keeps the lock alive after the parent exits ([flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html)). The kernel drops it when the last descriptor closes, so a killed run does not leave a stale slot (*inference* from the same page, which does not address process death directly). `flock(1)`'s `-o` option closes the descriptor before the command so a child does not hold it.
- `flock` has no counting form, so a pool of N slots is N lock files and a loop that tries each with `--nonblock`, then sleeps and retries.
- Put it in `scripts/check.sh`, before the first compile, and enable it only when an environment variable such as `OSKA_SLOT_DIR` is set. Every route into the heavy work (an agent's own run, the pre-push hook, the maintainer) then goes through it, and CI, which does not set the variable, is unchanged. Hooking each Bash call instead (a `PreToolUse` hook can rewrite a command with `updatedInput`, [hooks](https://code.claude.com/docs/en/hooks)) would also gate cheap commands, and a hook that waits is cancelled at its `timeout` (600 s by default for a command hook), well under a 20-minute wait.
- **Cap.** Start at 2 slots. *Inference:* one check uses all four cores while the slow binaries run, so 1 gives the best latency and 2 keeps the cores busy while one run is in its serial, single-threaded stretches (compiling, linking, the shell guards). Tune by watching `vmstat`'s run queue: it should stay near 4 to 8, not 13 to 20. The session rule of "at most 3 builds" is a good limit for memory and disk and too high for CPU.
- **A jobserver is not the right tool.** Cargo can take its job tokens from an inherited jobserver (the source says so, above), and GNU Make 4.4.1 is installed. But a jobserver only governs rustc jobs, which are about 15 s of a 4-minute run. It does not govern test threads.
- **`systemd-run --user --scope -p CPUWeight=…`** is an alternative that needs no lock: it gives the interactive session priority over builds. The user manager has the `cpu` controller (Part 1), and `CPUWeight` and `CPUQuota` are the documented knobs ([systemd.resource-control(5)](https://man7.org/linux/man-pages/man5/systemd.resource-control.5.html)). It shares the cores more fairly but does not reduce the number of jobs, so it does not fix latency. Use it only for background work such as the other project's tests.
- Claude Code limits subagents to 20 at a time by default (`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`, [sub-agents](https://code.claude.com/docs/en/sub-agents)). That does not limit the builds those agents start, so the slot is the control that matters.

### 3. The exit-144 cut-offs: probably another agent's `pkill`

Claude Code's docs do not mention exit code 144, signal 16 or any rule that stops a foreground push. What they do say about stopping background commands:

- A foreground subagent's background commands stop when it ends; the main conversation's and a background subagent's keep running ([tools reference](https://code.claude.com/docs/en/tools-reference.md)).
- A time limit on background commands (30 minutes by default, 2 hours at most) applies **only to unattended sessions**: `-p` runs, the Agent SDK, CI and cloud. An interactive local session has none ([tools reference](https://code.claude.com/docs/en/tools-reference.md)). This session's record in `~/.claude/sessions/` says `"kind":"interactive"`, so the limit does not apply.
- Background tasks stop at critical operating-system memory pressure, only when the session has been idle for 30 minutes and no turn or subagent is running ([interactive mode](https://code.claude.com/docs/en/interactive-mode.md)). That does not fit a session that was busy.
- On Linux, Claude Code refuses a `pkill` only when the pattern matches its own process ([errors](https://code.claude.com/docs/en/errors.md)); the docs advise `pkill -P $$`. *Inference:* it does not check for other sessions' processes.

What the transcripts show (timestamps are UTC):

- **Exit 144 is what Claude Code reports when a Bash tool shell receives SIGTERM.** I ran `echo before; kill -TERM $$; echo after` in this session's Bash tool: it printed `before` and the result was `Exit code 144`, not 143. So 144 is Claude Code's number for "the shell got SIGTERM", and "signal 16" is its encoding and not a signal anyone sent.
- **Agents run `pkill -f "scripts/check.sh"`, which kills every `check.sh` on the machine.** I found these Bash commands in the orchestrator's and its subagents' transcripts:
  - 03:23:47, one agent: `cargo fmt …; pkill -f "scripts/check.sh"; true`. Its own result was `Exit code 144` at 03:24:03.
  - 03:24:10, a different agent reading a background check's output file: `[exited with code 144]`. That is 7 seconds after the `pkill`. The task's own check was killed.
  - 04:49:46 and 05:16:52, two more agents ran `pkill -f "scripts/check.sh"`; one also ran `pkill -f "temp-leak/run.sh"`.
  - 06:47:03, the orchestrator ran `pkill -TERM -s <session id>` on the processes of a redundant check, and its own command ended with `Exit code 144`.
- The session ledger shows the `#271` push started at 03:17:47Z, inside the 03:23:47 `pkill` window, and it was then re-run detached and pushed at 03:46Z. *Inference:* that is the first cut-off, and the three or four other pushes that were cut off coincide with the other `pkill` runs. I did not match each to the second, because I have no per-push time for the later ones.
- **Why the detached runs survived** is *unresolved*. A `setsid` run also has `scripts/check.sh` in its command line, so a later `pkill -f` would kill it too. *Inference:* they survived because no `pkill` happened while they ran, not because `setsid` protects them. The recorded evidence is too thin to say more.

**Action.** Add a `PreToolUse` hook on `Bash` that exits 2 for `pkill`, `killall` and `kill $(pgrep …)` unless the pattern is scoped (`pkill -P $$ …`, or a PID the command itself started). Exit code 2 blocks the call and its stderr goes to the model ([hooks](https://code.claude.com/docs/en/hooks)); the software-factory note already proposes a `PreToolUse` guard for `--no-verify`, so this is one more rule in the same hook. Until then, say it in every implementer brief: stop only your own processes, by PID file or `pkill -P $$`. Debugging, if it recurs: run Claude Code with `--debug`, whose log "says why tasks were stopped" ([interactive mode](https://code.claude.com/docs/en/interactive-mode.md)), and record `$?` and `date` from a wrapper around each push.

### 4. Claude Code: keeping an orchestrator fast and cheap over many hours

- **Every request resends the whole context; the cache makes that cheap while it is warm.** "Every request sends the whole conversation, so a session that's collected three unrelated tasks re-reads all three on every turn of the fourth. That's discounted while the cache is warm and full price once it expires" ([best practices](https://code.claude.com/docs/en/best-practices)). This session read 187 million tokens from cache at an average of 309,000 tokens a request (Part 1). *Inference:* the late turns are slower than the early ones for this reason, and the orchestrator's job, tracking a ledger, does not need the history of tickets that merged hours ago.
- **Compact or clear at task boundaries, not mid-task.** "Pick your model and effort level at the top of a session, then save `/compact` for natural breaks between tasks" ([prompt caching](https://code.claude.com/docs/en/prompt-caching.md)). Models with a 1M window compact at about 967K tokens by default; `/autocompact 300k` (or `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, 100K to 1M) compacts earlier ([model config](https://code.claude.com/docs/en/model-config.md)). A summary survives compaction together with `CLAUDE.md`, auto memory, and the running background commands and subagents ([context window](https://code.claude.com/docs/en/context-window.md)). The docs' own advice for a session that has outgrown its task: `/clear`, then `/resume` the old one if needed ([costs](https://code.claude.com/docs/en/costs.md)).
- **Hand off at spec boundaries.** The orchestrator's state is already in a ledger file, the issue tracker and git. *Inference:* a fresh session started per spec (or per three or four tickets) that reads the ledger and the spec issue has a context of tens of thousands of tokens instead of 500 thousand. The cost is one cold-cache turn, which is small next to hundreds of warm 500-thousand-token turns.
- **Subagents get a five-minute cache by default, even on a subscription.** The main conversation gets one hour within plan usage; "Everything else", including subagents, gets five minutes unless you set `subagentPromptCacheTtl` or `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL` to `1h` (v2.1.242 or later) ([prompt caching](https://code.claude.com/docs/en/prompt-caching.md)). An implementer that wakes every few minutes to check a 15-minute run misses the cache each time and re-reads its whole context. Setting `1h` costs a higher cache-write rate. *Inference:* the better fix is fewer wake-ups: a single blocking wait (the slot, then `until [ -f RC ]; do sleep 15; done` as one command) instead of repeated turns.
- **Use `Monitor`, or one blocking command, to wait; do not leave polling loops.** The `Monitor` tool "runs a command in the background and feeds each output line back to Claude" ([tools reference](https://code.claude.com/docs/en/tools-reference.md#monitor-tool)). Fix the self-matching `pgrep -f` loops: match a PID or a result file, not a command line that contains the pattern. The docs warn that "a `/loop`, a goal's check after every turn, and Claude's own subagents all keep sending requests while you aren't typing" ([best practices](https://code.claude.com/docs/en/best-practices)).
- **Model and effort.** "Lower effort is faster and cheaper for straightforward tasks" ([model config](https://code.claude.com/docs/en/model-config.md)). A subagent's `effort` frontmatter overrides the session's ([sub-agents](https://code.claude.com/docs/en/sub-agents)), so per-role definitions can carry the model and effort the maintainer already chooses by hand (the software-factory note proposes these definitions). The orchestrator ran at high effort on Opus 5.5 for 606 mostly bookkeeping requests. *Inference:* medium (the default for Opus 5.5 per the model-config page) is likely enough for ledger and merge work, and a changed effort keeps the cache on Opus 5.5 and later ([prompt caching](https://code.claude.com/docs/en/prompt-caching.md)). Fast mode is a separate, higher-priced option with its own cache ([fast mode](https://code.claude.com/docs/en/fast-mode.md)); it speeds one model's responses and does nothing for the Pi's CPU.
- **Auto mode's classifier is outside this note's evidence.** I did not measure its added latency.
- **Isolation costs disk and time.** Each subagent worktree is a fresh checkout with an empty `target/` (worktrees share the object database but not build output, [git-worktree](https://git-scm.com/docs/git-worktree)). The first compile in each is the unavoidable part (Part 2.1, a minute or two *inference*). Claude Code removes a worktree "when the subagent finishes without changes"; one with changes stays until the periodic sweep ([worktrees](https://code.claude.com/docs/en/worktrees)), which is the disk-fill mechanism the software-factory note already describes.

### 5. Git and GitHub workflow

**What exists.**

- GitHub supports stacked pull requests ([about stacked PRs](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs), [reference](https://docs.github.com/en/pull-requests/reference/stacked-pull-requests)). A stack is a chain of PRs in one repository, each targeting the branch below it. Merging is bottom-up, and after the bottom one merges the rest are rebased automatically. The merge requirements come from the bottom PR's base branch. Stacks need a linear history, all branches in one repository (no forks), and are not supported in GitHub Desktop. The `gh stack` CLI extension handles the local workflow. The pages I fetched do not state a preview or general-availability status, and one search summary called it a public preview, so check before depending on it.
- **CI cost multiplies.** "A workflow runs once per pull request, a large stack multiplies your CI usage"; the docs show conditions to run the heavy jobs only on the lowest unmerged PR (`github.event.pull_request.stack.base.ref == github.event.pull_request.base.ref`) or only on the top one ([optimizing CI for stacked PRs](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/optimizing-ci-for-stacked-pull-requests)).
- Auto-merge "merges a pull request automatically after all required reviews and status checks pass", and is offered only when branch protection blocks an immediate merge ([auto-merge](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request)). A merge queue tests each PR against the latest base and the PRs ahead of it, and needs the `merge_group` event in any workflow that reports a required check ([merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)).
- **Repository state (checked 2026-11-15):** `main` has no branch protection (`gh api …/branches/main/protection` returns "Branch not protected"); the only ruleset is "Release tags"; auto-merge is off (`allow_auto_merge: false`). So CI is advisory today, as the software-factory note found.

**The shape of the work matters more than stacks.** *Inference* from the evidence (session notes, ledger):

- A ticket waited 5 to 7 hours to reach `main`, because it waited for the whole spec, its review and its PR. A small PR per ticket merges when its CI is green. Three of four merges of `main` into a long-lived spec branch conflicted, all in files every ticket touches (`site/build.osk`, the docs). Small PRs that land within hours conflict less.
- Windows and x86_64 failures were found at the end of the spec because CI runs only when a PR exists. `windows.yml` and `confinement.yml` already run on pushes to `ticket/**` and `review-fixes-*` branches; `ci.yml` runs only on pull requests and pushes to `main` (`ci.yml:3-6`). *Suggestion:* add `"ticket/**"` to `ci.yml`'s `push` branches (or open each ticket's PR as a draft at once), so the Linux x86_64 and arm64 jobs run as soon as a ticket is pushed.
- Stacks help only a real chain of dependent tickets (the plan in spec #191). Where tickets are independent, separate PRs from `main` need no stack and no rebasing. Each PR in a stack runs the CI again, so a chain of five costs five CI runs per push. The existing `concurrency` group already cancels an older run of the same pull request (`ci.yml:14-16`).

**What moves verification off the Pi.**

- Public-repository standard runners are free and unlimited: `ubuntu-latest`, `ubuntu-24.04-arm` and `windows-latest` are each 4 vCPU and 16 GB ([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)). The Free plan allows 20 concurrent jobs ([limits](https://docs.github.com/en/actions/reference/limits)). The repository's CI already uses them, and a push to `main` completes in about 8 minutes (Part 1). So the full suite on both Linux architectures plus Windows costs the Pi nothing.
- *Inference:* the Pi's `check.sh` run is therefore a pre-flight, not the gate. It should answer "is this safe to push?" quickly, and CI answers "is it correct everywhere?". The cheap way to say so in the process is to run the Pi check once per ticket (as the #191 trial does), and not again in the hook (Part 2.5, below).
- `CLAUDE.md` currently says to "run `scripts/check.sh` before every commit". A faster tier (guards, `fmt`, `clippy`, the fast test binaries) before a commit, with the six slow binaries left to CI, would be a policy change. Part 4 lists it as a question.

**A pass stamp instead of `--no-verify`.**

- The session notes describe running `check.sh` once on a commit and then `git push --no-verify` because the hook just `exec`s `check.sh` (`.githooks/pre-push`). That works, but it trains agents to use the flag the software-factory note wants blocked.
- `pre-push` receives one line per ref on standard input: `<local-ref> SP <local-object-name> SP <remote-ref> SP <remote-object-name>` and aborts the push on a non-zero exit ([githooks](https://git-scm.com/docs/githooks)). *Suggestion:* when `check.sh` passes it writes the tree id of `HEAD` (and refuses if the working tree is dirty) into a file under the git directory; the hook reads the pushed commit from stdin, and exits 0 if that commit's tree has a stamp, and otherwise runs `check.sh`. The gate is the same one, run once, and no flag is needed. `check.sh` unsets the `GIT_*` variables a hook exports (`check.sh:29-30`), so it must compute the stamp's location (`git rev-parse --git-common-dir`, shared by all worktrees) before or after that line on purpose.

### 6. Hardware options

| Option | Effect on this bottleneck | Source |
| --- | --- | --- |
| Faster storage for builds (NVMe over SD) | None for speed: I/O wait was 0% and a compile is about 15 s. It would help disk capacity and wear. The M.2 HAT+ is single-lane PCIe 2.0 at "500 MB/s peak", Gen 3 is "not certified" and "might be unstable", and booting from NVMe works with a recent bootloader | [M.2 HAT+](https://www.raspberrypi.com/documentation/accessories/m2-hat-plus.html); Part 1 |
| Cooling | None needed now: not throttled, 65 °C, full clock. Raspberry Pi's thermal limit is 85 °C, with throttling from 80 °C | [Raspberry Pi docs](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html); Part 1 |
| A second Pi 5 as a worker | Doubles the cores. Raises the cap on concurrent checks from 2 to 4 once there is a slot to enforce it. Costs setup and a second disk budget | *Inference* |
| GitHub-hosted runners (already used) | Same shape as the Pi, 4 vCPU and 16 GB, free for this public repository; both architectures plus Windows | [runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) |
| A self-hosted runner on the Pi or a second machine | **Not advised for this repository.** GitHub: "self-hosted runners should almost never be used for public repositories", because any user can open a pull request that runs code on it | [secure use](https://docs.github.com/en/actions/reference/security/secure-use) |
| Claude Code cloud sessions (each agent in its own VM) | Each session gets its own VM with "4 vCPUs, 16 GB of RAM, 30 GB of disk", so parallel agents stop sharing the Pi's cores. The VM is **x86_64**, so it cannot run the arm64-only native differential tests (which CI's arm job runs), and it may stop memory-hungry builds | [cloud environments](https://code.claude.com/docs/en/cloud-environments.md) |

*Inference:* more cores only help after the slot (Part 2.2) and the stamp (Part 2.5) are in place. Without them a second machine would be oversubscribed in the same way.

## Part 3: experiments to run at idle

None of these has been run. Each needs the machine with a load average near 1 (`uptime`), one change at a time, and `check.sh` or the named command timed three times.

1. **Baseline.** `time scripts/check.sh` with the lean environment, and `cargo build --tests --timings` in a fresh `target/`. Records the real idle cost of the compile and of each step on the Pi, which this note could only take from CI.
2. **Test-profile optimisation.** Re-time `cargo test -p oska-run --test site` with `CARGO_PROFILE_TEST_OPT_LEVEL=1`, then `2`, and again with a per-package override for `oska-run` only. Keep the result if the saving is clearly larger than the added compile time, on both the Pi and a CI-shaped run.
3. **nextest.** `cargo nextest run --workspace` (plus `cargo test --doc`) against `cargo test --all`. Count wall time, and run #270's test 50 times under each to see whether the `ETXTBSY` failure disappears.
4. **Slot cap.** With the slot in place, run 4 checks at cap 1, 2 and 3 and record total time and time to first result.

## Part 4: questions for the maintainer

1. Should `scripts/check.sh` take a local-only slot (enabled by an environment variable, so CI is unchanged), and should the starting cap be 2?
2. Is a pass stamp acceptable in place of running the hook's check a second time? It keeps one gate, and removes the reason for `--no-verify`.
3. May the Pi's pre-commit check be a faster tier, with the slow test binaries left to CI? `CLAUDE.md` says "before every commit" today.
4. If nextest wins at idle, may CI install it by a pinned download, given the workflow guard allows only `actions/*` and the pinned `rust-toolchain`? Or should it stay a local-only tool?
5. Should `ci.yml` run on `ticket/**` pushes, as `windows.yml` and `confinement.yml` do?
6. Should the orchestrator hand off to a fresh session at each spec boundary, with the ledger and spec issue as the only carried state?

## All sources fetched

| Source | Quality | Topic |
| --- | --- | --- |
| [Cargo: build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html) | primary | target-dir, build-dir, sccache mention |
| [Cargo: configuration](https://doc.rust-lang.org/cargo/reference/config.html) | primary | `build.jobs`, `build.incremental`, `build.build-dir` |
| [Cargo: profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) | primary | `test` inherits `dev`, per-package overrides |
| [Cargo: `cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html) | primary | binaries run serially, `--jobs` vs `--test-threads` |
| [Cargo: timings](https://doc.rust-lang.org/cargo/reference/timings.html) | primary | `--timings` report |
| [Cargo: environment variables](https://doc.rust-lang.org/cargo/reference/environment-variables.html), [build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html) | primary | jobserver, `RUSTC_WRAPPER`, `CARGO_INCREMENTAL` |
| Cargo source at tag `0.101.0`: [`src/compiler/layout.rs`](https://github.com/rust-lang/cargo/blob/0.101.0/src/compiler/layout.rs), [`src/util/flock.rs`](https://github.com/rust-lang/cargo/blob/0.101.0/src/util/flock.rs); and `master` for `job_queue` and `context` | primary (source) | exclusive build lock, blocking message, jobserver from environment |
| [rustc book: tests](https://doc.rust-lang.org/rustc/tests/index.html) | primary | `--test-threads`, `RUST_TEST_THREADS` |
| [jobserver crate](https://docs.rs/jobserver/latest/jobserver/struct.Client.html) | primary | `from_env_ext`, token acquisition |
| [flock(1)](https://man7.org/linux/man-pages/man1/flock.1.html), [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html) | primary | lock semantics, options, inheritance |
| [systemd.resource-control(5)](https://man7.org/linux/man-pages/man5/systemd.resource-control.5.html) | primary | `CPUWeight`, `CPUQuota`, `AllowedCPUs` |
| [proc_loadavg(5)](https://man7.org/linux/man-pages/man5/proc_loadavg.5.html) | primary | load average counts R and D tasks |
| [sccache README](https://github.com/mozilla/sccache/blob/main/README.md), [Rust notes](https://github.com/mozilla/sccache/blob/main/docs/Rust.md) | primary | incremental, uncacheable crate types, `SCCACHE_BASEDIRS` |
| [nextest: why process-per-test](https://nexte.st/docs/design/why-process-per-test/), [running tests](https://nexte.st/docs/running/), [test groups](https://nexte.st/docs/configuration/test-groups/), [pre-built binaries](https://nexte.st/docs/installation/pre-built-binaries/) | primary | scheduling, doctests, groups, aarch64 binary |
| Claude Code docs: [tools reference](https://code.claude.com/docs/en/tools-reference.md), [interactive mode](https://code.claude.com/docs/en/interactive-mode.md), [errors](https://code.claude.com/docs/en/errors.md), [env vars](https://code.claude.com/docs/en/env-vars.md), [sub-agents](https://code.claude.com/docs/en/sub-agents.md), [worktrees](https://code.claude.com/docs/en/worktrees.md), [hooks](https://code.claude.com/docs/en/hooks.md), [prompt caching](https://code.claude.com/docs/en/prompt-caching.md), [context window](https://code.claude.com/docs/en/context-window.md), [costs](https://code.claude.com/docs/en/costs.md), [model config](https://code.claude.com/docs/en/model-config.md), [best practices](https://code.claude.com/docs/en/best-practices), [fast mode](https://code.claude.com/docs/en/fast-mode.md), [cloud environments](https://code.claude.com/docs/en/cloud-environments.md), [agent view](https://code.claude.com/docs/en/agent-view.md) | primary | background commands, time limits, `pkill`, cache TTLs, compaction, effort, cloud VMs |
| [GitHub: stacked pull requests](https://docs.github.com/en/pull-requests/reference/stacked-pull-requests), [about](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs), [optimizing CI](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/optimizing-ci-for-stacked-pull-requests) | primary | stacks, CI cost |
| [GitHub: workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax), [limits](https://docs.github.com/en/actions/reference/limits), [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [secure use](https://docs.github.com/en/actions/reference/security/secure-use) | primary | concurrency, path filters, runner shapes and free use, self-hosted runner risk |
| [GitHub: merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue), [auto-merge](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request) | primary | required checks, `merge_group` |
| [git: githooks](https://git-scm.com/docs/githooks), [git-worktree](https://git-scm.com/docs/git-worktree) | primary | `pre-push` input, what worktrees share |
| [Raspberry Pi: M.2 HAT+](https://www.raspberrypi.com/documentation/accessories/m2-hat-plus.html), [hardware docs](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html) | primary | NVMe, PCIe speed, thermal limits |
| Local evidence: CI run `43686245247` logs, `gh run list`, `gh api` (branch protection, rulesets), `vmstat`, `/proc`, `vcgencmd`, Claude Code session transcripts and `~/.claude/sessions/*.json` | measured | all of Part 1 and the exit-144 timeline |
