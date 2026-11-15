# Research: could Oska speed up Claude's scripting and this project's development tooling?

- **Date:** 2026-11-15
- **Method:**
  - **Measured here.** On the dev machine (Raspberry Pi 5, 4 cores, Python 3.15.2 from `uv`, `oskac` 0.3.0). I streamed the orchestrating session's transcript and the other transcripts under `~/.claude/projects/-home-pi-oska/` with `grep` and `jq` and counted the `Bash` tool calls by the program they ran. I read the commands the model wrote, not their output, and I quote none of them. I timed `python3`, `sed`, `jq` and `oskac` with bash `time` (user plus system CPU, and wall clock) and recorded `uptime` beside each group. I built a release `oskac` for this note (`~/.local/bin/check-slot -n 2 cargo build --release -p oskac`, lean cargo environment, because none is installed: `command -v oskac` finds nothing) and ran a debug `oskac` that another worktree had already built. I ran two small Oska programs end to end.
  - **From docs and source.** This repository's code, docs and ADRs, and the Claude Code docs downloaded as Markdown from `code.claude.com/docs/en/*.md` (the machine runs Claude Code 2.1.325). No blog posts.
  - Statements that are my reasoning and not a source's are marked *inference*.
- **Question (the maintainer's):** "Could Oska be used to speed up Claude or this project's software development? For example, I see Claude using a lot of Python. Would using Oska be faster?"

This note builds on [`2026-11-development-throughput.md`](2026-11-development-throughput.md), which found that the binding constraint on this machine is CPU and that the test phase is 88 to 94% of `scripts/check.sh`. It does not repeat that note.

## Summary

**Short answer: not today, and speed is the smallest reason.** Oska starts faster than Python, but the difference is about 13 milliseconds of CPU a call, and a Claude turn takes seconds. Oska cannot yet do the parts of Claude's scripting that are not plain text and file work, and the language is frozen against adding them until the parity gate (v0.6.0). Where Oska can help is narrower: this repository's own repeatedly run tooling, where its compiler speed and its capability checks are worth something.

- **Python is a small part of what Claude runs, and most of it is file rewriting.** In the orchestrating session, 7 of 238 `Bash` calls (3%) ran `python3`; together they took 12 seconds of tool time in a session that lasted 9 hours 8 minutes. Across all 305 transcripts of this project in the past week, 1,433 of 13,898 calls (10%) did. About two thirds rewrite a file in place by text substitution; 3% handle JSON (Part 1).
- **Oska can already do that substitution, and it is faster to start.** I ran a read-replace-write program on 2,001 lines. CPU time: `python3` 16 ms; `oskac run --interpret` 3 ms; `oskac run` (compiles natively first) 66 ms; an executable built earlier with `oskac build` 1 ms (Part 3).
- **Native is the wrong mode for a one-off script.** The compile costs 43 to 66 ms for a small program and 116 to 294 ms for the site's two Oska programs, and a one-off runs once. The interpreter does `replace` over text in Rust, so native code gains nothing there (the repository's own benchmark says 1.1x) ([`docs/design.md`](../design.md), "Native backend benchmark").
- **Most of Claude's time is model turns, not tools.** In the orchestrating session the 309 model turns that followed a tool result took 26 minutes (median 4 s); all 238 `Bash` calls took 20 minutes (median 1 s). Fewer, clearer calls would save more than any runtime (Part 4).
- **The tooling gaps are large.** No JSON (`std.json`, #248, is blocked by the parity gate), no regular expressions, no maps, no running processes (`std.proc` checks but does not run), no network, and a symlink anywhere under a directory capability's root is refused (Part 2).

Ranked recommendations (expected impact is *inference* unless a number is cited):

| # | Recommendation | Why | Cost |
| --- | --- | --- | --- |
| 1 | **Do not move Claude's ad-hoc scripting to Oska now.** Leave `python3`, `sed` and `jq` alone. | The saving is about 13 ms of CPU a call (Part 3). The gaps in Part 2 would make some scripts impossible, and each Oska script needs a check-and-fix round that Python does not. | None |
| 2 | **Fix the compiler's own speed in tests before anything else.** Run the throughput note's experiment 2 (a higher `opt-level` for the test profile) and add `oskac` to it. | Measured here: an unoptimised `oskac` takes 5 to 6 times as long to compile the same program (230 to 420 ms against 43 to 65 ms), and tests such as `native_corpus.rs` run `CARGO_BIN_EXE_oskac`, which Cargo builds at `dev` optimisation (no `[profile]` in any `Cargo.toml`). | One `[profile]` line plus a measurement at idle |
| 3 | **Keep the build cache (#279) queued, and expect it to help tests and repeated runs, not one-off scripts.** | A cache hit skips a 43 to 294 ms compile; a one-off script is never run twice, so it never hits. | Already specified |
| 4 | **Write new, repeatedly run tools that only read and write text files in Oska** (more of what `site/build.osk` and `site/check-links.osk` already are), and do **not** port the guard scripts. | Native Oska runs 20 to 80 times faster than the interpreter on logic and publishes its authority report. The guards exist to hold even if the checker is wrong (Part 5). | Per tool |
| 5 | **Re-run this comparison after `std.json`, `std.proc` and the parity gate land, with an evaluation that counts turns and tokens, not seconds.** | Only then can Oska do what the Python does. The speed gap will not have changed; the turn count may (Part 4, Part 5). | An eval task set in the harness (#207) |
| 6 | **Put repeated multi-line shell sequences into checked-in scripts.** | The orchestrator wrote three near-identical 800-character polling loops; one script is one short call each time (inference on the saving). | Small |

## Part 1: where Python actually appears (measured here)

### The orchestrating session

The transcript is `58c1255e-0d1a-4171-9e91-1b127348babf.jsonl`: 4,470 lines, 2026-11-14 23:29 to 2026-11-15 08:37 UTC (9 h 8 min). I took every `tool_use` block named `Bash` (238 distinct ids) and counted a program when it is in command position (start of the command, after `|`, `;`, `&&`, `$(` or a newline). A call can run several programs, so the rows overlap.

| Program | Calls | What it did here |
| --- | --- | --- |
| `git` | 97 | worktrees, branches, merges, diffs |
| `gh` | 76 | issues, PRs, checks, secrets |
| `grep` | 72 | searching, mostly in `git`/`gh` pipelines |
| `sed` | 23 | 7 `sed -n 'X,Yp'` (reading a line range), 7 `sed -i` (one-line edits in place), the rest inside pipelines |
| `jq` | 13 | reading `gh` and hook JSON (49 more calls use `gh --jq`, which is not counted here) |
| `awk` | 11 | range and column picks, one metrics summary |
| `cargo` | 10 | builds and tests |
| **`python3`** | **7** | see below |

The seven `python3` calls were all `python3 - <<'EOF'` heredocs, and all were file rewrites, none JSON and none a computation:

1. Rewrite the front matter of a skill's `SKILL.md` (an exact multi-line string replace behind an `assert old in s`).
2. The same on a status-line script and a second skill file.
3. Insert two paragraphs into a memory file.
4. A heredoc that wrote a shell hook, then a Python rewrite of the status-line script (a block replaced between two markers).
5. Resolve a merge conflict in a test file (cut between the conflict markers, paste a block).
6. Resolve conflicts in four files with one regex and a string replacement of a documentation sentence.
7. Insert a glossary entry before an anchor line, in a new worktree.

Their commands averaged 1,648 characters, against 789 for all `Bash` calls (11.5 KB of the 188 KB of `Bash` input in total). They ran for 1 to 3 seconds each, 12 seconds in all. *Inference:* the model reached for Python where `sed` cannot express a multi-line replacement with an assertion, and where the built-in `Edit` tool, which replaces one exact string per call and must see the file first ([tools reference, Edit tool behavior](https://code.claude.com/docs/en/tools-reference.md#edit-tool-behavior)), would take one turn per hunk.

### All transcripts of this project, past week

`~/.claude/projects/-home-pi-oska/` holds 375 JSONL files from 2026-11-08 to 2026-11-15, 305 of which contain `Bash` calls: this session, three other top-level sessions, and their subagents (implementers, reviewers). I extracted every `Bash` call, de-duplicated by `tool_use` id: **13,898 calls**.

| Program (command position) | Calls | Share |
| --- | --- | --- |
| `grep` | 6,747 | 49% |
| `sed` | 3,596 | 26% (2,749 are `sed -n` reads, 959 are `sed -i` edits) |
| `git` | 2,689 | 19% |
| `cargo` | 1,538 | 11% |
| **`python3`** | **1,433** | **10%** |
| `gh` | 969 | 7% |
| `oskac` | 570 | 4% |
| `awk` | 246 | 2% |
| `jq` | 81 (349 counting `--jq`) | 1% (3%) |

The maintainer's impression is right for the implementer subagents: 1,198 of the 10,950 calls in the largest session's transcripts (11%) are `python3`, and 119 of 1,294 in one subagent group of this session. The orchestrator itself uses it far less (3%).

What the 1,433 Python calls did (my classification of the command text by pattern; it is a heuristic, so read the figures as approximate):

| What it did | Calls | Share |
| --- | --- | --- |
| Rewrite a file in place by text substitution (`.replace`, `re.sub`, `assert old in s`, then a write) | 981 | 68% |
| Run a scratchpad helper script whose body is not in the command (`edit_*.py`, `mk.py`, `probe.py`; a sample of 14 were edit or test-program generators, *inference* for the rest) | 342 | 24% |
| Write or append a new file of generated text | 35 | 2% |
| Read-only text analysis (counts, extraction) | 31 | 2% |
| Read or inspect JSON | 30 | 2% |
| Build or rewrite JSON and issue-ticket files | 14 | 1% |

Some of the same calls use a feature: a regular expression in 154 (11%), directory walking (`os.walk`, `glob`, `pathlib`) in 124 (9%), `sys.argv` in 66, `subprocess` in 12 (under 1%), and a network module name in 14 (1%, a keyword match I did not check). Typical agent Python here is not JSON, processes or the network. It is **read a file, change some text, write it back.**

### The repository itself

`git ls-files` lists no `.py` file, and `git grep -i python` finds no mention in `scripts/`, `.github/`, `.githooks/`, `site/`, `CLAUDE.md` or `docs/agents/`. The repository's tooling is Rust (the crates, plus `scripts/bump-rust/tables.rs`), 28 Bash scripts under `scripts/` (3,686 lines, 1,954 of them in eight `test.sh` files; plus the pre-push hook and `site/agent-setup/oska-check.sh`), and two Oska programs: [`site/build.osk`](../../site/build.osk) (2,575 lines) and [`site/check-links.osk`](../../site/check-links.osk) (415 lines), which the Pages workflow already builds and runs natively ([`pages.yml:31-58`](../../.github/workflows/pages.yml)). All of the Python is therefore Claude's own, written into scratch files and heredocs, and none of it is in a repository file.

## Part 2: what Oska can do today for such tasks (from docs and source)

[`docs/guide/what-works-today.md`](../guide/what-works-today.md) lists what runs. Files, text, `Args`, `Env`, `Stdin`, `Stdout`, `Clock` and `Random` check, run on the interpreter and compile natively on aarch64 and x86_64 Linux. `std.proc` and `Spawn`, `Net`, and `std.sql` check but do not run: the runtime refuses them with `E0371` or `E0370`. The prelude ([`prelude.osk`](../../crates/oska-check/src/prelude.osk)) and the four `std` files (`html.osk`, `proc.osk`, `rt.osk`, `sql.osk`) are the whole library: there is no `std.json`.

| Typical agent one-off | Oska today |
| --- | --- |
| Read a file, replace text, write it back | **Yes.** `DirRead.read_to_string`, `String.replace`, `contains`, `split`, `lines`, `slice`, `DirWrite.write`. I ran it (below). |
| Read lines, count, filter, extract by position | **Yes**, with a `for` loop. `List` has `new`, `push`, `len`, `get`, `at` and `map` only: no `filter`, `sort` or `contains`. |
| Walk a directory tree | **Yes, by hand.** `list_dir` is the only listing call; `site/check-links.osk` recurses and takes a failed `list_dir` to mean "a file" (`walk`, lines 87 to 102). |
| Match a pattern | **No regular expressions.** Only `contains`, `starts_with`, `ends_with`, `split`, `replace`. |
| Count by key, deduplicate, look up | **No map or set.** `check-links.osk` keeps its sets as one text with an item per line and searches it ("Each is a set", lines 43 to 48). |
| Parse or write JSON | **No.** `std.json` is #248 (below). |
| Run `git`, `gh`, `cargo`, `sed` | **No.** `std.proc.run` has a complete signature and no runtime ([`proc.osk`](../../crates/oska-check/src/std/proc.osk): "the runtime implementation comes with the interpreter (#9)" is the comment; #9 is closed and the runtime still refuses it, per `what-works-today.md`). |
| Call a web API | **No.** `Net` and `NetConnect.connect` check only; TLS is spec #228. |
| Edit a file outside the working directory | **Yes, with friction.** A directory capability reaches only beneath its root; the root is a `String` that must be trusted text, so an argument cannot name it without `assume_trusted` (an audit site), and an absolute path, a `..` that climbs above the root and any symlink beneath it are each refused ([`prelude.osk:49-99`](../../crates/oska-check/src/prelude.osk)). |
| Append to a file, delete, rename, check existence, stat | **No** such call. `write` replaces; `create_dir` makes one directory. |
| Hash, base64, time formatting, floating point | **No** (`std.sha2` is #231; there is no `Float` type in the prelude). |

**`std.json` is not close.** #248 ("Agents: `std.json`", ready-for-agent) says parsed JSON is untrusted until decoded into typed values. It is a child of #229 (`std.mcp` and `std.llm`), whose own text says [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md) "freezes such work until the parity gate" (#181, v0.6.0) and that it also waits for the `Bytes` milestone (#209). ADR 0014 freezes "no new syntax, types, prelude or `std` APIs, or capabilities" until all five platforms are native. The tags are `v0.1.0` to `v0.3.0`; macOS and Windows backends are still to come. *Inference:* JSON, a working `std.proc` and the network are therefore all weeks to months away, and the ticket order inside #229 puts `std.json` first.

**Measured here: the edit task works, and the confinement is real.** I wrote the read-replace-write program (`fs.read_only().restrict_to(".")`, `Path.new("config.txt")`, `replace`, `write`, 8 lines) and a general `subst FILE OLD NEW` (13 lines). `oskac check` passed first time and printed an authority report that names each attenuation (`FsRead -> DirRead: restrict_to "."`). `oskac run subst.osk -- /abs/path x y` failed with ``is absolute; a directory capability takes paths relative to its root``, `../etc/passwd` with ``leads outside the capability's root``, and a path through a symlink with ``is a symlink, and confined access never follows one``. A general tool that edits any file therefore has to say `restrict_to("/")`, and its report then says so.

## Part 3: speed, measured

**Setup.** A release `oskac` 0.3.0 built here (36.8 s wall, 89.4 s user, 1.2 s system, at load 7.0, one build slot pair, lean environment); `python3` 3.15.2 (`uv`'s) and the system `/usr/bin/python3` 3.13.5; a 2,001-line `config.txt`; two programs, `hello` (print one line) and `edit` (read the file, replace `retries = 3` with `retries = 5`, write it back). The Python equivalent of `edit` is three lines. Each row is 6 runs, in this order: min / median / max. **The load average was 5.8 for the release runs** (`uptime` at 02:48:40), below the 8 I set as the limit; the earlier debug-build rows were run at 10.0 to 11.0 and are marked. CPU time (user plus system) is the figure to trust; the wall clock is shown because it is what a waiting model sees. The kernel reports CPU time in 4 ms ticks, so differences under 4 ms are noise.

| Command | CPU (ms) | Wall (ms) | Notes |
| --- | --- | --- | --- |
| `/bin/true` | 1 | 1 | process start floor |
| `sed -i` (the edit) | 2 | 3 to 16 | |
| `jq -n 1` | 3 | 6 to 17 | |
| `python3 -c pass` | 16 / 16 / 22 | 15 / 16 / 23 | 3.13 (system): 22 to 23 CPU, 44 to 64 wall, at load 11.0 |
| `python3 edit.py` | 15 / 16 / 16 | 15 / 15 / 16 | |
| `oskac check hello.osk` | 2 / 2 / 2 | 2 / 2 / 2 | |
| `oskac check edit.osk` | 2 / 3 / 3 | 2 / 3 / 4 | |
| `oskac run --interpret hello.osk` | 2 / 2 / 6 | 2 / 2 / 5 | check plus interpret |
| `oskac run --interpret edit.osk` | 3 / 3 / 4 | 3 / 7 / 12 | |
| `oskac run hello.osk` (native) | 43 / 43 / 44 | 43 / 43 / 45 | compiles, writes a temporary executable, runs it |
| `oskac run edit.osk` (native) | 65 / 66 / 78 | 65 / 66 / 87 | |
| `oskac build edit.osk -o exe` | 65 / 65 / 65 | 65 / 65 / 65 | 262,504-byte static executable |
| `./exe` (built earlier) | 0 / 1 / 1 | 0 / 0 / 1 | hello: 131,432 bytes |

Larger programs, 3 runs each at load 5.8: `oskac check site/check-links.osk` 13 ms CPU, `build` 116 ms; `check site/build.osk` 62 ms, `build` 294 ms. Compile cost grows with the program; for a 415-line tool it is about a tenth of a second.

**The debug compiler.** I also ran an unoptimised `oskac` 0.3.0 that another worktree had built (`target/debug/oskac`), at load 10.2 to 10.9: `run hello` (native) 232 to 236 ms CPU, `run edit` (native) 414 to 423 ms, `build edit` 393 to 412 ms, `run --interpret edit` 8 ms. That is 5.4 and 6.3 times the release compile. It matters because Cargo's test profile is `dev` and no `[profile]` in the workspace changes it, and the `oskac` test binaries run `CARGO_BIN_EXE_oskac` ([`native_corpus.rs:81`](../../crates/oskac/tests/native_corpus.rs)). *Inference:* each Oska program the differential suites compile costs the debug figure, not the release one.

**What the numbers say.**

- Against `python3`, `oskac run --interpret` saves about 13 ms of CPU a call and native `oskac run` costs 27 to 50 ms more. Both are far below the 1 to 4 s of a typical Claude tool call and turn (Part 4).
- The built executable is faster than anything else (1 ms), but building it costs 65 ms and a one-off runs once. It pays off for a tool run many times, such as the site generator.
- The repository's own benchmark ([`docs/design.md`](../design.md), "Native backend benchmark", 2026-11-12, `oskac` 0.2.0, best CPU time of 5, on this Pi) says what native buys on the work itself: 46x (an integer loop), 61x (lists), 83x (recursion), 23x (text built by interpolation), 21x (the site generator: 2,226 ms interpreted, 104 ms native), and 1.1x on `replace` and `trim` over 520 KB, because the interpreter runs those in Rust. A read-replace-write script is in the last group.
- The interpreter runs about 4 million simple operations a second ([ADR 0011](../adr/0011-oska-generates-native-code-itself.md)), so a script with a real loop wants the native mode, after its compile. I did not measure Python on a logic-heavy task, so I make no claim about it.
- Wall clock was unreliable at load 10 to 11 (Python's 15 ms became 44 to 76 ms). The CPU figures were steady from run to run, within the 4 ms tick.

## Part 4: the token and latency angle for Claude

**What the transcript says.** For the orchestrating session, from the gaps between a tool result's timestamp and the model's next message (an approximation: I take the next assistant block's timestamp as when the model finished):

| Measure | Value |
| --- | --- |
| Elapsed, first to last message | 9 h 8 min |
| 309 model turns that directly followed a tool result | 26 min 32 s in all, median 4 s, none over 120 s |
| 238 `Bash` calls, tool call to result | 20 min 1 s in all, median 1 s; 190 under 5 s; 3 over 60 s account for 312 s |
| The 7 `python3` calls | 12 s in all (1, 1, 3, 2, 1, 2, 2 s) |
| Assistant output | about 290,000 tokens in 444 assistant messages (the largest `output_tokens` seen for each message id) |
| Everything else | about 8 hours: 18 scheduled wakeups, 23 questions to the maintainer, 27 subagent launches (not attributed further) |

So the model's turns are slightly longer than the tools' time, and both together are under an hour of the nine. Python's share of the tool time is 1%, of the session 0.04%. A faster runtime cannot touch more than that.

**What the docs say.**

- "Claude Code sends your full conversation with every request, and each time Claude uses tools it sends another request carrying that batch of tool results" ([costs](https://code.claude.com/docs/en/costs.md), "Why do long sessions use so much of my plan"). Each call is a request and costs a turn; with a warm cache it is cheap, but it is still a model round trip.
- The Bash tool "runs each command in a separate process" and a command runs under a timeout of two minutes by default ([tools reference](https://code.claude.com/docs/en/tools-reference.md#bash-tool-behavior)). Python's 16 ms start-up is a rounding error against both.
- Edit "performs exact string replacement … It doesn't use regex", needs the file read first, and needs `old_string` to be unique or `replace_all` set ([Edit tool behavior](https://code.claude.com/docs/en/tools-reference.md#edit-tool-behavior)). *Inference:* this, not Python's speed, is why a model rewrites a file with a heredoc: one call, many substitutions, an `assert` to prove each matched. Oska would be the same kind of script in another language, with the cost of a type check first.
- "Lower effort is faster and cheaper for straightforward tasks" ([model config](https://code.claude.com/docs/en/model-config.md), as quoted in the throughput note). Output tokens, including thinking, are billed as output ([costs](https://code.claude.com/docs/en/costs.md)).

**Would a faster runtime matter?** Not for these scripts. The cost of a script is the tokens the model writes (about 1,650 characters, *inference*: roughly 450 tokens, for the average Python heredoc here) and the turn it waits on. *Inference:* a language helps the model only by cutting turns: a script right the first time, an error the model can act on, or one call instead of three. That is the case Oska is built for ("every message has a stable code and a precise span, many carry a fix", [`agent-guide.md`](../guide/agent-guide.md)), and it is untested for scripting. The evaluation harness (#207) is the right place to test it, in turns and tokens to a correct result.

**A security angle worth a line.** Claude Code's file permission rules "don't apply … to arbitrary subprocesses that read or write files indirectly, like a Python or Node script that opens files itself", and only the sandbox gives OS-level enforcement ([permissions](https://code.claude.com/docs/en/permissions.md#read-and-edit)). An Oska program's reach is stated before it runs: `oskac check` prints each capability and each `restrict_to`, as I saw above. *Inference:* a `PreToolUse` hook could refuse an Oska script whose report shows `Net`, `Spawn` or a root of `/`. I have not built it, and it covers only scripts the model chose to write in Oska.

## Part 5: where Oska could genuinely help this project, and where it would not

### What would help

- **More tools like the site generator.** `site/build.osk` and `site/check-links.osk` are the dogfooding that already exists. `oskac check site/build.osk > site/authority.txt` publishes what the generator may do ([`pages.yml:34`](../../.github/workflows/pages.yml)), and the link checker is handed `FsRead` and narrows it to `_site/` and `site/tracked.txt` before it reads anything (`check-links.osk`, header). A bash script has no equivalent report. A new tool that only reads and writes text, and runs on every push or every ticket, fits: a docs or glossary linter is the obvious kind. *Inference.*
- **The site tests, by the interpreter.** In the throughput note, `site.rs` is 68 s of 225 s on x86_64 and six binaries are 96% of the test time; it runs `site/build.osk` through the interpreter on a fake host. The benchmark above has the generator 21 times faster native (2,226 ms against 104 ms on the Pi). The test already runs both: "on aarch64 and x86_64 Linux the generator also runs natively on the same files, and must write the same site" ([`site.rs:15-17`](../../crates/oska-run/tests/site.rs)). *Inference:* the first lever is the test profile's optimisation level (Part 3's debug figures), not rewriting the tests; the native side of those tests pays the 5 to 6 times debug compile.
- **The build cache, #279.** It targets two cases: an agent that runs `oskac check` after every edit and `oskac run` repeatedly on an unchanged package, and the repository's own differential suites. It leaves one-off scripts out by design: a script is a new program each time, so its key is new. On the numbers above, a hit saves 43 to 294 ms for a program the size of the examples and the site; for a check it saves 2 to 62 ms. *Inference:* the benefit is real for the test suites (hundreds of compiles) and small for an agent's loop. The spec's premise, that `oskac` checks and compiles from scratch on every command, is true; the measured cost per compile is small in release and large in the test profile. Tests that call `oska_native` directly do not use the cache (the spec leaves "library entry points" unchanged), so only the tests that run the `oskac` command could gain, and the key includes the SHA-256 of the running binary, so a test-profile `oskac` never shares entries with a release one.
- **The Claude Code hook.** The opt-in `oska-check.sh` post-edit hook is shell with a `sed` to find the file; #194 moves it into `oskac` (`oskac hook claude --event=post-edit`) using the toolchain's own JSON reader, so that it works on Windows. That is Rust inside the compiler, not Oska, but it is where the 2 to 3 ms `check` pays most: it runs after every edit.

### What would have to land first

| Item | Needed for | State |
| --- | --- | --- |
| `std.json` (#248) | Reading `gh --json`, hook input, config; the 44 JSON scripts above | Open, ready-for-agent, blocked by the parity gate (#181) and `Bytes` (#209) |
| A working `std.proc` | Anything that starts `git`, `gh`, `cargo`, `rustup`, `ssh-keygen`, `sha256sum` or `curl`: 12 of the 20 non-test scripts under `scripts/` (my count of command positions), and 34% of the `Bash` calls in the transcripts (59% in the orchestrating session) start `git`, `gh` or `cargo` | Signature complete, no runtime; frozen by ADR 0014 for new APIs, though the signature exists |
| `std.sha2` (#231), `Net` and TLS (#228) | `scripts/cross-build/manifest.sh`, the release verifier (#235), `check-security-txt.sh` | Open |
| The build cache (#279) | Cheap repeated `oskac run` and `test` | Specified, queued after #184 |
| Regular expressions, a map, `sort`, append, delete, existence | Comfortable one-off scripting | Not on the tracker that I found; frozen by ADR 0014 as new prelude API |

*Inference:* the freeze is the real gate. It is a deliberate decision ([ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)), recorded to keep five platforms at parity, and nothing here argues for reopening it.

### What would not help

- **The guard scripts.** [`scripts/intrinsic-guard/guard.sh`](../../scripts/intrinsic-guard/guard.sh) says its scan "holds even if the checker has a hole", and `unsafe-guard` compiles the other crates with `unsafe_code` forbidden by `rustc` itself. A guard written in Oska would run on the checker and compiler it is meant to back up, so a hole in them could hide the thing it looks for. They also run first in `check.sh`, before any `oskac` is built, and a release `oskac` build took 37 s wall (89 s CPU) here, before the 5-minute test phase. The shell versions are 43 to 117 lines each.
- **Release, install and CI glue.** `release/*.sh`, `install/*.sh`, `rust-stable/*.sh` and `cross-build/*.sh` are mostly signing, hashing, download and release steps built on `ssh-keygen`, `sha256sum`, `curl`, `gh` and `rustup`. They need `std.proc` and `std.sha2`, and *inference:* the installer has to run on a machine that has no `oskac` yet, so it cannot be an Oska program.
- **The test phase's time.** `check.sh` spends 88 to 94% of its time in `cargo test`. The throughput note's bottlenecks are CPU oversubscription, repeated runs and the unoptimised test profile. Nothing in Oska's tooling changes them, apart from the build cache and compiler speed above.
- **The model's turns.** See Part 4.

## Part 6: the answer, with conditions

**Would using Oska be faster than the Python Claude uses?** For a single small script, yes by the clock and no by any measure that matters. `oskac run --interpret` is 13 ms of CPU faster than `python3` on a read-replace-write, and native `oskac run` is 27 to 50 ms slower; a Claude turn is a median 4 s, and the seven Python calls of the orchestrating session cost 12 s of a 9 hour session.

**Could Oska speed up Claude?** Only through fewer or better turns, and nothing measured here shows it. The conditions under which it could:

1. `std.json`, `std.proc` and the network exist, so that Oska can do what the Python and shell do (after v0.6.0).
2. An evaluation shows that agent-written Oska scripts reach a correct result in fewer turns or tokens than Python (harness #207), because the checker's coded diagnostics and fixes are a different kind of feedback than a Python traceback.
3. The scripts are read or run repeatedly, so that native compile or the cache pays.

**Could Oska speed up this project's development?** Yes, in two places, and neither is the Python. The compiler's speed under the test profile (5 to 6 times slower than release, measured) and the build cache (#279) are on the critical path of `check.sh`. And new repeatedly run, text-only tools can be written in Oska, as the site tools are, for their speed and their published authority. Do not move the guards, the release glue or the installer.

## Part 7: not measured, and questions for the maintainer

- **Not measured.** A logic-heavy script in Python against native and interpreted Oska; the cost of a compile at idle (all figures at load 5.8 to 11); the Oska side on x86_64; the share of `check.sh` spent compiling Oska programs in the test profile (Part 3 gives the factor, not the total). The `oskac` build for these numbers is my own, at `oskac` 0.3.0, and the transcript classification is by pattern.
- **Questions.**
  1. Should the test profile be optimised for the `oska-*` crates (throughput note, experiment 2), with `oskac` measured as part of it?
  2. Should the build cache be on in `scripts/check.sh` runs (a private `OSKA_CACHE_DIR` per run, to keep tests independent)?
  3. Is an agent-scripting task set, scored in turns and tokens, wanted in the evaluation harness (#207) once `std.json` and `std.proc` run?
  4. Should a hook that reads the authority report of an Oska script before it runs be specified, or is that out of scope until scripts are written in Oska?

## All sources

| Source | Quality | Topic |
| --- | --- | --- |
| [`docs/guide/what-works-today.md`](../guide/what-works-today.md), [`docs/runtime.md`](../runtime.md), [`docs/guide/cli-reference.md`](../guide/cli-reference.md), [`docs/guide/agent-guide.md`](../guide/agent-guide.md) | primary | what checks, runs and builds; native versus interpreter |
| [`crates/oska-check/src/prelude.osk`](../../crates/oska-check/src/prelude.osk), [`std/proc.osk`](../../crates/oska-check/src/std/proc.osk), [`std/html.osk`](../../crates/oska-check/src/std/html.osk) | primary | the whole library surface |
| [`docs/design.md`](../design.md) ("Native backend benchmark", "The prelude"), [ADR 0011](../adr/0011-oska-generates-native-code-itself.md), [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md) | primary | benchmark figures, interpreter speed, the feature freeze |
| `gh issue view` #248, #229, #279, #194, #181, #209, #231, #207 | primary | `std.json`, the build cache, the hook, the gate |
| [`scripts/`](../../scripts/), [`site/build.osk`](../../site/build.osk), [`site/check-links.osk`](../../site/check-links.osk), [`.github/workflows/pages.yml`](../../.github/workflows/pages.yml), [`crates/oskac/tests/native_corpus.rs`](../../crates/oskac/tests/native_corpus.rs) | primary | what the repository's tooling is and runs |
| [`2026-11-development-throughput.md`](2026-11-development-throughput.md) | primary (this repository) | the CPU bound, the test phase, the unoptimised test profile |
| Claude Code docs: [tools reference](https://code.claude.com/docs/en/tools-reference.md), [costs](https://code.claude.com/docs/en/costs.md), [permissions](https://code.claude.com/docs/en/permissions.md), [model config](https://code.claude.com/docs/en/model-config.md), [sandboxing](https://code.claude.com/docs/en/sandboxing.md) | primary | Bash and Edit behaviour, per-request cost, permission rules and subprocesses |
| Local evidence: Claude Code transcripts under `~/.claude/projects/-home-pi-oska/` (305 with `Bash` calls), `uptime`, bash `time`, a release `oskac` 0.3.0 built here, `python3` 3.15.2 and 3.13.5 | measured | Parts 1, 3 and 4 |
