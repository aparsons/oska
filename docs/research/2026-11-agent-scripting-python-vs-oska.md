# Research: should the agent write its throwaway scripts in Oska instead of Python?

- **Date:** 2026-11-15
- **Method:**
  - I extracted every Bash tool call from one Claude Code session on the trading app (`/home/pi/claude-alpaca-trader`), the transcript `~/.claude/projects/-home-pi-claude-alpaca-trader/b8d2f1ca-e5df-4f81-81b6-bd8151a53627.jsonl` (2,612 lines, 5.6 MB). I used `jq` to select the calls whose command contains `python3`, and I classified each one by hand. Each example below names its `tool_use` id.
  - I built `oskac 0.3.0` in release mode from commit `3cffd32` into a scratch target directory (`cargo build --release -p oskac`). I wrote small probe programs, ran them with `oskac check` and `oskac run`, and recorded the diagnostics exactly.
  - I timed every case on the same machine, a Raspberry Pi 5 (4 cores, Linux 6.18, Python 3.15.2, jq 1.7). The machine was busy: the load average was 16 to 19 during the runs. So I give CPU time (user plus system, from bash `time`) as the main number, because load changes it less than wall time. Each figure is the median of 21 runs.
  - The only other sources are the Oska source and docs at `3cffd32`, `git help gitattributes`, and the Claude Code tool definitions as this session receives them. Statements that are my reasoning and not a measurement are marked *inference*.
- **Question:** "I see Claude using a lot of Python. Would Claude be faster or more effective if it used Oska?"
- **Related research:** [AI-driven development](2026-11-ai-driven-development.md) covers tooling for agents that *write Oska programs*. This note is about the reverse: Oska as the language an agent uses for its own one-off scripts.

## Summary

**No, not today, for either speed or effectiveness. Yes, later, for one class of use, if Oska gains four features.**

- **Speed does not depend on the language.** An Oska program on the interpreter starts with less CPU than Python: 3 ms against 21 ms. But the agent spends about 6.8 s of model time to write each Python call and about 1 s to run it, so a 20 ms difference is under 0.3% of a cycle. The native backend is slower than Python for a one-off script, because `oskac run` compiles first: 48 to 80 ms of CPU.
- **Oska cannot do 7 of the 29 Python uses at all.** 6 parse JSON, and Oska has no JSON parser, no map type and no floating-point numbers. `jq`, which is installed, covers them. The seventh needs SQLite, which does not run yet.
- **Oska can do the other 22 (file edits and conflict resolution), but each program is 2 to 6 times longer.** It also costs more turns: the model does not know Oska, and my 7 programs needed 3 extra drafts. 2 drafts failed the check, and 1 gave a wrong answer with exit code 0.
- **Oska's one clear advantage is safety, and it is real.** A Python heredoc runs with all of the user's authority. An Oska script declares `fs: Fs`, narrows it to `.`, and the runtime refuses a path outside it. `oskac check` prints that bound before the script runs.
- **Recommendation:** move JSON inspection to `jq`. Keep one-off text edits on the Edit tool or Python. Dogfood Oska for one *stored, reused* tool: the CHANGELOG conflict resolver, which I wrote and ran in Oska during this research.

## 1. What the Python use actually is

The session made 226 Bash calls. 29 of them (13%) run Python. The agent made only 3 Edit calls and 2 Read calls in the same session. Every one of the 29 Python calls exited without an error, so Python cost no retries.

| Class | Calls | Share | What the code does |
| --- | --- | --- | --- |
| A. Exact text edits in files | 19 | 66% | Read a file, `assert old in s`, `s.replace(old, new, 1)`, write it back. About 56 replacements over about 35 file touches; 6 calls edit 2 to 7 files each. |
| B. JSON from an API answer or a config file | 6 | 21% | `json.load(sys.stdin)`, then count, filter keys, or walk the tree. |
| C. Git conflict resolution | 3 | 10% | A `re.S` regex over `<<<<<<<`, `=======`, `>>>>>>>`, keeping both sides of a CHANGELOG conflict. |
| D. Other | 1 | 3% | An in-memory `sqlite3` database to read `EXPLAIN QUERY PLAN`. |

The 19 edit calls hold 40,562 bytes of command text in all. The edit calls are the large ones: 6 to 99 lines, 431 to 4,386 bytes each.

Examples:

- **A.** `toolu_01TbM5HwWF2mvMBvwnCf9Gw6` replaces a 13-line TypeScript function in `app/src/operator/cost.ts` and one sentence in `app/src/operator/evidence.ts`, with an `assert` before each. `toolu_01K4ShHnizECBZXX9V9EiqWA` defines a `rep(p, old, new)` helper and edits 4 files. `toolu_01MRrKPy3gLuGetbyAfdnWSS` edits the runbook, an incident note and `CHANGELOG.md` in one call.
- **B.** `toolu_01XLqECPaHhSJmBwurG4Ku4B` pipes `agent-read` into `python3 -c 'import sys,json;d=json.load(sys.stdin);print(type(d).__name__, len(d)); print(sum(len(o.get("fills",[])) for o in d), "fills")'`. `toolu_01KgQXBKcqgNv9smfhW5d8W6` walks the whole strategy comparison and prints keys named `cost`, `holder` and others. `toolu_012cxUt5ArdqeWgPQcrAfWEK` searches `~/.claude.json` for strings that contain "oska".
- **C.** `toolu_01STXFQfnmhS9koopCRibkzh` and `toolu_01R2s1wm7Dxx5UYGBNXfEP8g` run the same five lines on `CHANGELOG.md` during a rebase. `toolu_01AuePhdjmkdT4iDfuJe9Rso` keeps one side of a conflict in a test file.
- **D.** `toolu_013yT6dczcTTKXXjwUoxgzFw` checks that a `json_each(?)` query uses an index.

*Inference:* the agent chooses Python for class A because the Edit tool must have a prior Read of the file ("You must Read the file in this conversation before editing, or the call will fail") and does one replacement per call. One Python heredoc does many replacements in many files in one turn, with no Read.

## 2. Can Oska do each class today?

| Need | Oska 0.3.0 | Source |
| --- | --- | --- |
| Read stdin | Yes, line by line: `Stdin.read_line` returns `Result<Option<Untrusted<String>>, IoError>` | `crates/oska-check/src/prelude.osk:27-32`; `docs/guide/capabilities.md:9` |
| Read and write files | Yes, through directory and file capabilities | `docs/guide/what-works-today.md:24`; `prelude.osk:110-160` |
| Exact string replace | Yes, but it replaces *every* occurrence. There is no "replace once", no `find` or `index_of`. | `prelude.osk:332-335` |
| Regex | No. `String` has `lines`, `split`, `contains`, `starts_with`, `ends_with`, `replace`, `join`, `chars`, `slice`, `trim`, `parse_int`, and no more. | `prelude.osk:311-360` |
| JSON | No. `std.json` is named in the design but does not exist; issue #248 "Agents: `std.json`" is open. | `docs/design.md:157`; `gh issue view 248 -R aparsons/oska` |
| Maps, floats | No map type in the prelude. No float: `let x = 1.5` gives `E0101 expected a name after '.', found '5'`. | `prelude.osk:294-307`; probe below |
| Run git, gh or a process | No: `std.proc` and `Spawn` check but do not run (`E0371`, `E0370`). | `what-works-today.md:29`; `capabilities.md:16` |
| String concatenation with `+` | No, on purpose: `E0223`. Use `"{a}{b}"`. | `oskac explain E0223` |
| Inline program, no file | Partly. `oskac run` needs a path (`cli-reference.md:95`). `-` and `-c` do not exist. `/dev/stdin` and bash process substitution work. | probe below |

### Probes

All probes ran in a scratch directory. The outputs are verbatim.

**Inline.** `oskac run /dev/stdin <<'EOF' ... EOF` runs the program. But then stdin holds the program, so the program cannot also read piped JSON. Process substitution keeps stdin free, and this worked: `cat orders.json | oskac run <(cat count.osk)` printed `200 orders with a fills key`. `oskac run -` gives `error: cannot read -: No such file or directory (os error 2)` and exits with 1.

**Class A, edit.** I ported the `cost.ts` edit of `toolu_01TbM5HwWF2mvMBvwnCf9Gw6` (one replacement). It checked and ran first time, and the result matched Python's:

```oska
fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let dir = fs.restrict_to(".")
    let p = Path.new("target.ts")
    let s = dir.read_only().read_to_string(p)?
    let old = "\t\treturn \{ bp: Math.max(0, round2(slippageBp)), value: \"measured\", fills \};\n"
    let new = "\t\tconst bp = round2(slippageBp)\n\t\tif (bp >= EVIDENCE_COST_BP_PER_SIDE) \{\n\t\t\treturn \{ bp, value: \"measured\", fills \};\n\t\t\}\n"
    if !s.contains(old) {
        io.println("old text not found")?
        return Ok(())
    }
    dir.write_only().write(Path.new("target.ts"), s.replace(old, new))?
    Ok(())
}
```

It is 13 lines and 606 bytes, against 4 lines and 316 bytes in Python. The cost is in the string literals. Oska has no raw or triple-quoted string. A literal resolves only `\n \t \r \0 \\ \" \{ \}` (`crates/oska-syntax/src/lib.rs:370-381`), and `{` starts interpolation. TypeScript and JSON are full of both. The 19 edit calls of class A hold 261 braces and 280 double quotes that Oska would need escaped.

An unescaped brace is a hazard, not only a cost. With `{ bp: 1 }` the check stops: `E0102 string interpolation only accepts a name or field path such as {user.name}, found { bp: 1 }`. But when the brace holds one name that is in scope, the program runs and substitutes it with no warning:

```console
$ cat unesc2.osk
fn main(io: Stdout) -> Result<(), IoError> {
    let fills = "SILENT"
    let old = "const x = {fills};"
    io.println(old)?
    Ok(())
}
$ oskac run unesc2.osk
const x = SILENT;
```

In an edit, that `old` would not match, so the edit fails safely. In a `new` text it writes wrong code into the file.

**Class B, JSON.** Oska cannot parse JSON, so I wrote the nearest program: read stdin, count a marker. My first draft failed the check with `E0223: '+' is not defined for 'String'`. My second draft ran and printed `0 orders with a fills key`, with exit code 0. The input had `"fills": [` with a space, and my marker had none. Python's `json.load` gave the true answer, `200 199` (200 orders, 199 fills). A text search for a key is not a JSON parser, and this failure was silent. The `fills` sum, the tree walk of `toolu_01KgQXBKcqgNv9smfhW5d8W6`, and every decimal price are out of reach until Oska has JSON, a map and a float.

**Class C, conflicts.** I wrote a resolver that keeps their side, then ours, for every conflict in a file. My first draft failed with `E0101 expected ')', found '{'`, because I wrote a Rust-style struct literal `IoError { message: ... }`. Oska uses call-style constructors, `IoError(message: ...)` ([ADR 0004](../adr/0004-call-style-constructors.md)). The second draft checked and ran correctly: 32 lines and 1,103 bytes, against the 5 lines and 196 bytes of Python in `toolu_01STXFQfnmhS9koopCRibkzh`. The Oska version does more: it resolves every conflict in the file, and the Python resolves only the first. With a path outside its root it stopped: `` `../CL.md` leads outside the capability's root ``, exit code 1.

**Class D, SQLite.** `std.sql` checks but does not run (`what-works-today.md:28`), so no.

| Class | Oska today | Better tool today |
| --- | --- | --- |
| A. Text edits (19) | Yes, with escaping, and 2 times the length | Edit tool for one or two edits; Python for a batch |
| B. JSON (6) | No | `jq` |
| C. Conflicts (3) | Yes, as a stored program; 6 times the length of the Python | A stored Oska tool, or git's `union` merge driver |
| D. SQLite (1) | No | Python, or the `sqlite3` command |

## 3. Speed on this Pi

CPU time, median of 21 runs. Wall time was noisy because of the load, so it is in brackets as the minimum.

| Case | CPU ms | Wall ms (min) |
| --- | --- | --- |
| `bash -c true` (the cost of the harness) | 2 | 2 |
| `python3 -c pass` | 24 | 22 |
| `python3 count.py` (`json.load` of 30 KB from stdin) | 41 | 37 |
| `python3 replace.py` (the class A edit) | 24 | 37 |
| `jq '[.[].fills\|length]\|add'` on the same 30 KB | 5 | 5 |
| `oskac check` (count program) | 4 | 3 |
| `oskac run --interpret` (empty `main`) | 4 | 5 |
| `oskac run --interpret` (count program) | 5 | 4 |
| `oskac run --interpret` (class A edit) | 6 | 5 |
| `oskac run` native (empty `main`) | 48 | 78 |
| `oskac run` native (count program) | 57 | 57 |
| `oskac run` native (class A edit) | 80 | 78 |
| `oskac build` output run directly (count program) | 3 | 2 |

