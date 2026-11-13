# Research: tooling and changes that would improve AI-driven development in Oska

- **Date:** 2026-11-12
- **Method:**
  - I read the agent-facing surface at `v0.3.0` (commit `6538de5`) and cite each claim by file and line. The sources were the README, `GLOSSARY.md`, `docs/design.md`, the ADRs, the guides, `docs/checker.md`, `site/agent-setup/`, `site/build.osk` and `crates/oskac/src`.
  - I ran a locally built `oskac 0.3.0` to confirm behavior. Each such probe is marked **(probe)**, with its input described.
  - I read the open GitHub issues, and the earlier research in [`2026-11-ai-first-secure-language.md`](2026-11-ai-first-secure-language.md) and [`2026-11-install-update-uninstall.md`](2026-11-install-update-uninstall.md).
  - For prior art I fetched only primary sources: specifications, official documentation, source code, and arXiv papers with their venues. Statements that are my reasoning and not a source's are marked *inference*.
- **Question:** What changes, additional tooling or functionality would improve AI-driven software development using Oska?

## Summary

Oska already has the core of an agent loop that most languages lack:

- stable diagnostic codes;
- byte-precise spans;
- machine-applicable fixes;
- an offline `explain` command;
- an authority report;
- a copy-and-paste agent setup with a check-on-edit hook.

The gaps are in what sits around that loop:

