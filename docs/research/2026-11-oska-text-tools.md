# Research: would Oska versions of grep, sed, awk and jq make Oska development faster?

- **Date:** 2026-11-15
- **Method:**
  - **Measured here.** On the dev machine (Raspberry Pi 5, 4 cores, Claude Code 2.1.325). I parsed every transcript under `~/.claude/projects/-home-pi-oska/` (408 JSONL files, 2026-11-08 to 2026-11-15: the five top-level sessions and their subagents) with a Python script. For each `tool_use` block I kept the tool name, the `Bash` command, the call's timestamp, and from the matching `tool_result` its timestamp, its size in characters, `is_error`, and every line of the output that starts `grep:`, `ugrep:`, `sed:`, `awk:` or `jq:`. A call's duration is the gap between the two timestamps; it includes the harness and any permission prompt, and I left out calls started with `run_in_background`. I classified each `Bash` call by the programs in command position, after removing heredoc bodies and quoted strings (the rules are below each table). I timed the tools with bash `time` (7 runs, median), with the load average at 5.6 to 6.3, and ran no cargo build.
  - **From docs and source.** This repository's code, docs and ADRs; the Claude Code docs downloaded as Markdown from `code.claude.com/docs/en/*.md`; the GNU grep, GNU sed and gawk manuals, the POSIX `sed` page, FreeBSD's `sed(1)`, the jq 1.7 manual and ripgrep's README. No blog posts.
  - Statements that are my reasoning and not a source's are marked *inference*.