Findings:

- For a one-off script, the interpreter is the fast backend: 4 to 6 ms of CPU, about one fifth of Python's 24 ms. `oskac run` picks the native backend by default on aarch64 Linux (`cli-reference.md:148`), and that is 11 to 13 times more CPU here, because it compiles before it runs. Native pays only for a program that is built once and run many times.
- The interpreter falls behind on large input when the program is written naively. With a 237 KB, 14,646-line JSON file, my count program that built one string line by line took 278 ms of CPU on the interpreter and 156 ms native, against Python's 44 ms. A version that tests each line and keeps no string took 21 ms on the interpreter.
- None of this matters to the agent. From the transcript timestamps, the median time from the previous tool result to each Python tool call was 6.8 s (2.5 to 18.4 s), and the median run of the whole Bash call was 1.0 s. These are *estimates*: the model writes the call in that gap, but the gap also holds other output. Against a cycle of about 8 s, Python's 20 ms of extra start-up is 0.25%.

### Turns and tokens

The cost that counts is model output and retries. *Inference* from the probes above:

| | Python | Oska 0.3.0 |
| --- | --- | --- |
| Length of the class A edit | 316 bytes | 606 bytes (1.9 times) |
| Length of the class C resolver | 196 bytes | 1,103 bytes (5.6 times) |
| Length of the class B count | 93 bytes | 364 to 400 bytes (4 times), and it does not parse |
| Failed drafts | 0 of 29 calls in the session | 3 extra drafts for 7 programs in this research: 2 diagnostics, 1 silent wrong answer |
| Reading before the first program | none | I read the prelude (464 lines, 15.7 KB) and parts of 5 guides |

Each failed draft costs one more model turn, which is about 7 s and the full program text again. So on this sample, Oska costs more turns and more tokens than Python for every class.

## 4. Effectiveness for the agent

**Familiarity.** Oska's first commit is dated 2026-11-08 (`git log --reverse`). The model's training data ends before that, so it knows Oska only from what it reads in the session. My two diagnostic failures were habits from other languages: `+` on strings and a Rust struct literal. Python's failure rate in the session was zero.

**Error recovery.** Oska's diagnostics are good for a machine. Each one has a stable code and a byte span, and `oskac explain` gives an example and the fix offline. 36 of the 147 codes in the catalog have a machine-applicable fix (from `oskac explain <code> --format=json`, field `machine_fix`). None of the diagnostics that I hit had one: `E0223`, `E0101`, `E0102`, `E0221` and `E0240` came with no `fix` object. Python's traceback names the file, the line and the exception, which was enough here. The worst failure in this research had no diagnostic at all: the silent `0` of the JSON count. That is a property of text matching, not of Oska, and it is why JSON belongs in a parser.

**Safety.** This is Oska's real advantage:

- A Python heredoc has all of the user's authority. In this session one of them read `~/.claude.json` (`toolu_012cxUt5ArdqeWgPQcrAfWEK`). The same code could read the trading app's API keys.
- An Oska script declares what it can do in `main`, and `oskac check` prints it before anything runs, for example `fn main: Stdout (io), Fs (fs)` and `Fs -> Dir: restrict_to "."`. A path that leaves the root is refused at run time (`` `../target.ts` leads outside the capability's root ``). It cannot start a process or open a socket (`what-works-today.md:29-30`).
- The limit of this advantage: the agent's Bash tool already has full authority, so a confined script does not confine the agent. *Inference:* the gain is for review. A permission hook could approve `oskac run` for a program whose authority report shows only `Fs` narrowed to the working tree and `Stdout`, and ask about everything else. A hook cannot read a Python heredoc that way.

**What the agent already has.**

- `jq` 1.7 is installed and covers class B. I ran two of the uses as one expression each: `jq -c 'length, ([.[].fills|length]|add)'` printed `200` and `199`, and `jq -c '[paths(..) as $p | select($p[-1]=="holder") | $p]'` walks the tree like `toolu_01KgQXBKcqgNv9smfhW5d8W6`. The session already used `jq` in 7 Bash calls, most of them through `gh --jq`.
- The Edit tool does an exact replacement and refuses an `old_string` that is not unique. That is stricter than Python's `assert old in s`, which does not check uniqueness. It needs a Read first and does one edit per call, which explains why the agent batched edits in Python.
- For class C, git has a built-in merge driver: "union: Run 3-way file level merge for text files, but take lines from both versions, instead of leaving conflict markers" (`git help gitattributes`). A `CHANGELOG.md merge=union` line in `.gitattributes` would remove the conflicts. The same manual says the order of the added lines can be random, so the result needs a check.

## 5. Conclusion

**Faster?** No. Start-up time is not the bottleneck: the interpreter saves about 20 ms of CPU on a cycle of about 8 s. The native default costs 16 to 56 ms more CPU than Python. Model time decides, and Oska makes longer programs with more retries.

**More effective?** Not today. Oska cannot do class B or D (24% of uses). It can do class A and C, but with 2 to 6 times the text and a real risk of silent interpolation. It is safer, and that is the only axis where it wins.

**Per class, now:**

| Class | Move to | Why |
| --- | --- | --- |
| B. JSON (6) | `jq` | Shorter than Python, 5 ms, already installed |
| A. Text edits (19) | Edit tool for 1 or 2 edits; keep Python for batches | The Edit tool checks uniqueness; Oska's literals need escaping |
| C. Conflicts (3) | A stored Oska tool (dogfooding), or `merge=union` | The same 5 lines ran 3 times. A stored program is written once, so its length does not matter, and its authority is printed. |
| D. SQLite (1) | Keep Python | `std.sql` does not run |

**What Oska would need to compete for agent scripts.** In order of effect:

1. **`std.json`** with a dynamic value, a map type and floating-point numbers (issue #248). Without them Oska cannot read an API answer, and 21% of the uses are exactly that.
2. **A raw string literal** with no escapes and no interpolation, for example a multi-line form. It removes the escaping cost and the silent-interpolation hazard in class A. *Inference:* this is the largest single gain for edits.
3. **`oskac run -` or an `-e` flag**, so that a program can come from an argument and stdin stays free for data. Process substitution works today but is easy to get wrong.
4. **An interpreter-first `run` for small programs**, or a compile cache. Native `run` costs 48 to 80 ms of CPU per call, 11 to 13 times the interpreter.
5. **Text functions:** `find` or `index_of`, `replace_once` (or a count), and a regex module. Class C needs a hand-written state machine today.
6. **`std.proc` at run time**, so a script can run `git` or `gh`. It checks today but does not run.
7. **A one-page scripting cheat sheet** for agents: `main`'s capabilities, `restrict_to(".")`, stdin, `"{a}{b}"` and the constructor syntax. My two diagnostic failures were exactly those habits. The prelude, at 15.7 KB, is too long to read for a 10-line script.

**Dogfooding proposal.** Keep the conflict resolver as a stored Oska program in the trading repo's tooling, and use it for the next CHANGELOG rebase. It is reused, it touches files only under the working tree, and its authority report is short. Do not move inline JSON or batch edits to Oska until items 1 to 3 exist. Then repeat this measurement on a new session.

## Sources

| Source | Kind | Used for |
| --- | --- | --- |
| Session transcript `b8d2f1ca-e5df-4f81-81b6-bd8151a53627.jsonl`, extracted with `jq` | primary (measurement) | The 29 Python uses, their classes, errors and timestamps |
| `crates/oska-check/src/prelude.osk` at `3cffd32` | primary | Stdin, files, `String` methods, no map or float |
| `crates/oska-syntax/src/lib.rs:370-381` | primary | The escapes a string literal accepts |
| `docs/guide/what-works-today.md`, `docs/guide/cli-reference.md`, `docs/guide/capabilities.md`, `docs/guide/agent-guide.md`, `docs/design.md`, ADR 0004, ADR 0008 | primary | What runs, how `run` works, the capability model |
| `oskac 0.3.0` probes and timings on this Raspberry Pi 5 | primary (measurement) | Diagnostics, outputs, CPU and wall times |
| [aparsons/oska#248](https://github.com/aparsons/oska/issues/248) | primary | `std.json` is planned and open |
| `git help gitattributes`, git 2.47.3 | primary | The `union` merge driver |
| Claude Code's Edit tool definition, as given to the agent in this session | primary | Read-before-Edit and the uniqueness rule |