1. **The shipped hook gives wrong feedback in packages.** It checks the edited file alone, so in a package it reports errors that do not exist.
2. **Every agent has to re-implement the fix loop and the authority comparison by hand.** The agent guide gives Python for one and prose for the other.
3. **The diagnostic JSON is thinner than what LSP, rustc and Go analyzers emit.** It has no line or column, no severity, no related locations and no fix applicability.
4. **Nothing reports authority that is declared but never used.**
5. **An agent with no training data on Oska has no reference for the prelude's API.**
6. **There is no measurement yet** (#11). So none of the "AI-first" premises, nor any of the proposals below, has data behind it.

Published evidence supports investing in the feedback loop, not in more prose:

- **Linting inside the edit tool.** SWE-agent's linter on edit raised its SWE-bench Lite solve rate from 15.0% to 18.0% ([arXiv 2405.15793](https://arxiv.org/abs/2405.15793), NeurIPS 2024, Table 3).
- **Type information during generation.** Type-constrained decoding cut compile errors by 74.8% on HumanEval synthesis ([arXiv 2504.09246](https://arxiv.org/abs/2504.09246), PLDI 2025).
- **Error codes plus explanations plus small diffs.** RustAssistant fixed 73.63% of 182 real Rust compile errors ([arXiv 2308.05177](https://arxiv.org/abs/2308.05177), ICSE 2025).
- **Self-repair has limits.** Its gains are "often modest" once repair cost is counted, and it is "bottlenecked by the model's ability to provide feedback on its own code" ([arXiv 2306.09896](https://arxiv.org/abs/2306.09896), ICLR 2024). That argues for precise, tool-generated feedback over asking the model to critique itself.

The top recommendations, ranked at the end, are:

1. Fix the hook and move it into `oskac`.
2. `oskac fix`.
3. An authority diff against a baseline.
4. Run the evaluation harness, instrumented by diagnostic code.
5. Richer diagnostic JSON with fix applicability.
6. A least-authority diagnostic for unused capabilities.
7. A generated prelude and `std` API reference.
8. An MCP server.
9. Parser error recovery.
10. Property tests driven by contracts.

## Part 1: the agent-facing surface today

### 1. The loop and its outputs

- **The documented loop** has seven steps: check, apply machine fixes, fix the rest by hand, run, test, read the authority report, and the release check ([`docs/guide/agent-guide.md:5-15`](../guide/agent-guide.md)). The same loop is in the `AGENTS.md` template ([`site/agent-setup/AGENTS.template.md:5-13`](../../site/agent-setup/AGENTS.template.md)).
- **Machine-readable output.** Every command takes `--format=json` ([`cli-reference.md:47`](../guide/cli-reference.md)):
  - `check` prints one object on stdout: `diagnostics`, `functions`, `audit` and `attenuations` ([`cli-reference.md:163-187`](../guide/cli-reference.md)).
  - `test` prints `tests`, `passed`, `failed`, `skipped` and `diagnostics` ([`cli-reference.md:189-195`](../guide/cli-reference.md)).
  - `run` prints one JSON object per line on stderr ([`cli-reference.md:197-199`](../guide/cli-reference.md)).
  - `build` prints `output` and `diagnostics` ([`cli-reference.md:119-120`](../guide/cli-reference.md)).
  - `parse` prints a documented AST ([`docs/ast.md:3`](../ast.md)).
- **The diagnostic shape** is `code`, `message`, `path`, `span` (UTF-8 byte offsets) and an optional `fix`. A fix holds a `message`, an optional `path` when it edits another file, and `edits` ([`crates/oska-syntax/src/lib.rs:348-366,415-451`](../../crates/oska-syntax/src/lib.rs); [`docs/checker.md:60-61`](../checker.md)).
  - **No line or column.** `Location` holds a line and a column, but `to_json` writes only `path` ([`lib.rs:341-345,427-429`](../../crates/oska-syntax/src/lib.rs)). The human output does print `file:line:col` ([`cli-reference.md:66`](../guide/cli-reference.md)).
  - **No severity.** The one exception is the fallback note, `"severity":"note"` ([`crates/oskac/src/native.rs:50,70`](../../crates/oskac/src/native.rs)).
  - **No related locations, and at most one fix per diagnostic.**
- **Exit codes** are documented for every command:
  - `run`: 0, 1, 2, 101 and 128+n ([`cli-reference.md:95-101`](../guide/cli-reference.md)).
  - `test`: 0, 1 and 2 ([`cli-reference.md:109-113`](../guide/cli-reference.md)).
  - `build`: 0, 1 and 2 ([`cli-reference.md:124-128`](../guide/cli-reference.md)).
- **The commands** are `lex`, `parse`, `check`, `run`, `test`, `build`, `explain`, `--version` and `--help` ([`crates/oskac/src/main.rs:124-159`](../../crates/oskac/src/main.rs)). None of these exist:
  - `fix`
  - `fmt`
  - `doc`
  - `audit`
  - a test filter
  - a watch mode
  - a language server

  The tour says "No formatter or package manager" ([`docs/guide/language-tour.md:349`](../guide/language-tour.md)), although the design calls for "deterministic [formatting], with a single canonical style" ([`docs/design.md:479`](../design.md)). `oskac audit` is designed but not built ([`design.md:466`](../design.md)).

### 2. Diagnostic catalog and fixes

- **139 catalogued codes, 35 with a machine fix (probe).** The checker and runtime notes have 120 and 19 `####` code entries ([`docs/checker.md`](../checker.md), [`docs/runtime.md`](../runtime.md)). I ran `oskac explain <code> --format=json` on each, and 35 have `"machine_fix":true`.
- **What `explain` prints.** For each code it gives the meaning, the fix description, a failing example and the fixed version, offline ([`cli-reference.md:205-227`](../guide/cli-reference.md)). The site serves the same entries as `/codes/<code>` and `/codes.json` ([`site/build.osk:158`](../../site/build.osk)). This already matches RustAssistant's best practice of fetching an explanation by error code (§Part 2, 6).
- **A fix exists only "when the repair is unambiguous"** ([`cli-reference.md:161`](../guide/cli-reference.md)). There is no notion of a fix that is plausible but uncertain:
  - Some mechanical repairs have no fix. `E0102`, an expression in an interpolation hole, has none even though the repair, "compute the value with `let` first", is mechanical ([`checker.md:119`](../checker.md)).
  - One fix that does exist inserts placeholders. `E0253`'s fix inserts `todo()` arms, which compile but panic ([`agent-guide.md:64`](../guide/agent-guide.md)). In rustc's terms that fix is `HasPlaceholders`, not `MachineApplicable`.
- **Parse errors stop the file.** "A parse error is reported alone" ([`cli-reference.md:187`](../guide/cli-reference.md); [`debugging.md:109`](../guide/debugging.md)).
  - **(probe)** A file with a missing `)` and, three lines later, the misspelled type `Strng` reported only `E0101`.
  - The checker itself does recover. It "reports every independent error" and uses an error type to suppress follow-on errors ([`design.md:127-130`](../design.md)).
- **Every diagnostic is an error.** There are no warnings or lints. The design states this on purpose for injection: "a compile error… It is not a lint" ([`design.md:295`](../design.md)).
  - **(probe)** I checked `fn main(io: Stdout, fs: Fs, net: Net, clock: Clock)` where only `io` is used, plus a helper `add(a, b, fs, clock)` that ignores `fs` and `clock`. It exits 0.
  - The authority report then lists `main: Stdout (io), Fs (fs), Net (net), Clock (clock)` and `add: Fs (fs), Clock (clock)`.
  - So **declared authority that is never used is invisible to the checker.** It is visible only to a reader of the report.

### 3. The authority report as review

- **The report is the review.** For each function it gives `authority`, `via` and `pure`, plus every audit site and attenuation ([`agent-guide.md:78-109`](../guide/agent-guide.md); [`checker.md:62-77`](../checker.md)).
- **Comparison is manual.** The agent is told to "compare this against the report from before your change". It is given four things to look for: grown authority, a new audit entry, a wider attenuation root, and `pure` turned impure ([`agent-guide.md:111-118`](../guide/agent-guide.md)). Nothing in the toolchain produces that comparison.
- **Function names are not unique keys yet.** A trait method and an inherent method with the same name share one report name (#123). Any automatic diff needs that fixed first.

### 4. Agent integration

- **The agent-setup prompt** installs `oskac` with verified `--json` output. It writes an `AGENTS.md` section and a one-line `CLAUDE.md` (`@AGENTS.md`). It offers an opt-in Claude Code `PostToolUse` hook ([`site/agent-setup/prompt.md:14-60`](../../site/agent-setup/prompt.md); [`CLAUDE.template.md:1`](../../site/agent-setup/CLAUDE.template.md); [`claude-settings.json:1-15`](../../site/agent-setup/claude-settings.json)).
- **Tests hold these files to the real tools.** Every flag, command and link they mention must exist, and the hook must request no permission ([`crates/oskac/tests/agent_setup.rs:1-12`](../../crates/oskac/tests/agent_setup.rs)).
- **The hook** pulls `file_path` out of the hook's JSON input with a `sed` regex. It runs `oskac check --format=json "$file"`, and on failure exits 2 so Claude sees stderr ([`site/agent-setup/oska-check.sh:13-32`](../../site/agent-setup/oska-check.sh)). It has three defects:
  - **It is wrong in packages (probe).** In `examples/shopping`, `oskac check src/main.osk` exits 1 with two `E0203` and three `E0221` diagnostics. The file is "handled alone" ([`cli-reference.md:35`](../guide/cli-reference.md)), but `oskac check .` exits 0. So after every edit to a package's entry module, the hook feeds Claude false errors and blocks with exit 2. The `E0203` message does name the cause, but an agent has to recognize it, and the hook's whole job is to save that turn.
  - **The JSON is parsed with a regex**, which breaks on a path containing an escaped quote. This is an *inference* from the `sed` pattern at `oska-check.sh:14`.
  - **It is POSIX sh only**, so it does not work on Windows (#87).
- **There is no gate at the end of a task.** Nothing runs the tests or `--release` before the agent says it is done. The loop's "you are done when…" ([`agent-guide.md:15`](../guide/agent-guide.md)) rests on the agent's diligence.
- **No MCP server, skill or plugin is published.**

### 5. Docs for agents

- **What the site build publishes for agents** ([`site/build.osk:24-25,149-158,429-456`](../../site/build.osk)):
  - `llms.txt` in the llmstxt.org shape;
  - `llms-full.txt`;
  - a Markdown copy of every page;
  - `search.json`;
  - `codes.json` with a page per code;
  - `latest.json`.

  The site generator is itself an Oska program.
- **The prelude has no published API reference.** The prelude has 464 lines and 82 function declarations ([`crates/oska-check/src/prelude.osk`](../../crates/oska-check/src/prelude.osk)), and `site/build.osk` never reads it. The guides list methods in prose, for example the string methods ([`language-tour.md:268`](../guide/language-tour.md)), and the `std` modules in a table ([`checker.md:86-90`](../checker.md)). An agent that needs "what methods does `List` have?" must read the Rust repo's `.osk` source or guess.
- **Known gaps are in the agent guide** ([`agent-guide.md:128`](../guide/agent-guide.md)). These are module `const` (#83), `let` annotations (#84) and `{self.x}` (#85), all `ready-for-agent`. Each is a trap every agent pays for in context tokens, or else falls into.

### 6. Testing and debugging

- **What tests have.** Tests get a deterministic `t`: an in-memory `fs`, a fixed `clock`, a seeded `random`, and a recording `stdout`. They have four assertions ([`docs/guide/testing.md:17-37`](../guide/testing.md)). `Random` has one method, `next_int` ([`prelude.osk:231-235`](../../crates/oska-check/src/prelude.osk)).
- **What tests lack:**
  - property-based tests;
  - fuzzing;
  - coverage;
  - mutation testing;
  - snapshot assertions;
  - a way to run one test by name.

  The repo's own Rust tests use golden files regenerated with `UPDATE_GOLDEN=1` ([`crates/oskac/tests/package_review_golden.rs:1-6`](../../crates/oskac/tests/package_review_golden.rs)), but Oska programs have no equivalent.
- **Contracts are checked at runtime in every build.** `requires` blames the caller (`P0006`) and `ensures` blames the function (`P0007`) ([`design.md:342-346`](../design.md)). Contracts are therefore free test oracles that nothing uses yet.
- **Debugging has three tools:** `dbg`, panics with a code and a span, and `--format=json` ([`debugging.md:3`](../guide/debugging.md)). A panic has no call stack, only the span where it happened ([`debugging.md:99-103`](../guide/debugging.md)).

### 7. Constraints any proposal must respect

- **No crates from outside the repo.** `scripts/check.sh` enforces this ([`scripts/check.sh:9-11`](../../scripts/check.sh); [ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md):16; [ADR 0011](../adr/0011-oska-generates-native-code-itself.md):14). So a JSON-RPC server for LSP or MCP must hand-write its JSON parser. One already exists in test code ([`crates/oskac/tests/common/mod.rs:8`](../../crates/oskac/tests/common/mod.rs)) and in the AST export ([`crates/oska-syntax/src/export.rs:98`](../../crates/oska-syntax/src/export.rs)).
- **`unsafe` is allowed only in `oska-sys`** ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)), and system calls only in `std.rt` ([ADR 0013](../adr/0013-system-calls-only-in-std-rt.md)).
- **No user macros.** Code generators that write ordinary source files are the sanctioned alternative ([ADR 0003](../adr/0003-no-user-defined-macros.md)).
- **One checked program.** The interpreter is the reference, and the native backend is held to it by the differential suite ([ADR 0007](../adr/0007-interpreter-runs-the-checked-program.md); `GLOSSARY.md:126-144`).
- **No analytics** ([`CODING_STANDARDS.md:15`](../../CODING_STANDARDS.md)). So measurement must come from the opt-in evaluation, never from telemetry.
- **Contested decisions wait for benchmark data**, explicitly including "the diagnostic format" ([`design.md:498`](../design.md)).

### 8. Status of the earlier research's recommendations

These are the "Gaps worth considering" from [`2026-11-ai-first-secure-language.md:193-199`](2026-11-ai-first-secure-language.md).

| Recommendation | Status at v0.3.0 |
| --- | --- |
| Unsafe/FFI policy | **Designed, not needed yet.** v0 has no FFI. The manifest allow-list and `oskac audit` are specified ([`design.md:453-466`](../design.md)). `unsafe` in the toolchain is confined to `oska-sys` (ADR 0009). |
| Capability hierarchy | **Done, differently.** `main` is the root of authority and declares exactly what it gets ([`GLOSSARY.md:14-16`](../../GLOSSARY.md)). Attenuation is enforced ([ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)). |
| Contracts | **Runtime half done.** `requires`/`ensures` run in all builds ([`design.md:341-346`](../design.md)). `old()` is planned (#92). SMT proving is later ([`design.md:354`](../design.md)). |
| Sink-specific APIs | **Done in the checker.** These are sink literals, data and structural positions, `E0300` and `E0341` ([`checker.md:82-105`](../checker.md)). `std.sql` and `std.proc` check but do not run ([`language-tour.md:347`](../guide/language-tour.md)). |
| Evaluation | **Not done.** #11 is `ready-for-human`, and there is no `eval/` directory. The plan is in [`design.md:481-498`](../design.md). |

The earlier research had no verified findings for its "Agent tooling and feedback loops" angle ([`2026-11-ai-first-secure-language.md:173`](2026-11-ai-first-secure-language.md)). Part 2 fills that gap. Installing, updating and uninstalling are covered in [`2026-11-install-update-uninstall.md`](2026-11-install-update-uninstall.md) and are not repeated here.

## Part 2: prior art

### 1. Editor protocol: LSP 3.17

Source: [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).

- **`Diagnostic`** has `code` and `codeDescription` (a link to docs; Oska has `/codes/<code>`). It has `relatedInformation`, "e.g. when symbol-names within a scope collide all definitions can be marked". It has `data`, "preserved between a `textDocument/publishDiagnostics` notification and `textDocument/codeAction` request".
- **Code actions.** `quickfix` and `source.fixAll` code actions carry a `WorkspaceEdit` across files. A preferred quick fix "properly addresses the underlying error".
- **`source.fixAll`** covers "errors that have a clear fix that do not require user input. They should not suppress errors or perform unsafe fixes."
- **Pull diagnostics** (`textDocument/diagnostic`, since 3.17) let the client choose when diagnostics are computed.

### 2. Tool protocol for agents: MCP

Source: the [MCP specification 2026-07-28, tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).

- **Tools are "model-controlled".** Each has an `inputSchema` and an optional `outputSchema`. Results come back as `structuredContent`, with the JSON also as text for compatibility.
- **Errors.** Tool execution errors "contain actionable feedback that language models can use to self-correct".
- **Annotations:**
  - `readOnlyHint` and `destructiveHint` (default `true`);
  - `idempotentHint` and `openWorldHint`;
  - "clients MUST consider tool annotations to be untrusted unless they come from trusted servers."
- **Transport.** stdio uses newline-delimited JSON-RPC from a client-launched subprocess.
- **Precedent:** gopls ships "an experimental built-in server for the Model Context Protocol" with `gopls mcp -instructions` ([gopls MCP](https://go.dev/gopls/features/mcp)). This is a language server's analyses exposed directly to agents.

### 3. Agent harness: Claude Code and AGENTS.md

- **Hooks** ([Claude Code hooks](https://code.claude.com/docs/en/hooks)):
  - `PostToolUse` exit 2 "Shows stderr to Claude; the tool already ran".
  - `Stop` exit 2 "Prevents Claude from stopping, continues the conversation".
  - `PreToolUse` exit 2 "Blocks the tool call".
  - "Claude never sees stderr from a hook that exits 0."
- **Skills** are `SKILL.md` files. Claude applies them when the `description` matches, and a `paths` glob can scope them ([skills](https://code.claude.com/docs/en/skills)).
- **Subagents** run "in its own context window with a custom system prompt, specific tool access" ([subagents](https://code.claude.com/docs/en/sub-agents)).
- **Project MCP servers** live in `.mcp.json`, and Claude Code prompts for approval before using them ([MCP in Claude Code](https://code.claude.com/docs/en/mcp)).
- **Plugins** bundle "skills, agents, hooks, MCP servers" as one installable unit ([plugins](https://code.claude.com/docs/en/plugins)).
- **AGENTS.md** is "a README for agents", used by "over 60k open-source projects". "The closest AGENTS.md to the edited file wins." Agents "will attempt to execute relevant programmatic checks and fix failures before finishing" ([agents.md](https://agents.md)).
- **llms.txt** has an H1 as the only required section, then H2 file lists. It recommends `.md` copies at the same URL ([llmstxt.org](https://llmstxt.org)). Oska follows this already.

### 4. Compiler and linter fix tooling

- **rustc JSON** ([rustc JSON output](https://doc.rust-lang.org/rustc/json.html)):
  - Every suggestion carries `suggestion_applicability`:
    - `MachineApplicable`: "should be automatically applied";
    - `MaybeIncorrect`: "may be what the user intended, but it is uncertain";
    - `HasPlaceholders`: "cannot be applied automatically";
    - `Unspecified`.
  - The code object carries an `explanation`.
  - "Enumerated fields like 'level' or 'suggestion_applicability' may add new values."
- **`cargo fix`** "will automatically take rustc's suggestions… and apply them". It has `--broken-code` and `--allow-dirty` ([cargo fix](https://doc.rust-lang.org/cargo/commands/cargo-fix.html)).
- **Go's `analysis` package** ([go/analysis](https://pkg.go.dev/golang.org/x/tools/go/analysis)):
  - A `Diagnostic` has `Category`, `URL` and `Related`, and several `SuggestedFix` alternatives, of which "at most one may be applied".
  - Fixes for different diagnostics "should be treated as independent changes to the same baseline file state".
  - `go vet -json` emits analyzer diagnostics as JSON ([cmd/vet](https://pkg.go.dev/cmd/vet)).
- **tsserver** offers `getCodeFixes` per error code and `getCombinedCodeFix` by `fixId` to "fix all" of one kind in a file ([protocol.ts v5.9.3](https://raw.githubusercontent.com/microsoft/TypeScript/v5.9.3/src/server/protocol.ts)).
- **Ruff** splits fixes into safe and unsafe. With safe fixes, "the meaning and intent of your code will be retained", while unsafe ones may change runtime behavior. Only safe fixes apply by default ([Ruff fix safety](https://docs.astral.sh/ruff/linter/#fix-safety)).

### 5. Testing tools

- **QuickCheck**: "Properties are described as Haskell functions, and can be automatically tested on random input" (Claessen & Hughes, ICFP 2000, [PDF](https://www.cs.tufts.edu/~nr/cs257/archive/john-hughes/quick.pdf)).
- **Hypothesis** shrinks a failing case to a minimal one, then reports which parts "can vary freely". It saves failures to a local database and replays them ([Hypothesis API](https://hypothesis.readthedocs.io/en/latest/reference/api.html)).
- **Go native fuzzing** ([Go fuzzing](https://go.dev/doc/security/fuzz/)):
  - "uses coverage guidance";
  - failing inputs go to `testdata/fuzz/…`;
  - they "will now be run by default with go test, serving as a regression test".
- **libFuzzer** is "an in-process, coverage-guided, evolutionary fuzzing engine" ([LibFuzzer](https://llvm.org/docs/LibFuzzer.html)). cargo-fuzz drives it ([cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html)).
- **Mutation testing** gives a "mutation adequacy score". It rests on the competent-programmer and coupling-effect hypotheses (Jia & Harman, IEEE TSE 37(5), 2011, [PDF](https://web.eecs.umich.edu/~weimerw/2020-481F/readings/mutation-testing.pdf)).
- **cargo-mutants** reports caught, missed, unviable and timeout mutants, and prints only the missed ones "because they're the most actionable" ([mutants.rs](https://mutants.rs/using-results.html)).

### 6. Static-analysis exchange: SARIF

Source: the [SARIF 2.1.0 OASIS Standard](https://docs.oasis-open.org/sarif/sarif/v2.1.0/).

- **Fields.** It has `result`, rules with `helpUri`, and `fixes` (artifact changes and replacements). It has `codeFlows`/`threadFlow`, "a temporally ordered sequence of code locations", which is the right shape for a path from an untrusted source to a sink.
- **GitHub code scanning** accepts SARIF and uses `partialFingerprints` "to detect when two results are logically identical" across commits ([GitHub SARIF support](https://docs.github.com/en/code-security/code-scanning/integrating-with-code-scanning/sarif-support-for-code-scanning)).

### 7. Research on LLMs, compilers and feedback

| Paper | Venue | Finding relevant here |
| --- | --- | --- |
| Self-Debugging, [arXiv 2304.05128](https://arxiv.org/abs/2304.05128) | ICLR 2024 | Execution feedback improves accuracy "by up to 12%" on TransCoder and MBPP, and "can match or outperform baseline models that generate more than 10x candidate programs". |
| Olausson et al., [arXiv 2306.09896](https://arxiv.org/abs/2306.09896) | ICLR 2024 | Counting its cost, self-repair gains are "often modest… sometimes not present at all". It is "bottlenecked by the model's ability to provide feedback on its own code". |
| Type-constrained decoding, Mündler et al., [arXiv 2504.09246](https://arxiv.org/abs/2504.09246) | PLDI 2025 | "94% of compilation errors result from failing type checks" and about 6% from syntax. Type constraining cut compile errors by 74.8% (HumanEval) and 56.0% (MBPP), against 9.0% and 4.8% for syntax-only constraining. Repair improved "relatively by 37%". |
| SWE-agent, [arXiv 2405.15793](https://arxiv.org/abs/2405.15793) | NeurIPS 2024 | Its design principles are "Environment feedback should be informative but concise" and that a guardrail such as "a code syntax checker… can help agents recognize and quickly correct errors". On SWE-bench Lite: linter on edit 18.0% vs 15.0% without it; a 100-line viewer 18.0% vs the whole file 12.7%; summarized search 18.0% vs iterative 12.0%. |
| RustAssistant, [arXiv 2308.05177](https://arxiv.org/abs/2308.05177) | ICSE 2025 | With GPT-4 it fixed 73.63% of 182 real Rust commits' errors and 92.59% of micro-benchmarks. It fetches explanations by error code (`--explain`). Asking for whole revised snippets "tanked below 10%", so it uses a changelog-style diff. |
| MultiPL-E, [arXiv 2208.08227](https://arxiv.org/abs/2208.08227) | IEEE TSE 2023 | Studies "the impact of language frequency and language features on model performance" across 18 languages. |
| MultiPL-T, [arXiv 2308.09895](https://arxiv.org/abs/2308.09895) | OOPSLA 2024 | Code LLMs "struggle with low-resource languages". Test-validated, LLM-translated training data is "significantly more efficient and effective than alternatives such as training longer". |
| SynCode, [arXiv 2403.01632](https://arxiv.org/abs/2403.01632) | TMLR 2025 (venue from a secondary index) | Grammar-constrained decoding "reduces 96.07% of syntax errors in generated Python and Go code". llama.cpp's GBNF does the same for local models ([GBNF README](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md)). |

**How these papers apply to Oska (*inference*):**

1. Oska's errors will mostly be type and capability errors, not syntax errors. Mündler et al.'s 94/6 split, and Oska's regular syntax, both point that way. So investment belongs in the checker's feedback, not in grammar tooling.
2. Feedback should be short, located and code-keyed (SWE-agent, RustAssistant).
3. The tool, not the model, should judge correctness (Olausson et al.).
4. A low-resource language must ship its API knowledge in context, because the model cannot have it (MultiPL-E, MultiPL-T).

## Part 3: proposals

Effort is S (days), M (one to two weeks) or L (a milestone). "Failure mode" names the agent problem a proposal addresses.

### A. Feedback loop and diagnostics

**A1. Fix the check-on-edit hook, and move it into `oskac`.**

- **What:**
  - Add `oskac hook claude [--event post-edit|stop]`. It reads the hook's JSON on stdin with the in-repo parser and finds the nearest `oska.toml` above the edited file. It checks the package if there is one, and the file otherwise.
  - On `PostToolUse` it prints only the diagnostics, compact and with line and column, and exits 2.
  - On `Stop` it runs `oskac test` and `oskac check --release` and exits 2 on failure, so the agent cannot finish with failing tests.
  - `oska-check.sh` becomes a one-line call.
- **Failure mode:**
  - The shipped hook reports false errors for package modules (Part 1 §4).
  - The end-of-task gate is unenforced.
  - Windows has no hook (#87).
- **Evidence:** SWE-agent's linter on edit gained 3 points on SWE-bench Lite. "Concise" feedback is one of its stated design principles. Claude Code's `Stop` hook exit 2 "prevents Claude from stopping".
- **Fit:** Rust in `oskac`, no new crates. The hook still asks for no permission, so the `agent_setup.rs` invariants hold. It is cross-platform because it is the `oskac` binary.
- **Effort:** S.
- **Risks:**
  - A `Stop` gate that runs a long test suite slows every turn. Make it opt-in, and time-box it.
  - The hook is still Claude Code-specific. A1's logic should live in a reusable function (see C1).

**A2. `oskac fix [path]`: apply machine-applicable fixes until none are left.**

- **What:** Apply one fix at a time, re-check, and repeat until no diagnostic has an applicable fix or an iteration cap is reached. Write each file once at the end. Print JSON of the fixes applied, by code and path, and the diagnostics that remain. Add a `--dry-run` that prints a unified diff.
- **Failure mode:** Every agent re-implements the loop from the Python in [`agent-guide.md:35-56`](../guide/agent-guide.md), with three pitfalls the guide has to spell out: stale offsets, byte versus character offsets, and edit order ([`agent-guide.md:58-64`](../guide/agent-guide.md)). It costs a tool round-trip per fix.
- **Evidence:** `cargo fix`, `ruff --fix`, LSP `source.fixAll` and tsserver `getCombinedCodeFix` all make "apply the certain fixes" one command.
- **Fit:** Uses the existing `Fix` type. It never applies a fix that changes authority. No current fix does; this is a rule for the future.
- **Effort:** S.
- **Risk:** It applies `E0253`'s `todo()` arms silently. A3's applicability levels prevent this.

**A3. Richer diagnostic JSON, with fix applicability.**

- **What:** Additive fields, so existing consumers keep working:
  - `line` and `column`, 1-based. Count characters too, or document the unit.
  - `severity`.
  - `related`: a list of `{path, span, message}`, such as "declared here", "untrusted since here" or "capability passed here".
  - `fix.applicability`: `machine`, `maybe` or `placeholders`, following rustc.
  - `suggestions`: alternatives that are never auto-applied, following Go's at-most-one-of `SuggestedFix`.
  - `url`: `https://oskalang.dev/codes/<code>`, following LSP `codeDescription` and SARIF `helpUri`.
- **Failure modes:**
  - Agents edit by text match, and Claude Code's `Edit` takes an `old_string`. A byte offset has to be converted before a model can use it. *Inference*, from the guide's own instruction to "slice the file's bytes" ([`debugging.md:105`](../guide/debugging.md)).
  - A trust error (`E0300`) points at the sink, but the fix usually belongs where the text became untrusted.
  - With applicability levels, `E0102` and the closest-name repairs could carry `maybe` fixes instead of none, and `E0253` would honestly be `placeholders`.
- **Evidence:** LSP `relatedInformation`; rustc's four applicability levels; Ruff's safe/unsafe split; SARIF `codeFlows` for source-to-sink paths. RustAssistant's success relied on structured error info plus explanations.
- **Fit:** This is "the diagnostic format", which [`design.md:498`](../design.md) says is gated on benchmark data. Additive fields keep the format stable while the evaluation (D1) measures their effect.
- **Effort:** S for line, column, severity and url. M for `related` on trust and capability errors.

**A4. Parser error recovery at item boundaries.**

- **What:** On a parse error, skip to the next top-level `fn`, `struct`, `enum`, `impl`, `trait`, `test` or `use` at column 0, and keep parsing. Check the items that parsed. Suppress every diagnostic inside an item that failed to parse.
- **Failure mode:** One syntax error hides every other error in the file (probe, Part 1 §2), so a file with several mistakes costs several iterations.
- **Evidence:** Mündler et al. find only about 6% of compile errors are syntactic, so the gain is bounded. That is why this is M priority, not top. SWE-agent's finding that informative feedback matters still applies.
- **Fit:** The checker already recovers and has an error type ([`design.md:127-130`](../design.md)). The parse-error-alone rule exists because "the rest of the file cannot be trusted" ([`debugging.md:109`](../guide/debugging.md)). Recovering at item granularity keeps that promise for the broken item only.
- **Effort:** M.
- **Risk:** Misleading cascades. Keep recovery conservative and test it with golden files.

**A5. A least-authority diagnostic: unused capabilities.**

- **What:** Report a capability-bearing parameter that the body never uses, directly or by passing it on.
  - **On `main`,** a machine fix removes it, since nothing calls `main`.
  - **On other functions,** report it with a `maybe` fix, because callers change too.
  - **A `cap struct` field** that is never read gets the same treatment.
  - **Possible extension:** an attenuation whose result is used only for reads, cut from a read-write capability ("could be `read_only()`").
- **Failure mode:**
  - Agents over-ask for authority to make code compile. The probe in Part 1 §2 accepts `net: Net` in `main` with no use.
  - The authority report then overstates what the program does, and that weakens the report as a review signal.
- **Evidence:** This is the least-authority principle the earlier research found (findings 3 and 4). The agent guide already tells agents to prefer a narrower capability ([`agent-guide.md:113`](../guide/agent-guide.md)). This would make that checkable.
- **Fit:**
  - Oska has no warning tier, so a decision is needed (open question 1). One option is an error only under `--release`, as `dbg` and `todo` are ([`cli-reference.md:48`](../guide/cli-reference.md)). Another is a new `warning` severity that fails `--release`.
  - Unused locals and imports could join the same tier.
- **Effort:** M.
- **Risk:** Generic or `cap fn` uses need care to avoid false positives.

**A6. `oskac fmt`.**

- **What:** The canonical formatter the design already calls for ([`design.md:479`](../design.md)), with `--check` for CI. It must be idempotent.
- **Failure mode:** Diff noise in agent PRs, which makes review harder. A canonical style also makes text-matching edits by agents more predictable (*inference*).
- **Evidence:** No primary source here measures formatting's effect on agents. This is a review-cost argument, not an agent-accuracy one.
- **Fit:** The AST export exists. Comments must be preserved: check whether the AST keeps them before starting.
- **Effort:** M. **Priority:** below the top 10 for agents, though higher for human reviewers.

### B. Authority and security review tooling

**B1. Authority diff: `oskac check --baseline <report.json>` (or `--baseline-ref <git-ref>`).**

- **What:** Compare the current report with a saved one. Emit `widened`, `narrowed`, `new_audit_sites`, `removed_audit_sites`, `new_attenuations` (with their roots), and `became_impure`, keyed by function name. Exit non-zero on any widening unless the user passes `--allow-widening`. An agent runs it before and after, and CI runs it on PRs.
- **Failure mode:** The agent guide's step 6, "compare this against the report from before your change" ([`agent-guide.md:111-118`](../guide/agent-guide.md)), is a manual diff of JSON. Agents will skip it or get it wrong (*inference*), and it is the step that carries the security model.
- **Evidence:** This is the same idea the package design adopts for dependencies: "the build fails when computed exceeds approved", approved by editing the manifest so the change is a reviewable diff (#12, [ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md):17).
- **Fit:** Uses the report as it is. It needs stable, unique function names first (#123). The application's own authority could later be approved in `oska.toml` the way dependencies' authority will be.
- **Effort:** S for a file baseline. M with git refs, which need git access under capabilities or a shell-out.
- **Risk:** Renames show as remove-plus-add. Report them as such and do not guess.

**B2. `oskac audit` now, with SARIF output.**

- **What:** Build the command the design already specifies ([`design.md:466`](../design.md)) over today's audit sites: `assume_trusted`, `expose` and attenuations to `/`. Add `--format=sarif`:
  - one rule per kind, with `helpUri` pointing at the codes pages;
  - B1's widenings as results;
  - `partialFingerprints` from function name and kind, so GitHub tracks them across commits.
- **Failure mode:** A human reviewer of an agent's PR does not see a new trust assertion unless they read the JSON. SARIF puts it inline in the PR.
- **Evidence:** SARIF 2.1.0; GitHub's code scanning SARIF support.
- **Fit:** SARIF is JSON, so no crate is needed.
- **Effort:** S for JSON. M for SARIF with fingerprints.

**B3. Taint-path explanation on trust errors.**

- **What:** For `E0300` and `E0341`, add `related` entries (A3) for the chain from the capability or program input where the text became untrusted to the sink, and point at the validator pattern in [`untrusted-data.md`](../guide/untrusted-data.md).
- **Failure mode:** The fastest way to silence `E0300` is `.assume_trusted()` at the sink, which the guide forbids ([`agent-guide.md:118`](../guide/agent-guide.md)). Showing the source makes a validator at the boundary the obvious fix (*inference*).
- **Evidence:** SARIF `threadFlow` exists for exactly this shape.
- **Effort:** M. It needs provenance tracked in the trust-flag code, which #111 proposes to refactor; do them together.

**B4. A `PreToolUse` guard for authority-bearing edits (opt-in).**

- **What:** A hook mode that blocks, with exit 2 and a reason, an edit that:
  - adds a capability to `main`'s signature;
  - adds `.assume_trusted()` or `.expose()`;
  - changes approved dependency authority in `oska.toml` once #12 lands.

  The message tells the agent to ask the user first.
- **Failure mode:** This enforces the guide's "Ask first when the task seems to need it" ([`agent-guide.md:118`](../guide/agent-guide.md)), which today is advice only.
- **Evidence:** Claude Code's `PreToolUse` exit 2 "Blocks the tool call", and `permissionDecision: "deny"` carries a reason.
- **Effort:** S once A1 exists.
- **Risk:** Text matching on a proposed edit is heuristic. B1 at `Stop` is the real check, and this guard is only an early warning.

### C. Editor and agent integration

**C1. An MCP server: `oskac mcp` over stdio.**

- **What:** Expose the loop as typed tools with `outputSchema` and `structuredContent`, and publish a `.mcp.json` snippet with the agent setup.
  - **Tools:**
    - `check` (read-only);
    - `fix` (A2; not read-only, not destructive);
    - `test` (read-only, on stand-in capabilities only);
    - `explain` (read-only);
    - `authority_diff` (B1);
    - `symbols` and `api` (C3, D2).
  - **Resources:** the code catalog and the prelude reference.
  - **Leave `run` out at first.** It executes the program with real capabilities, so it is `openWorldHint`/`destructiveHint` and should stay a shell command the user's permission settings govern.
- **Failure modes:**
  - Agents misuse CLIs: wrong flags, parsing stdout versus stderr, empty output on unreadable files ([`agent-guide.md:29`](../guide/agent-guide.md)).
  - The hook covers only Claude Code. MCP is client-neutral.
- **Evidence:**
  - MCP's tool schemas and error contract: errors "contain actionable feedback that language models can use to self-correct".
  - gopls ships the same idea for Go.
  - SWE-agent's main result is that a purpose-built agent–computer interface beats a raw shell, by a "64% relative increase compared to Shell-only".
- **Fit:** JSON-RPC and JSON parsing hand-written in Rust; reuse the test JSON reader. No `unsafe`. Tool annotations are untrusted hints, so the security comes from what the tools can do, not from labels.
- **Effort:** M.
- **Risks:**
  - It is a long-running process: memory, and stale state if it caches. Re-read files on each call.
  - Claude Code asks for approval before using project `.mcp.json` servers, which is good, so keep it opt-in like the hook.

**C2. A language server: `oskac lsp`.**

- **What:** Push and pull diagnostics; `quickfix` and `source.fixAll` code actions from `Fix`; hover with type and authority; go-to-definition; document symbols.
- **Failure mode:** Mostly a human-side need: reviewers read agent code in editors. Agents gain through C1 and through harnesses that consume LSP (*inference*; I found no primary source on Claude Code consuming LSP diagnostics directly, so verify before relying on it).
- **Evidence:** LSP 3.17 maps almost one-to-one onto Oska's diagnostic and `Fix` types. UTF-16 position conversion is the main extra work.
- **Fit:** Share a core with C1, as gopls does.
- **Effort:** L.

**C3. Semantic queries: `oskac symbols`, `oskac signature <name>`, `oskac type-at <path>:<offset>`.**

- **What:** JSON answers to "where is `X` defined", "what is `X`'s signature, contracts and authority", "who calls `X`", and "what type does the checker infer here".
- **Failure mode:** Agents navigate with grep, which cannot resolve a call-style constructor from a function call. ADR 0004 says "Tooling must resolve names before it can tell them apart" ([ADR 0004](../adr/0004-call-style-constructors.md)).
- **Evidence:** In SWE-agent's ablations, compact, purpose-built search beats iterative search (18.0% vs 12.0%).
- **Effort:** M. The checked program already has every name resolved (ADR 0007).

**C4. Ship a Claude Code plugin and a skill.**

- **What:** Bundle A1's hooks, C1's `.mcp.json` and an `oska` skill. The skill's `paths: **/*.osk` loads the loop and the "things an agent should know" only when Oska files are involved.
- **Failure mode:** `AGENTS.md` costs context in every session. A path-scoped skill loads on demand (*inference*, from the skills docs' `paths` field).
- **Effort:** S.
- **Risk:** It ties the docs to one vendor. Keep `AGENTS.md` as the neutral source, and generate the skill from it.

### D. Measurement, docs and discoverability

**D1. Run the evaluation harness (#11), instrumented by diagnostic code.**

- **What:** Ship the planned harness: 10 tasks, `claude -p`, 5 attempts, func@k and func-sec@k ([`design.md:487-498`](../design.md)). Also log, per run:
  - each diagnostic code hit, in order;
  - whether its `fix` was applied;
  - iterations to a clean check;
  - authority report deltas;
  - audit sites added.
- **Failure mode:** Without data, the ranking below is reasoning, not evidence. The design gates the diagnostic format and mandatory contracts on this data ([`design.md:498`](../design.md)).
- **Evidence:** CWEval-style func-sec@k is the earlier research's metric. RustAssistant and SWE-agent show that ablations on tool design move results by several points.
- **Fit:** Maintainer-run, never in CI, never telemetry ([`CODING_STANDARDS.md:15`](../../CODING_STANDARDS.md)).
- **Effort:** M. It needs the sink test doubles (#11's plan).
- **Use:** Run every proposal here as an ablation: with and without A2, A3, A5 and D2.

**D2. A generated API reference for the prelude and `std`: `oskac doc` and `api.json`.**

- **What:**
  - Generate from `prelude.osk` and `std/*.osk`: every type, method, signature, contract and capability, with its doc comment. Doc comments may need adding.
  - Publish it as pages, as `api.json`, and in `llms.txt` and `llms-full.txt`.
  - Run `oskac doc` on a user package to give the same for its `pub` and `pub(pkg)` items.
- **Failure mode:**
  - Agents invent methods from Rust and Python habits.
  - The only reference today is prose plus the source in the compiler repo (Part 1 §5).
  - The cost compounds because the model has no Oska training data.
- **Evidence:** MultiPL-E and MultiPL-T document the low-resource penalty. Type constraining shows that knowing which members exist cuts most compile errors (Mündler et al.).
- **Fit:**
  - `site/build.osk` already builds `codes.json` from docs. Add the API pages there, or split them out with #88.
  - The checker's prelude parser already has the declarations.
  - A test should hold the docs to the prelude, like the CLI reference is held to `--help` ([`cli-reference.md:3`](../guide/cli-reference.md)).
- **Effort:** S–M.

**D3. "Coming from Rust or Python" diagnostics and a cheat sheet.**

- **What:**
  - Add targeted parser and checker messages, with fixes, for habits from high-resource languages. Examples: `Point { x: 1 }`, which should be `Point(x: 1)` (ADR 0004); `+` on strings, which should be interpolation ([`language-tour.md:268`](../guide/language-tour.md)); `;`, `&`/`&mut` and `println!`.
  - Write a one-page table of these, linked from `llms.txt`.
  - Choose the entries from D1's code frequencies, not by guessing.
- **Failure mode:** Defaulting to familiar syntax.
- **Evidence:** RustAssistant's success depended on precise per-code explanations. MultiPL-T shows the low-resource gap.
- **Effort:** S per message.

**D4. Close the documented traps (#83, #84, #85).**

- **What:** Fix the three known gaps listed in the agent guide, then delete the warning.
- **Failure mode:** #84 is silent: a misspelled type in a `let` annotation is not reported, so an agent cannot discover its own mistake. The other two cost a wasted iteration each.
- **Effort:** S each. They are already `ready-for-agent`.

### E. Testing

**E1. Property tests, with contracts as oracles.**

- **What:**
  - Add a test form that takes typed parameters, for example `test "reverse twice" for (xs: List<Int>) { ... }`. The runner generates inputs from `t.random`, runs them, say, 100 times, and shrinks a failure.
  - The JSON report carries the seed and the minimal input. The input is saved under `testdata/` and replayed on every `oskac test`, as Go and Hypothesis do.
  - **Contract-driven mode:** for a pure function with `requires` and `ensures`, generate inputs that satisfy `requires` and check `ensures`, with no test to write. A `P0007` found this way is a bug the agent wrote.
- **Failure mode:** Agents write example tests that mirror their own implementation (*inference*). Olausson et al. show that a model's self-feedback is the bottleneck, so oracles independent of the model's tests are worth more than more model-written tests.
- **Evidence:** QuickCheck, Hypothesis (shrinking and replay), and Go fuzzing's regression corpus. Oska's pure functions are deterministic by construction ([ADR 0002](../adr/0002-capabilities-are-the-effect-system.md)), so they are ideal property targets.
- **Fit:** No new effect is needed: generation uses the test's seeded `t.random`. A `for (...)` test form is a syntax decision, so it needs an ADR. A library form, `t.check(...)` with a closure, may avoid new syntax.
- **Effort:** M for generators and shrinking of built-in types. L for user structs: derivations, per ADR 0003's "built-in, compiler-defined derivations".

**E2. Fuzz the untrusted boundary.**

- **What:** `oskac test --fuzz <fn>` targets functions whose parameters are `Untrusted<String>`. These are exactly the validators and parsers, and the type system already marks them as the attack surface. Use coverage guidance on the interpreter, and keep a corpus in `testdata/fuzz/`.
- **Failure mode:** Validators written by agents that panic or accept bad input.
- **Evidence:** libFuzzer and Go fuzzing: coverage-guided search, with crashers kept as regression tests.
- **Fit:** The interpreter can count branch coverage without instrumenting native code. Panics (exit 101) and contract violations are the crash signal.
- **Effort:** L.

**E3. Coverage and mutation feedback.**

- **What:**
  - `oskac test --coverage` reports, as JSON, the functions and branches that no test reached.
  - `oskac mutate` runs operator and constant mutants on the checked program and reports only the missed ones.
- **Failure mode:** "Write a test for every behavior you add" ([`agent-guide.md:11`](../guide/agent-guide.md)) has no check behind it. Missed mutants tell an agent exactly which assertion is missing.
- **Evidence:** Jia & Harman's mutation adequacy; cargo-mutants prints only missed mutants "because they're the most actionable".
- **Fit:** Mutating the checked program avoids reparsing. The differential suite could confirm both backends agree on each mutant.
- **Effort:** M for coverage, L for mutation.

**E4. Small test-runner conveniences.**

- **What:**
  - `oskac test --filter <substring>`.
  - Per-test durations in the JSON.
  - A snapshot assertion, `t.assert_snapshot(value)`, with `--update`, reviewed as a diff, like the repo's own golden tests.
- **Failure mode:** Rerunning a whole suite to check one fix costs time and tokens.
- **Effort:** S.

### F. Language and runtime features that remove agent error classes

- **F1. Typed holes.** `todo()` already exists. Make the checker report, as a `note`, the expected type at each `todo()`, and the names in scope that have that type. An agent can then ask "what goes here?" by writing `todo()`. The release check still rejects it.
  - **Evidence:** this targets the type-error class that is 94% of compile errors in Mündler et al. (*inference*: their method constrains decoding, which an agent harness cannot do, so this is the nearest tool-side equivalent).
  - **Effort:** S–M.
- **F2. A mechanical fix for `E0102`.** Hoist the interpolated expression into a `let` just before the statement. This keeps the design rule that only names are interpolated, and removes an iteration. **Effort:** S.
- **F3. Panic call stacks.** Add a `frames` list of `{function, path, span}` to panic JSON, at least on the interpreter, with matching output from the native backend to keep the differential suite. A `P0004` deep in a helper today gives only the helper's span ([`debugging.md:99-103`](../guide/debugging.md)). **Effort:** M.
- **F4. Capability trace at runtime.** `oskac run --trace=capabilities --format=json` logs each capability call: the method, the path or host, and the span. An agent can then confirm that the program touched only what the task needs. This is a runtime companion to the static authority report. **Effort:** M. The host is already one place in each backend.
- **F5. Contracts:** keep them optional until D1 has data, as [`design.md:500-502`](../design.md) says. Do `old()` (#92) first, because without it most `ensures` on mutating methods cannot be written.

### G. Package ecosystem

- **G1. `oskac new <name>` and `oskac new --lib`.** These would write `oska.toml`, `src/main.osk` and the `AGENTS.md` section, so agents never lay out a package by hand. Package layout errors exist as codes (`E0413`, `E0402`). **Effort:** S.
- **G2. Authority approval as an agent guardrail (#12).** When dependencies arrive, make widening approval the agent's stop sign. B4 blocks edits to approved authority, and `oskac check` names the dependency and the new kind. The model is decided in #12. **Effort:** inside milestone 5.
- **G3. Pinning the toolchain per project.** This is covered by the install research, item 4 of its summary ([`2026-11-install-update-uninstall.md:16`](2026-11-install-update-uninstall.md)). I do not repeat it here.

## Ranked top 10

The ranking weighs the evidence for the failure mode, how often an agent hits it, its security value given Oska's goals, and effort. Cheap fixes to things that are broken now rank first.

1. **A1: fix the hook and move it into `oskac`, with a `Stop` gate.** The shipped loop gives false errors on package modules today (probe), and it has no Windows support (#87). SWE-agent's linter-on-edit result is the strongest primary evidence for this kind of guardrail. Effort S.
2. **A2: `oskac fix`.** It replaces a loop every agent re-implements with known pitfalls, and precedent exists in every major toolchain. Effort S.
3. **B1: authority diff against a baseline.** It turns the guide's manual, security-critical step 6 into a machine check and a CI gate, using the same widening logic as packages. Effort S–M. It needs #123 first.
4. **D1: the evaluation harness, logged by code.** Every other item here is unmeasured, and the design gates the diagnostic format on this data. It also tells D3 and A3 where to start. Effort M.
5. **A3: line and column, severity, `related`, applicability and `url` in diagnostics.** These are cheap, additive changes that bring Oska level with LSP, rustc and Go. They make `maybe` fixes possible, label `E0253` honestly, and support B3. Effort S (core).
6. **A5: the unused-capability diagnostic.** It is the one item that closes a security gap: over-asked authority is invisible today (probe). It is cheap to state and directly serves "least authority". Effort M, plus a warning-tier decision.
7. **D2: a generated prelude and `std` API reference, with `oskac doc`.** It answers the low-resource-language penalty with in-context API knowledge, and nothing else in the toolchain provides that. Effort S–M.
8. **C1: an MCP server.** It gives typed, client-neutral access to check, fix, test, explain and diff. gopls is the precedent, and it avoids per-client hooks. It ranks after the CLI work because it wraps it. Effort M.
9. **A4: parser error recovery at item boundaries.** It removes serial iterations, but syntax is a small share of errors (about 6% in Mündler et al.). Effort M.
10. **E1: property tests with contracts as oracles.** It gives an oracle independent of the model's own tests, where self-repair research says the bottleneck is. It uses pure functions and contracts that already exist. Effort M–L, and it needs a syntax ADR.

Just outside the top 10:

- D4, the known traps: small, already `ready-for-agent`, and they could be done at any time.
- B2, SARIF for human review.
- C3, semantic queries, which are partly subsumed by C1.
- F1, typed holes.
- A6, `fmt`.

## Caveats

- **The prior art measures other languages.** It is mostly Python, with some Rust and Go, and usually 2023–2025 models. Gains such as SWE-agent's +3 points from linting may not carry over to current agents or to Oska.
- **Two key numbers come from settings an Oska agent does not use.** The "94% type errors" figure is from constrained decoding over TypeScript, and agents using Oska cannot constrain decoding. I use it only for the share of error kinds.
- **SynCode's venue** comes from a secondary index. Every other venue was confirmed on arXiv or the publisher.
- **The probes ran on a debug build** of `oskac 0.3.0` on aarch64 Linux. The hook defect was shown with `examples/shopping`. I did not run the hook end to end inside Claude Code.
- **Nothing here is measured on Oska.** Until D1 runs, the ranking is reasoned, not shown.

## Open questions for the user

1. **The warning tier.** Should Oska gain a non-fatal `warning` severity, which `--release` would reject, for unused capabilities, locals and imports (A5)? Or should these be errors only under `--release`, as `dbg` and `todo` are? *Recommended:* the `--release`-only error. It keeps "every diagnostic is an error" and needs no new concept.
2. **`fix` and placeholders.** Should `oskac fix` apply `placeholders` fixes such as `E0253`'s `todo()` arms? *Recommended:* no, not by default. Add `--placeholders` to opt in, matching Ruff's safe-only default.
3. **Where the authority baseline lives.** Is it a committed file, like a lockfile, or a git ref computed on demand (B1)? *Recommended:* a committed `authority.json`, reviewed in diffs, consistent with how dependency authority will be approved.
4. **MCP and `run`.** Should the MCP server expose `run` with real capabilities, or only `test` on stand-ins (C1)? *Recommended:* stand-ins only at first.
5. **The property-test form.** New syntax (`test "…" for (x: T)`) or a library call on `t` (E1)? This needs an ADR either way.
6. **Vendor-specific files.** How far should the agent setup go in vendor-specific packaging, such as a Claude Code plugin and skill (C4), beyond the neutral `AGENTS.md` and MCP?

## All sources fetched

| Source | Quality | Topic |
| --- | --- | --- |
| [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) | primary | Editor protocol |
| [MCP specification 2026-07-28: tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) | primary | Agent tool protocol |
| [gopls MCP server](https://go.dev/gopls/features/mcp) | primary | Language server exposed over MCP |
| [Claude Code hooks](https://code.claude.com/docs/en/hooks), [skills](https://code.claude.com/docs/en/skills), [subagents](https://code.claude.com/docs/en/sub-agents), [MCP](https://code.claude.com/docs/en/mcp), [plugins](https://code.claude.com/docs/en/plugins) | primary | Agent harness |
| [agents.md](https://agents.md) | primary | Agent instructions convention |
| [llmstxt.org](https://llmstxt.org) | primary | Docs for agents |
| [rustc JSON output](https://doc.rust-lang.org/rustc/json.html), [cargo JSON messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages), [cargo fix](https://doc.rust-lang.org/cargo/commands/cargo-fix.html) | primary | Diagnostics and fixes |
| [go/analysis](https://pkg.go.dev/golang.org/x/tools/go/analysis), [cmd/vet](https://pkg.go.dev/cmd/vet), [Go fuzzing](https://go.dev/doc/security/fuzz/) | primary | Analyzers, fixes, fuzzing |
| [TypeScript protocol.ts v5.9.3](https://raw.githubusercontent.com/microsoft/TypeScript/v5.9.3/src/server/protocol.ts) | primary (source) | Code fixes |
| [Ruff fix safety](https://docs.astral.sh/ruff/linter/#fix-safety) | primary | Safe and unsafe fixes |
| [Hypothesis API](https://hypothesis.readthedocs.io/en/latest/reference/api.html), [QuickCheck (ICFP 2000)](https://www.cs.tufts.edu/~nr/cs257/archive/john-hughes/quick.pdf) | primary | Property-based testing |
| [LibFuzzer](https://llvm.org/docs/LibFuzzer.html), [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) | primary | Fuzzing |
| [Jia & Harman, IEEE TSE 2011](https://web.eecs.umich.edu/~weimerw/2020-481F/readings/mutation-testing.pdf), [cargo-mutants](https://mutants.rs/using-results.html) | primary | Mutation testing |
| [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/), [GitHub SARIF support](https://docs.github.com/en/code-security/code-scanning/integrating-with-code-scanning/sarif-support-for-code-scanning) | primary | Static-analysis exchange |
| [arXiv 2304.05128](https://arxiv.org/abs/2304.05128) (ICLR 2024), [arXiv 2306.09896](https://arxiv.org/abs/2306.09896) (ICLR 2024) | primary | Self-repair |
| [arXiv 2504.09246](https://arxiv.org/abs/2504.09246) (PLDI 2025), [arXiv 2403.01632](https://arxiv.org/abs/2403.01632) (TMLR 2025, secondary venue), [llama.cpp GBNF](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md) | primary | Constrained decoding |
| [arXiv 2310.06770](https://arxiv.org/abs/2310.06770) (ICLR 2024), [arXiv 2405.15793](https://arxiv.org/abs/2405.15793) (NeurIPS 2024) | primary | Agent–computer interfaces |
| [arXiv 2308.05177](https://arxiv.org/abs/2308.05177) (ICSE 2025) | primary | Compiler-error repair |
| [arXiv 2208.08227](https://arxiv.org/abs/2208.08227) (IEEE TSE 2023), [arXiv 2308.09895](https://arxiv.org/abs/2308.09895) (OOPSLA 2024) | primary | Low-resource languages |