- **Question (the maintainer's):** "Would Oska development be faster if there were Oska implementations of grep, sed, awk and jq?"

This note builds on [`2026-11-oska-for-development-tooling.md`](2026-11-oska-for-development-tooling.md) (Python against Oska for agent scripts, what Oska's library lacks, `oskac` start-up) and [`2026-11-development-throughput.md`](2026-11-development-throughput.md) (CPU-bound Pi, test phase 88 to 94% of `scripts/check.sh`). It does not repeat them.

## Summary

**No. The text tools are a rounding error in where the time goes, they almost never fail, and an Oska version would be slower and less well integrated with Claude Code than what the agents already use.** The value of writing them is elsewhere: as test programs that would force a regular-expression library and `std.json` into existence, which is dogfooding and not speed.

- **The agents use the text tools constantly, but they cost almost no time.** `grep` is in 7,169 of 15,270 `Bash` calls, `sed` in 3,868, `awk` in 229 and `jq` in 103 (plus 575 `gh --jq`). The 4,480 calls that run only text and viewing tools took 4,133 s in all: **2.4% of the 48.6 hours of `Bash` time**, median 0.4 s a call. Builds, tests and waits for them took **74%** (Part 1).
- **Almost all of a text-tool call's 0.4 s is not the tool.** The tool itself takes 2 to 16 ms here: `grep -rn` over `crates` and `docs` 9 to 10 ms, `sed -n` a range 2 ms, `jq` on 31 KB 4 ms. A faster binary could save at most a few milliseconds of each call; one idle `scripts/check.sh` (about 5 minutes, throughput note) is about 30,000 `grep -rn` runs (Part 3).
- **An Oska version would be slower.** A fixed-string line count over the repository's 4.8 MB of text: GNU `grep -c` 2 ms of CPU, the same program in Oska 28 ms as a prebuilt native executable, 361 ms to compile it, and 564 ms on the interpreter. Oska has no regular expressions at all (Part 3).
- **Failures are rare and cheap.** About 38 calls (0.25%) printed an error from one of the tools, and most were a wrong path, which any tool would report. Only about 14 (0.09%) were the tool's language: jq quoting (5), a gawk-only function under `mawk` (2), `sed` expressions (2), patterns read as options or bad regexes (5). Each cost one model turn (Part 2).
- **Claude Code already ships the fast tools and is built around them.** On Linux, `grep`, `find` and `rg` in Claude's shell are functions that run `ugrep`, `bfs` and ripgrep embedded in the Claude binary ([tools reference, Glob tool behavior](https://code.claude.com/docs/en/tools-reference.md#glob-tool-behavior); `type grep` here). The Edit tool accepts `cat`, `head`, `tail`, `sed -n 'X,Yp'`, `grep` and `rg` as a read of the file, and the Bash tool treats exit 1 from `grep`, `rg` and `find` as benign. A tool with another name gets neither (Part 4).

Ranked recommendations (expected impact is *inference* unless a number is cited):

| # | Recommendation | Why | Cost |
| --- | --- | --- | --- |
| 1 | **Do not build Oska grep, sed, awk or jq to speed up development.** | 2.4% of `Bash` time, of which the tools' own run time is a few percent; an Oska version is 14 times slower than GNU grep even native (Parts 1 and 3). | None |
| 2 | **Spend the effort on the 74%:** the throughput note's build slots, pass stamp and test-profile work. | Builds, tests and waits are 129,342 s of 174,857 s of `Bash` time (Part 1). | Already in flight |
| 3 | **Have agents read `oskac --format=json`, not grep its human output.** Say so in [`docs/agents/implementer.md`](../agents/implementer.md). | 699 `Bash` calls that name `oskac` pipe it to `grep`; 179 use `--format=json`; none pipe that to `jq` (Part 1). The JSON has stable codes and spans ([`agent-guide.md`](../guide/agent-guide.md)), so a filter cannot drop or misread a diagnostic. | One paragraph |
| 4 | **If dogfooding is wanted, treat `grep` and `jq` as acceptance tests for `std.regex` and `std.json` (#248) after the parity gate,** not as tools for agents. | Both are missing from the library; ADR 0014 freezes new `std` APIs until the gate (Part 5). | A spec each, later |
| 5 | **Smallest step now:** add the fixed-string line scan from Part 3 as a workload in [`scripts/bench-native.sh`](../../scripts/bench-native.sh). | It needs no new API, and native Oska at 28 ms against GNU grep at 2 ms on the same 4.8 MB is a concrete string-performance target (Part 3). | A few lines |

## Part 1: how the agents use the tools (measured here)

### All tool calls

21,123 `tool_use` blocks in 408 transcripts, de-duplicated by id. `Bash` 15,270; `Edit` 1,834; `Write` 1,107; `Read` 579; **`Grep` 0 and `Glob` 0**. Those two tools are not in these sessions at all: "On macOS, Linux, and WSL, Claude Code leaves Glob and Grep out of the default tool set, and Claude searches with `find` and `grep` through the Bash tool instead. In Claude's shell those two commands run embedded versions of `bfs` and `ugrep`" ([tools reference](https://code.claude.com/docs/en/tools-reference.md#glob-tool-behavior)). Here `type grep` prints a shell function that runs `/home/pi/.local/share/claude/versions/2.1.325` as `ugrep -G --ignore-files --hidden -I --exclude-dir=.git …`, and `rg` and `find` are similar functions. So the agents' `grep` is already Claude Code's own embedded `ugrep` 7.8.4, which skips `.gitignore`d and binary files; `command grep` is GNU grep 3.11.

### Which programs, and what for

A program counts once per call when it is in command position (start, or after `|`, `;`, `&&`, `||`, `$(`, a newline, `then`, `do`), after heredoc bodies and quoted strings are removed.

| Program | Calls that run it | Of 15,270 |
| --- | --- | --- |
| `grep` | 7,169 | 47% |
| `head` / `tail` / `cat` | 5,633 / 3,044 / 2,881 | |
| `sed` | 3,868 | 25% |
| `git` | 2,969 | 19% |
| `cargo` | 2,146 | 14% |
| `python3` | 1,557 | 10% |
| `gh` | 1,113 (575 with `--jq` or `-q`) | 7% |
| `awk` | 229 | 1.5% |
| `jq` | 103 | 0.7% |
| `rg` | 0 | |

What the uses were (occurrences, not calls: one call can hold several):

| Use | Occurrences | What replaces it today |
| --- | --- | --- |
| `sed -n 'X,Yp'` on a file (read a line range) | 4,058 | `Read` with `offset`/`limit`; the Edit tool already counts this `sed` as a read |
| `grep` on named files (often a log in the scratchpad) | 5,288 | |
| `grep` filtering another command's output (after `\|`) | 3,819 | |
| `grep -r` over a tree | 1,305 | |
| `sed -i` in place | 1,254 | `Edit` (1,834 calls), Python rewrites (the tooling note) |
| `sed` after a pipe (`-n` filter or transform) | 467 | |
| `awk` after a pipe / on files | 216 / 46 | |
| `jq` after a pipe / on a file or `-n` | 53 / 86 | `gh --jq` (575 calls) |

Two uses stand out. **Most `grep` filtering is trimming build output:** 3,111 of the 3,807 build-and-test calls pipe into `grep`, `tail`, `head`, `sed` or `awk` (1,902 into `grep`). And **699 calls that name `oskac` pipe into `grep`**, 179 use `--format=json`, and none pipe that JSON into `jq`. *Inference:* the agents grep human-readable diagnostics where a structured form exists.

### Where the time and the tokens go

Each call goes in the first category that matches, in this order: build/test (`cargo`, `check.sh`, `check-slot`, `nextest`), CI wait (`gh run watch`, `gh run view`, `gh pr checks`), Python, wait/poll (`sleep`, `until`, `while`), `oskac`, `gh`; then *text-only* (only `grep`, `sed`, `awk`, `jq` and viewing tools such as `cat`, `head`, `wc`, `ls`), `git` with a text tool, `git`, other with a text tool, view-only, other.

| Category | Calls | Time (s) | Share of time | Median (s) | Output (M chars) | Share of output |
| --- | --- | --- | --- | --- | --- | --- |
| build/test | 3,806 | 62,162 | 35.6% | 2.5 | 3.3 | 9.8% |
| wait/poll | 734 | 62,427 | 35.7% | 140.5 | 0.3 | 0.9% |
| CI wait and logs | 210 | 4,753 | 2.7% | 3.2 | 0.2 | 0.7% |
| other | 703 | 16,816 | 9.6% | 1.4 | 0.6 | 1.7% |
| `git` | 1,437 | 8,356 | 4.8% | 1.3 | 2.1 | 6.1% |
| `git` + text tool | 510 | 4,556 | 2.6% | 0.8 | 1.6 | 4.6% |
| `gh` | 688 | 4,271 | 2.4% | 2.4 | 3.2 | 9.3% |
| **text-only** | **4,480** | **4,133** | **2.4%** | **0.4** | **16.9** | **49.9%** |
| other + text tool | 592 | 3,006 | 1.7% | 1.0 | 0.9 | 2.8% |
| Python | 1,011 | 2,805 | 1.6% | 0.6 | 0.8 | 2.3% |
| view-only | 931 | 1,287 | 0.7% | 0.8 | 3.9 | 11.5% |
| `oskac` | 168 | 286 | 0.2% | 1.0 | 0.1 | 0.3% |
| **All `Bash`** | **15,270** | **174,857 (48.6 h)** | | | **33.8** | |

- **Time.** Builds, tests and waits for them (the first three rows) are 129,342 s, **74%**. Text-only calls are 2.4%, at a median of 0.4 s each. The same agents wrote 13.4 million output tokens over 18,723 assistant messages (the largest `output_tokens` per message id, thinking included). The tooling note measured a median of 4 s per model turn in the orchestrating session; a text-tool call's cycle is therefore about 4 s of model and 0.4 s of tool call, of which the tool process is about 0.01 s (Part 3).
- **Tokens.** Text-only calls returned half of all `Bash` output: 16.9 million characters (*inference*: roughly 4 million tokens at 4 characters a token), a median of 2,198 characters a call, and only one over the 30,000-character inline limit. The `Read` tool returned 4.2 million. That output is the file text and matches the agents asked for, so a different implementation of the same command prints the same thing. Only asking for less (a count, a file list, a narrower range) changes it. The commands themselves are short: 0.81 million characters for all 4,480 text-only calls (*inference*: about 0.2 million tokens, 1.5% of the output tokens).

## Part 2: failures and what they cost (measured here)

I searched every `Bash` result for lines that start with a tool's name (`grep:`, `ugrep:`, `sed:`, `awk:`, `jq:`) and for jq's "(Unix shell quoting issues?)" hint, and counted distinct calls.

| Tool | Language, quoting or dialect | Wrong path | Data | Calls |
| --- | --- | --- | --- | --- |
| `ugrep` (the shell's `grep`) | 3: a pattern starting `->` read as an option (2), "error at position 395" (1) | 19 | | 22 |
| GNU `grep` | 2: `unrecognized option '---- …'`, `invalid max count` | 1 | | 3 |
| `sed` | 2: `unknown command: '-'`, `unknown option to 's'` | 1 | | 3 |
| `awk` | 2: `function asort never defined` | | | 2 |
| `jq` | 5: syntax errors, 3 of them flagged "(Unix shell quoting issues?)" | 1 | 2: `Cannot index array with string`, `cannot be matched, as it is not a string` | 8 |
| **Total** | **14 (0.09% of `Bash` calls)** | **22** | **2** | **about 38 (0.25%)** |

- **The dialect failures are real but few.** `asort` is a gawk extension: "gawk provides the built-in `asort()` and `asorti()` functions" while "in most awk implementations, sorting an array requires writing a sort() function" ([gawk manual, 12.3.2](https://www.gnu.org/software/gawk/manual/html_node/Array-Sorting-Functions.html)). This machine's `awk` is `mawk` 1.3.4, so a gawk habit failed twice. `sed -i` is the classic portability trap: POSIX `sed` has no `-i` (its synopsis is `sed [-En] script [file...]`, [POSIX sed](https://pubs.opengroup.org/onlinepubs/9799919799/utilities/sed.html)), and BSD and macOS `sed` take `-i extension` with a required argument ([FreeBSD sed(1)](https://man.freebsd.org/cgi/man.cgi?query=sed&sektion=1)). It caused **no** failure here, because every agent runs on Linux with GNU sed 4.9; it would matter only to an agent on macOS.
- **Every text-only call that failed for any reason:** 33 of 4,480 (0.7%), and 26 of those were Claude Code refusing a command in a worktree-isolated agent, 3 the auto-mode classifier. For comparison, `Edit` failed 8 times in 1,834 calls.
- **The cost.** One extra model turn per failure. At the tooling note's median of 4 s a turn, 38 failures are about 2.5 minutes of model time in a week of 286 transcript-hours (the sum of each transcript's first-to-last span; the agents overlap). *Inference:* a wrong path, 22 of the 38, would fail the same way in any tool.
- **The shipped scripts handle portability by hand already.** [`scripts/install/install.sh`](../../scripts/install/install.sh) avoids `sed -i`, uses `LC_ALL=C awk` for its edits, reads JSON with `tr` and `sed` so that it does not need `jq` (`json_value`, lines 540 to 549), and computes epochs in `awk` "as GNU and BSD date parse dates differently" (line 1065). Two commits in 548 fixed ShellCheck findings in the installer tests (`6cd9e1c` SC2012, `a3bb1c0` SC1003), one line each. The Windows installer is a separate 1,221-line [`install.ps1`](../../scripts/install/install.ps1). An installer cannot be an Oska program, because it runs before `oskac` exists (the tooling note, Part 5).
- **Not measured.** A `sed -i` or `s///` that silently matched nothing (GNU sed exits 0 for that), and failures the agent noticed only later. The transcripts do not show them.

## Part 3: time per invocation (measured here)

7 runs each, median, by bash `time`, at load 5.6 to 6.3. "Corpus" is the repository's 713 tracked UTF-8 text files concatenated: 4,791,256 bytes, 122,148 lines. The Oska binary is the release `oskac` 0.3.0 left in the session scratchpad from 2026-11-14 (no `.debug_info` section, 10.5 MB); no release `oskac` is installed and I built none.

| Command | Wall (ms) | CPU (ms) |
| --- | --- | --- |
| `/bin/true` (process floor) | 1 | 1 |
| `grep -rn 'pub fn check' crates docs` (the shell's embedded `ugrep`) | 10 | 9 |
| `command grep -rn …` (GNU grep 3.11) | 9 | 9 |
| `rg -n …` (embedded ripgrep 14.1.1) | 15 | 16 |
| `git grep -n …` | 12 | 13 |
| `sed -n '300,360p'` on `prelude.osk` | 2 | 2 |
| `cp` plus `sed -i` one substitution on a 16 KB file | 3 | 3 |
| `jq` select-and-count on a 31 KB array | 4 | 4 |
| GNU `grep -c 'pub fn'` corpus | 2 | 2 |
| `ugrep -c` corpus | 3 | 3 |
| `rg -c` corpus | 5 | 5 |
| `mawk` count corpus | 14 | 14 |
| `python3 -c pass` | 21 | 21 |
| `python3` JSON count on the 31 KB array | 38 | 38 |
| `oskac check filt.osk` | 8 | 8 |
| `oskac run --interpret filt.osk` (corpus) | 628 | 564 |
| `oskac build filt.osk` | 393 | 361 |
| `oskac run filt.osk` (native: compile, then run) | 355 | 355 |
| `./filt` (the executable `oskac build` wrote) | 28 | 28 |

`filt.osk` is 11 lines: `read.restrict_to_file("corpus.txt")`, `read_to_string`, a `for` over `lines()`, `contains("pub fn")`, a counter. It needs no new API and printed 762, the same as `grep -c`. Its authority report was `fn main: Stdout (io), FsRead (read)` and `FsRead -> FileRead: restrict_to_file "corpus.txt"`.

- **The tools are milliseconds; the work around them is minutes.** `check.sh` takes about 5 minutes idle on this Pi, 15 to 25 minutes under load, and 4.5 to 7.5 minutes on GitHub's runners ([throughput note](2026-11-development-throughput.md), Summary). The text-only calls' median of 0.4 s per call against the tools' 2 to 16 ms means at least 96% of each call's time is the harness and the round trip.
- **Oska is slower at this, not faster.** Native Oska is 14 times GNU grep's CPU on the same fixed-string scan, the compile is 13 times the run, and the interpreter is 280 times. *Inference:* `lines()` builds a list of 122,148 strings before the loop starts; grep scans the buffer. A regex grep could not be written at all today (no regex in the prelude, [`prelude.osk`](../../crates/oska-check/src/prelude.osk) `String` methods).
- **One number I cannot explain.** This binary's native `oskac run` of a one-line hello took 218 ms of CPU; the tooling note measured 43 ms for a release build at a similar load. The interpreter's hello took 7 ms (2 ms there). This build may come from an older commit. It does not change the conclusion: even the tooling note's 43 to 66 ms compile is 20 times a GNU grep run.

## Part 4: what Claude Code already provides, and what a new tool would lose (from the docs)

- **Search.** On Linux and macOS the agent's `grep` and `find` are embedded `ugrep` and `bfs`, and `rg` is the embedded ripgrep ([Glob tool behavior](https://code.claude.com/docs/en/tools-reference.md#glob-tool-behavior); `USE_BUILTIN_RIPGREP`, [env vars](https://code.claude.com/docs/en/env-vars.md)). ripgrep "will respect gitignore rules and automatically skip hidden files/directories and binary files" ([ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md)). On Windows the `Grep` and `Glob` tools are in the default set, and the `PowerShell` tool runs commands natively ([tools reference](https://code.claude.com/docs/en/tools-reference.md#powershell-tool)). The dedicated `Grep` is "built on ripgrep" and by default returns file paths only (`files_with_matches`) ([Grep tool behavior](https://code.claude.com/docs/en/tools-reference.md#grep-tool-behavior)). *Inference:* cross-platform consistency for agents' searching is already Claude Code's job, not Oska's.
- **Read-before-edit.** "Claude can also edit a file without a separate Read after viewing it with a Bash command such as `cat` or `grep`. Those commands are `cat`, `nl`, `bat`, `batcat`, `head`, `tail`, `sed -n 'X,Yp'`, `grep`, `egrep`, `fgrep`, and `rg`, each run on one file with no pipes or redirects" ([Edit tool behavior](https://code.claude.com/docs/en/tools-reference.md#edit-tool-behavior)). An `osed -n` or `ogrep` view would not count, so each edit after it would need a `Read` call first, which is one more turn.
- **Exit status.** "A command that exits 1 counts as a valid result for the Bash tool only when Claude Code recognizes exit code 1 as a benign outcome for that command: `grep`, `rg`, `egrep`, `fgrep`, `find`, `diff`, `test`, and `[` … Every other command that exits 1 counts as a failure", and a failure is shown inline only up to about 10,000 characters, against 30,000 for a valid result ([Bash tool behavior, output limits](https://code.claude.com/docs/en/tools-reference.md#output-limits)). GNU grep's convention is "the exit status is 0 if a line is selected, 1 if no lines were selected" ([GNU grep manual, Exit Status](https://www.gnu.org/software/grep/manual/grep.html#Exit-Status)); jq returns non-zero for a false or null last value only with `--exit-status` / `-e` ([jq 1.7 manual](https://jqlang.org/manual/v1.7/#invoking-jq)). An Oska grep that followed grep's convention would be reported as a failure on every no-match.
- **Edits.** `Edit` "performs exact string replacement … It doesn't use regex", and refuses an `old_string` that is missing or not unique ([Edit tool behavior](https://code.claude.com/docs/en/tools-reference.md#edit-tool-behavior)). That is already the "safer structured edit" an Oska `sed` would offer, with a uniqueness check `sed -i` does not have; agents used it 1,834 times against 1,254 `sed -i`. *Inference:* the reason agents still use `sed -i` and Python is batching (many edits in one call), which the tooling note covers.

## Part 5: where the value would be instead

- **Dogfooding and a forcing function.** Oska has no regular expressions and no JSON. No issue proposes a regex library (`gh issue list --search regex` finds none); `std.json` is #248, which is blocked behind the parity gate (#181) and the `Bytes` work (#209), and [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md) freezes new `std` APIs until then. A grep and a jq are small, well-specified programs with decades of expected behaviour to test against, the way [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md) chose TLS and git as "the language's largest test", which "leave behind libraries that real programs can use". A regex engine is a parser of untrusted input, the code that ADR says Oska exists for. *Inference:* a linear-time regex in `std`, with an `ogrep` example as its acceptance test, and a jq-like query over `std.json` as #248's, are worth doing for the language, after the gate. They would not make anyone faster.
- **A capability-confined edit tool.** An Oska `sed -i` would print its reach before it runs (`FsWrite -> DirWrite: restrict_to "."`) and refuse paths outside it, as the tooling note measured. *Inference:* the gain is reviewability of an agent's edit tool, but `Edit` is already confined by Claude Code's permission rules, and the `sed -i` and Python heredocs it would replace run inside the same unconfined Bash.
- **A performance target.** The 14 times gap between native Oska and GNU grep on a line scan (Part 3) is a concrete benchmark for the string runtime. [`scripts/bench-native.sh`](../../scripts/bench-native.sh) has a `bulk-text` workload (`replace` and `trim`) but no line scan.

## Part 6: cheaper alternatives, against the data

| Alternative | Addresses | Evidence |
| --- | --- | --- |
| Keep the embedded `ugrep`/`rg` and GNU `sed`/`jq` | speed | 2 to 16 ms a run (Part 3); already integrated with Edit and exit-code handling (Part 4) |
| Read `oskac --format=json` (with `jq` if filtering) instead of grepping text | failures, tokens | 699 `oskac` greps against 0 JSON-to-`jq` (Part 1) |
| Ask for less output: `grep -c`/`-l` before `-n`, `sed -n` ranges, `head` limits | tokens | text-only calls are 49.9% of `Bash` output (Part 1) |
| The throughput note's build slots, pass stamp, test-profile and polling fixes | wall-clock | 74% of `Bash` time is builds, tests and waits (Part 1) |
| Teach gawk-free `awk` and jq quoting (a jq program in a file, `jq -f`) in [`docs/agents/implementer.md`](../agents/implementer.md) | the 14 dialect failures | Part 2; *inference* on the fix |

## Part 7: the answer, and what would change it

**Would Oska implementations of grep, sed, awk and jq make development faster?** No. They would replace calls that are 2.4% of the agents' tool time, whose own run time is a few milliseconds of a 0.4 s call and a 4 s turn, with programs that run 14 to 280 times slower, cannot do regular expressions or JSON yet, and that Claude Code would not recognise as file views or as benign on exit 1. The failures they could prevent are about 14 in 15,270 calls.

**What would change this:**

1. Text tools becoming a real share of time: for example, if the build and wait work in the throughput note brought builds near zero and text calls rose above, say, a fifth of tool time. Today they are 2.4% against 74%.
2. A repository that agents work on from Windows or macOS without Claude Code's embedded tools, where GNU and BSD differences start to cost retries. Nothing in these transcripts shows that; every agent ran on Linux.
3. Claude Code letting a project register its own viewers and benign exit codes, so a new tool lost nothing against `grep` and `sed -n`. I found no such setting in the [tools reference](https://code.claude.com/docs/en/tools-reference.md) or [settings](https://code.claude.com/docs/en/settings.md).
4. An evaluation (#207) showing agents get the Oska form right first time more often than `jq`/`sed` quoting. The tooling note's companion probe found the opposite for scripts today.

**Smallest first step, if the maintainer wants the dogfooding:** add the 11-line fixed-string line scan from Part 3 as a `scan` workload in `scripts/bench-native.sh`, so the native string runtime has a number to beat (28 ms against GNU grep's 2 ms on 4.8 MB). It needs no new API and does not touch the freeze. After the parity gate, write the `std.regex` spec with `ogrep` as its acceptance test, and use a jq-like query as #248's.

## Not measured

- Wall clock at idle (all figures are at load 5.6 to 6.3), and the Oska numbers with a fresh release build, since I ran no cargo build (Part 3's unexplained 218 ms).
- How many agent turns were spent *reading* text-tool output rather than running it; durations are per call, not per turn.
- Silent wrong edits by `sed -i` or Python, which leave no error in a transcript.
- The classification is by pattern on the command text; read the counts as approximate. The script and its outputs are in the session scratchpad, which is not durable.

## All sources

| Source | Quality | Topic |
| --- | --- | --- |
| Claude Code transcripts under `~/.claude/projects/-home-pi-oska/` (408 files, 21,123 tool calls), parsed with Python | measured | Parts 1 and 2 |
| bash `time` on this Pi: GNU grep 3.11, embedded ugrep 7.8.4 and ripgrep 14.1.1, GNU sed 4.9, mawk 1.3.4, jq 1.7, Python 3.15.2, release `oskac` 0.3.0; `type grep` | measured | Parts 1 and 3 |
| Claude Code docs: [tools reference](https://code.claude.com/docs/en/tools-reference.md) (Bash, Edit, Read, Glob, Grep, PowerShell), [env vars](https://code.claude.com/docs/en/env-vars.md), [settings](https://code.claude.com/docs/en/settings.md) | primary | Embedded search tools, read-before-edit, exit codes, output limits |
| [GNU grep manual](https://www.gnu.org/software/grep/manual/grep.html), [GNU sed manual](https://www.gnu.org/software/sed/manual/sed.html), [POSIX sed](https://pubs.opengroup.org/onlinepubs/9799919799/utilities/sed.html), [FreeBSD sed(1)](https://man.freebsd.org/cgi/man.cgi?query=sed&sektion=1), [gawk manual, Array Sorting Functions](https://www.gnu.org/software/gawk/manual/html_node/Array-Sorting-Functions.html), [jq 1.7 manual](https://jqlang.org/manual/v1.7/), [ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md) | primary | Tool behaviour, exit status, portability |
| [`scripts/install/install.sh`](../../scripts/install/install.sh), [`install.ps1`](../../scripts/install/install.ps1), [`scripts/bench-native.sh`](../../scripts/bench-native.sh), [`prelude.osk`](../../crates/oska-check/src/prelude.osk), [`docs/guide/cli-reference.md`](../guide/cli-reference.md), `git log` | primary | Portability in shipped scripts, Oska's library, `--format=json` |
| [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md), [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md), `gh issue` #248, #181, #209, #207 | primary | Dogfooding precedent, the freeze, `std.json` |
| [`2026-11-oska-for-development-tooling.md`](2026-11-oska-for-development-tooling.md), [`2026-11-development-throughput.md`](2026-11-development-throughput.md), `2026-11-agent-scripting-python-vs-oska.md` (draft) | primary (this repository) | Model-turn time, `oskac` start-up, `check.sh` duration |
