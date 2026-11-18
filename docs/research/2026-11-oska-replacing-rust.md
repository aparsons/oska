# Research: how long until Oska can replace Rust implementations?

- **Date:** 2026-11-17
- **Snapshot:** `main` at `fc411f9` (2026-11-16, merge of PR #267), `oskac` 0.4.0 (tag `v0.4.0`, 2026-11-15). Every "as of" below means this commit unless another is named.
- **Method:**
  - **Read here.** `README.md`, `GLOSSARY.md`, `docs/design.md`, `docs/runtime.md`, `docs/guide/what-works-today.md`, ADRs 0011 to 0014, the four research notes named below, `site/build.osk`, `site/check-links.osk`, `examples/`, and the prelude (`crates/oska-check/src/prelude.osk`, 464 lines).
  - **Issue tracker** (`gh`, 2026-11-17): #3, the open specs and tickets labelled `ready-for-agent`, and every closed issue with its close date.
  - **Delivery rate** from `git log --merges --since=2026-11-01` and issue close dates. No build or test was run: the Pi's CPU was shared with two implementers.
  - **Counted here.** Lines of Rust per crate (`find crates/<c>/src -name '*.rs' | xargs cat | wc -l`) and uses of Rust features Oska lacks (`grep` over `crates/*/src`, test directories excluded).
  - Statements that are my reasoning and not a source's or a measurement are marked *inference*. Every estimate in Part 4 is inference built on the measured rate.
- **Question (the maintainer's, 2026-11-17):** "How long until Oska can be used to replace Rust implementations?"
- **Builds on:** [`2026-11-development-throughput.md`](2026-11-development-throughput.md), [`2026-11-oska-for-development-tooling.md`](2026-11-oska-for-development-tooling.md), [`2026-11-oska-text-tools.md`](2026-11-oska-text-tools.md), [`2026-11-agent-scripting-python-vs-oska.md`](2026-11-agent-scripting-python-vs-oska.md). It does not repeat their measurements, only cites them.

## Summary

The question has two readings:

- **A. Self-hosting:** replace this repository's own Rust (`crates/`, 47,981 lines of non-test Rust and 48,962 lines of Rust tests) with Oska.
- **B. User programs:** replace Rust for the CLIs, services and tooling that users write.

**Reading B is the one that matters, and its CLI half is the near target.**

- ADR 0011 calls self-hosting "a goal for later, not a prerequisite". No issue or spec mentions it as of 2026-11-17. Every open language spec targets programs users write: #209, #227, #228, #229 and #230.
- The interpreter in Rust is the oracle the native backend is tested against ([ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)). A port would take away that independent reference.
- Every gap that blocks A also blocks B-CLI. So B-CLI is on the path to A anyway.

| Reading | What "replace" means | Specs to go (as of 2026-11-17) | Estimate (calendar) | Confidence |
| --- | --- | --- | --- | --- |
| **B1. CLIs and dev tooling** (text, files, processes, JSON) | A Rust CLI of that kind could be written in Oska instead, on all five platforms | 9 to 11 (2 filed and open, the rest unfiled) | **2026-12-10 to 2027-01-31** | Medium-low |
| **B2. Networked services** (HTTPS client, a server, a database, dependencies) | A small Rust service could be written in Oska | B1 plus 8 to 11 | **2027-02 to 2027-05** | Low |
| **B3. Rust in general** (speed within ~2x, FFI, an ecosystem of packages) | Choosing Oska over Rust is not a trade-off on speed or libraries | Not on any roadmap | **Not before 2027 H2; not estimable from throughput** | Very low |
| **A. Self-hosting `oskac`** | Every crate in `crates/` rewritten in Oska, with a bootstrap story | B1 plus 13 to 19 | **2027-03 to 2027-07, and only if it is scheduled** (nothing schedules it as of 2026-11-17) | Low |

**Top three blockers, both readings:**

1. **The language freeze.** ADR 0014 freezes all new syntax, types, prelude, `std` APIs and capabilities until macOS (#154) and Windows (#155) run natively and the parity gate (#181, v0.6.0) turns on. #154 was filed on 2026-11-13 and had no merged ticket by 2026-11-17.
2. **No binary data or bit operations.** `Bytes`, bitwise operators and conversions between integer types (#209) are missing. They block crypto, TLS, binary files and any code generator, including a self-hosted `oska-native`.
3. **A thin standard library.** There is no map or set, no float, no `List` sort, filter or contains, and no regex or JSON. `std.proc` checks but does not run (E0371), and there is no file append, delete, rename or stat. These gaps are recorded in the [tooling note](2026-11-oska-for-development-tooling.md), Part 2.

## Part 1: what Oska could replace as of `fc411f9` (2026-11-16)

**What runs.** The [what-works-today table](../guide/what-works-today.md) lists what runs on both the interpreter and native code (static executables for aarch64 and x86_64 Linux, built from any host):

- the core language: structs, enums, `match`, generics, traits, closures and lists;
- contracts, multi-module packages and `test` blocks;
- `Stdout`, `Stderr`, `Stdin`, `Args`, `Env` and `Secret`, `Clock` and `Random`;
- confined files;
- structured concurrency (tasks run one at a time, [design.md "Concurrency in v0"](../design.md));
- `Untrusted` text and `std.html`.

**What only checks.** `std.sql`, `std.proc` and `Net` check but are refused before they run (E0371, E0370).

**Real evidence that it works.** The project's own website is an Oska program:

- [`site/build.osk`](../../site/build.osk) is 2,590 lines and [`site/check-links.osk`](../../site/check-links.osk) is 415 lines, both as of 2026-11-17.
- The Pages workflow builds and runs both natively ([tooling note](2026-11-oska-for-development-tooling.md), Part 1).
- Natively, the generator builds the site in 104 ms on the Pi, against 2,226 ms on the interpreter ([design.md, "Native backend benchmark"](../design.md), 2026-11-12, `oskac` 0.2.0).

It also shows the gaps. `check-links.osk` keeps its sets "as one text with an item per line" because there is no map or set ([tooling note](2026-11-oska-for-development-tooling.md), Part 2). #88 (open) asks to split the 2,590-line generator into modules.

**So as of 2026-11-17, Oska can replace a Rust program that:**

- reads and writes text files under a directory;
- transforms text with `split`, `replace`, `contains` and the like;
- takes arguments and environment variables, and prints;
- needs no hash map, no float, no binary data, no processes and no network;
- runs on Linux, or on the interpreter elsewhere.

Static generators, linters, link checkers and text reformatters fit that description. `examples/shopping` (33 lines in 3 modules) and the eval tasks under `eval/tasks/` (`least-authority-line-count`, `command-injection-*`) are in the same class.

**It cannot replace, as of 2026-11-17:**

- anything that spawns a process or uses the network;
- anything that parses JSON or binary formats, or does floating-point maths;
- anything that needs a dictionary, unless it works around the gap as `check-links.osk` does;
- anything that needs native speed on macOS or Windows;
- anything that needs a third-party package: there is no dependency mechanism yet, design milestone 5 and #12 (`needs-triage`).

**Speed against Rust-class tools** (measured in the earlier notes):

- **Program logic.** Native Oska runs it 46 to 83 times faster than the interpreter ([design.md](../design.md), 2026-11-12). No Oska-against-Rust benchmark exists.
- **One string workload.** A fixed-string line count over 4.8 MB took 28 ms as a native Oska executable against 2 ms for GNU `grep` ([text-tools note](2026-11-oska-text-tools.md), 2026-11-15).
- **The optimizer.** [design.md](../design.md) says that going further "would take a real optimizer (appending in place to a text nothing else holds, keeping values in registers across calls)".

## Part 2: delivery rate (measured)

The repository starts at `a4ef823` on 2026-11-08.

**Merges.** `git log --merges --since=2026-11-01` gives 257 merge commits from 2026-11-08 to 2026-11-16: 54, 37, 24, 11, 13, 15, 29, 54 and 20 per day. Most are ticket-into-spec merges. Only 3 are GitHub PR merges, all on 2026-11-16 (#263, #267, #269).

**Issues and PRs.**

- 178 issues were closed from 2026-11-09 to 2026-11-17, about 20 a day, mostly tickets.
- `gh pr list --state merged`: 23 PRs were merged on 2026-11-15 and 17 on 2026-11-16, after the PR-per-ticket trial began (spec #191).

**Specs.** 16 specs were closed in 9 days, **1.8 a day**:

- #21, #32 and #49 (2026-11-09);
- #59, #93 and #105 (11-10);
- #115 (11-11);
- #125 (11-13);
- #208 (11-14);
- #153, #183, #184, #191 and #280 (11-15);
- #318 (11-16);
- #185 (11-17).

**Heavy specs** (language, runtime or backend, the kind every gap below needs) are a slower series:

- 9 of the 16 are heavy. 6 of those closed in the first 3 days, while the code base was new.
- Since 2026-11-11, three have closed in 7 days, **about 0.4 a day**: #115, #125 and #153.
- #125 (aarch64 backend, 16 tickets) ran 2026-11-11T17:34Z to 11-13T00:10Z, **31 hours**.
- #153 (x86_64 backend, 10 tickets) ran 2026-11-13T01:53Z to 11-15T07:37Z, **54 hours**.
- Since 2026-11-15 the open heavy specs (#154, #155) have waited behind tooling, CI and site specs (#185, #207, #280, #318). #263 merged one Windows ticket (#175) on 2026-11-16.

**What limits the rate.**

- The Pi has 4 cores, and CPU is the binding constraint ([throughput note](2026-11-development-throughput.md), 2026-11-15). Two build slots means about two implementers at a time.
- Unfiled specs need design and a maintainer decision first. *Inference:* that adds about half a day to a day each, partly in parallel with other work.
- After the gate, every feature must land natively on five platforms, and macOS and Windows feedback comes only through CI (ADR 0014, "Consequences"). *Inference:* each heavy spec then costs 1.3 to 1.5 times what it costs today.

**Rate used for the estimates.** *Inference:* **0.4 to 0.8 heavy specs a day** before the gate, and **0.3 to 0.6** after it. The low end is the rate measured since 2026-11-11. The high end assumes heavy specs get priority and two of them run in parallel.

## Part 3: ranked gaps, sized in specs

"Filed" means an open spec exists as of 2026-11-17. "Unfiled" means no issue exists, and the size is my estimate from comparable closed specs. One spec is roughly the size of #115 or #209: 6 to 16 tickets.

### Reading B: user programs

| # | Gap | Kind | Blocks | Specs | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | **Language freeze until the parity gate** | platforms | everything below | 2 | Filed: #154 (8 tickets, 0 merged), #155 (8 tickets, 1 merged via #263), ending at #181 |
| 2 | **`Bytes`, bitwise and shift operators, wrapping arithmetic, integer conversions, secret integers** | language | binary files, crypto, TLS, codecs, hashing | 1 | Filed: #209 (tickets #220 to #226) |
| 3 | **Collections:** map and set types, `List` sort, filter, contains and remove, iteration helpers. The prelude's `List` has only `new`, `push`, `len`, `count`, `get`, `at` and `map` (`prelude.osk`, as of 2026-11-17) | std | almost every real CLI; the site works around it | 1 to 2 | Unfiled |
| 4 | **`std.proc` runtime** and file operations (append, delete, rename, stat, exists) | std, runtime | build tools, git wrappers, anything that orchestrates | 1 to 2 | Unfiled. `std.proc` has only signatures (E0371) |
| 5 | **`std.json` and a regex module** | std | config, APIs, log tools | 1 to 2 | #248 is filed, but only as a ticket inside #229. Regex is unfiled |
| 6 | **Floats and number formatting; raw or multi-line string literals** | language | numeric tools, emitting JSON or code (each `{` must be escaped: [Python note](2026-11-agent-scripting-python-vs-oska.md) §2) | 1 | Unfiled |
| 7 | **A formatter** (`oskac fmt`). [design.md](../design.md) "Toolchain" promises "deterministic, with a single canonical style", but `cli-reference.md` has no `fmt` as of 2026-11-17 | tooling | team adoption, agent diffs | 1 | Unfiled |
| — | *B1 subtotal (gaps 1 to 7)* | | | **9 to 11** | |
| 8 | **Crypto and a TLS 1.3 client** | std (in Oska, [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)) | HTTPS clients, package fetch, self-update | 2 | Filed: #227 (6 tickets), #228 (10 tickets) |
| 9 | **A server:** a non-loopback listener and HTTP serving. ADR 0012 excludes a TLS server, so a service would sit behind a proxy (a decision for the maintainer) | std, runtime | every service | 1 to 2 | Unfiled. #228 gives `NetListen` loopback only |
| 10 | **Parallel execution or non-blocking I/O.** Tasks "run one at a time" ([design.md](../design.md), "Concurrency in v0") | runtime | services under load | 1 to 2 | Unfiled ("Parallel execution and `Sched` channels come later") |
| 11 | **Packages with dependencies:** git fetch, minimal version selection, lockfile, authority approval | tooling, ecosystem | any reuse of code across repositories | 2 | Unfiled. Design milestone 5; #12 is `needs-triage` and needs the TLS client |
| 12 | **A runtime for `std.sql`:** a database driver, written in Oska or reached through FFI, which is "later" ([design.md](../design.md), "Unsafe and FFI") | std, FFI | database-backed services | 1 to 2 | Unfiled |
| — | *B2 subtotal (gaps 8 to 12)* | | | **8 to 11** on top of B1 | |
| 13 | **An optimizer:** register allocation across calls, in-place appends | performance | Rust-class speed (28 ms against grep's 2 ms) | 2+ | Unfiled |
| 14 | **C FFI, a language server (#275, `needs-triage`), a debugger, and third-party packages** | tooling, ecosystem | general replacement of Rust | open-ended | Mostly unfiled. Ecosystem growth depends on adoption, not on this repository's throughput |

### Reading A: self-hosting `oskac`

All of B1 is needed. On top of it (sizes are *inference*):

| # | Gap | Evidence (as of 2026-11-17) | Specs |
| --- | --- | --- | --- |
| A1 | **Map and set types**, heavily used | 87 `HashMap`/`BTreeMap` and 45 `HashSet`/`BTreeSet` lines in `crates/*/src` | Covered by gap 3 |
| A2 | **`Bytes` and bit operations for the code generator.** `oska-native` (10,988 lines) encodes aarch64 and x86_64 instructions and writes ELF | gap 2, #209 | Covered |
| A3 | **No trait objects** (`dyn`). Code must be redesigned around enums | 86 `dyn` lines in `crates/*/src`; [design.md](../design.md) "Traits in v0": "There are no trait objects" | Part of each port spec |
| A4 | **Running and marking executables.** `oskac run` and `oskac test` build into a temporary directory and execute the result ([runtime.md, "Native builds"](../runtime.md)). That needs `std.proc` plus a way to set the executable bit and delete temporary directories | gap 4, plus a capability decision | 1 |
| A5 | **A bootstrap ADR:** how `oskac` is built from source without Rust (a seed binary or a kept Rust build), reproducibility, and whether the Rust interpreter stays as the differential oracle (ADR 0014 calls it "the reference, not a fallback") | No issue or ADR as of 2026-11-17 | 1 |
| A6 | **Compiler speed.** A self-hosted `oskac` runs on Oska's own optimizer. A 5 to 6x slower debug-built `oskac` already hurts the test suites ([tooling note](2026-11-oska-for-development-tooling.md), Part 3) | gap 13 | 1 to 2 |
| A7 | **The port itself:** `oska-syntax` 5,369 lines, `oska-check` 16,668, `oska-run` 5,403, `oska-native` 10,988, `oskac` 6,633, `oska-eval` 1,636, and `oska-sys` 1,284 (becoming `std.rt` per [ADR 0013](../adr/0013-system-calls-only-in-std-rt.md)). Plus 48,962 lines of Rust tests to port or to drive from outside | Counted here | 10 to 14 |
| — | *A subtotal on top of B1* | | **13 to 19** |

## Part 4: estimates

### Assumptions

1. The rate is **0.4 to 0.8 heavy specs a day** until the gate, and **0.3 to 0.6** after it (Part 2).
2. The parity gate starts in earnest within a few days of 2026-11-17. #154 and #155 together are about 3 heavy-spec equivalents: macOS counts about double, being two platforms, Mach-O and signing. *Inference:* the gate (v0.6.0) lands **2026-11-24 to 2026-12-05**.
3. Unfiled specs are written as earlier ones fit in, with one maintainer decision each and no long stalls.
4. No new rule of the size of ADR 0014 (a second freeze) arrives, and no milestone is redone.
5. The Pi stays the only build machine with two slots, and CI covers the other platforms.
6. The maintainer's priorities do not change. As of 2026-11-17 they are platforms, then Bytes, crypto, TLS, agents and self-update (#209, then #227 to #230).

The four estimates follow. Each "+N days" counts from the end of the item before it.

### B1: CLIs and dev tooling

- **Remaining work after the gate:** 7 to 9 specs, at 0.3 to 0.6 a day, which is 12 to 30 days.
- **Estimate: 2026-12-10 to 2027-01-31.**
- **Confidence: medium-low.** The work is mostly library-shaped, and the platform work is filed. Most of it is unfiled, though. The planned order also puts crypto, TLS and agents (#227 to #230, 4 specs) before the collections, `std.proc` and JSON that CLIs need. Following that order adds about 1 to 2 weeks, which the upper bound includes.

### B2: services

- **Remaining work after B1:** 8 to 11 specs, at the same rate, which is 14 to 37 days.
- **Estimate: 2027-02 to 2027-05.**
- **Confidence: low.** The server, parallel execution, a database driver and dependency management are all unfiled, and each needs design decisions. ADR 0012 excludes a TLS server, which shapes what a service can be.

### B3: Rust in general

- **Estimate: none.** Two of the three conditions are not on any roadmap: speed within about 2x of Rust, and FFI. The third, a package ecosystem, depends on outside adoption, not on this repository's rate.
- **Bound:** not before 2027 H2.
- **Confidence: very low.**

### A: self-hosting

- **Remaining work after B1:** 13 to 19 specs, which is 22 to 63 days.
- **Estimate: 2027-03 to 2027-07, if it is scheduled straight after B1.** As of 2026-11-17 nothing schedules it.
- **Confidence: low.** The rate measured on greenfield Rust code (about 97,000 lines of Rust in 9 days since `a4ef823`) may not hold for a port into a language whose library is being written at the same time. Each new language feature after the gate also costs five platforms. Both push toward the upper bound.

### Which reading matters most

The evidence points to **B, and B1 first**:

- ADR 0011 defers self-hosting.
- The open spec queue is all user-facing (#209, #227 to #230).
- The dogfooding that exists, the site generator, is a B1 program.
- The tooling notes (2026-11-15) found that what stops Oska replacing even Claude's Python is B1's gaps: no JSON, maps, processes or regex.
- Self-hosting adds about 13 to 19 specs for little user value. It would also turn the Rust interpreter, which is the independent oracle, into Oska code checked by itself (*inference*).

## Part 5: the first three specs that would shorten the path most

1. **Finish the parity gate: start #154 (macOS, v0.5.0) now, then #155 (Windows, v0.6.0).**
   - **Why first:** ADR 0014 freezes every other gap in Part 3, so nothing on either reading can start until v0.6.0. Every week it waits moves every estimate above by a week.
   - **Status:** #154 was filed on 2026-11-13 and had no merged ticket by 2026-11-17.
   - **Its timing:** the x86_64 milestone took 54 hours of wall time (#153), which suggests what each remaining platform costs.
2. **#209, the Bytes milestone.** This is the first spec queued behind the gate. It unblocks binary I/O, crypto (#227), TLS (#228), codecs, and the self-hosted code generator (A2). It is filed with tickets #220 to #226.
3. **A new spec: collections and the process runtime for tools.** It would cover:
   - map and set types, and `List` sort, filter, contains and remove;
   - running `std.proc` (it checks but does not run, E0371);
   - file append, delete, rename and stat.

   **Why:** this is the largest unfiled B1 gap, and A needs it too (87 map lines in `oskac`). It is also what the site's set-as-text workaround and both tooling notes point at. It should be written now, so it is `ready-for-agent` when the gate opens. Pulling `std.json` (#248) out of #229 to land beside it would do more for B1 than keeping it behind `std.llm`.

**Not recommended yet:**

- **A self-hosting spec.** Write the bootstrap ADR (A5) only after B1 lands, when the library exists to port into.
- **An optimizer spec.** It matters for B3 and A6, but neither is near.
