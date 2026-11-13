# Research: AI inside Oska programs, and AI around the Oska toolchain

- **Date:** 2026-11-12
- **Method:**
  - I read the project's own description of itself at `oskac 0.3.0` (commit `fec98ec`): the README, `GLOSSARY.md`, `docs/design.md`, ADRs 0002, 0006, 0008, 0010, 0012 and 0013, the capability, untrusted-data and testing guides, the prelude (`crates/oska-check/src/prelude.osk`), and the signature-only `std.sql` and `std.proc` modules.
  - I ran a locally built `oskac 0.3.0` (debug build, aarch64 Linux) once, on a 25-line program described under [the probe](#a-probe-a-generic-tool-loop-is-reported-pure). It is marked **(probe)**.
  - This note builds on two earlier ones and does not repeat them. [`2026-11-ai-driven-development.md`](2026-11-ai-driven-development.md) covers tooling for *writing* Oska: the hook fix, `oskac fix`, the authority diff, `oskac mcp`, the LSP server and a plugin. [`2026-11-claude-code-software-factory.md`](2026-11-claude-code-software-factory.md) covers the factory that *builds* Oska. Where a proposal here depends on one of theirs, I cite it by its letter, such as "C1 in the AI-driven note".
  - For outside sources I fetched only primary ones: the Claude API docs (platform.claude.com), the Claude Code docs (code.claude.com), the Model Context Protocol specification, the Agent Skills specification, the OWASP Top 10 for LLM Applications 2025, Anthropic engineering posts, arXiv papers, and the official docs of Deno, WASI, Pydantic AI, the Vercel AI SDK and Ollama. Statements that are my reasoning and not a source's are marked *inference*.
- **Question:** What capabilities or functionality could be developed for Oska that would use Claude or other AI agents? Could Claude run inside Oska applications as a dedicated agent? How could Oska software be more agentic, AI-aware and AI-forward? Could there be a Claude plugin for Oska?

The note keeps two directions apart:

- **Part A: AI in Oska programs (runtime).** Oska code that calls a model, runs an agent loop, or is operated by one.
- **Part B: AI around Oska (tooling and ecosystem).** Claude Code plugins, skills, MCP servers, the Agent SDK and GitHub Actions that help people and agents work on Oska code.

## Summary

**Oska's design answers the hardest problems of LLM applications structurally, not by prompting.** The OWASP Top 10 for LLM Applications says "it is unclear if there are fool-proof methods of prevention for prompt injection" ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)). The mitigations it recommends are least privilege, validated output formats, segregated external content and human approval ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/), [LLM06](https://genai.owasp.org/llmrisk/llm062025-excessive-agency/)). Oska already has the language mechanisms for each one:

| OWASP mitigation | Oska mechanism that already exists |
| --- | --- |
| "Treat the model as any other user"; parameterized queries for LLM output ([LLM05](https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/)) | Text read through a capability is `Untrusted<String>`, and untrusted text cannot reach a structural position such as `sql"..."` query text, a program name or `TrustedHtml` without an audited `.assume_trusted()` ([untrusted data](../guide/untrusted-data.md)) |
| "Minimize extension permissions"; "implement authorization in downstream systems rather than relying on an LLM" ([LLM06](https://genai.owasp.org/llmrisk/llm062025-excessive-agency/)) | No ambient authority. A function reaches only the capabilities it is passed, and directory and file capabilities confine every path ([capabilities](../guide/capabilities.md), [ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)) |
| "Define and validate expected output formats" with deterministic code ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)) | Validators that turn untrusted text into validated types, and runtime-checked `requires`/`ensures` contracts ([design.md](../design.md)) |
| Keep secrets out of model context ([LLM02](https://genai.owasp.org/llmrisk/llm022025-sensitive-information-disclosure/)) | `Secret<String>` cannot be interpolated or displayed, and `.expose()` is an audit site |

The common weakness of agent frameworks in other languages is that a tool runs with the whole process's authority. Deno, the closest mainstream runtime, says "All code executing on the same thread shares the same privilege level" ([Deno security](https://docs.deno.com/runtime/fundamentals/security/)). In Oska a tool is a value holding attenuated capabilities, so **the checker, not the model, bounds what an agent can do**, and `oskac check` prints that bound. This is the "blast radius" idea in the prompt-injection design-patterns paper: "once an LLM agent has ingested untrusted input, it must be constrained so that it is impossible for that input" to trigger consequential actions ([Beurer-Kellner et al., arXiv 2506.08837](https://arxiv.org/html/2506.08837), §3).

**Most of the runtime work can start before Oska has networking.** An Oska program can be an MCP server over stdio today, because the stdio transport needs only standard input and output ([MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)). A `std.llm` API can be designed signature-first and tested against a fake model in the test host, as `std.sql` and `std.proc` already are. A local model can be reached over plain loopback HTTP once the `NetConnect` runtime exists, before TLS: Ollama serves "a subset of the Anthropic Messages API" at `/v1/messages` on `localhost:11434` ([Ollama](https://docs.ollama.com/api/anthropic-compatibility)). Calling the Claude API itself waits for the TLS client of [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md).

**A probe found one gap that matters for agents.** A generic function that calls a tool through a type parameter is reported as `pure`, although it reads files (probe). A generic agent loop would therefore hide its authority from the report. See [the probe](#a-probe-a-generic-tool-loop-is-reported-pure).

**The three most promising runtime ideas:**

1. **A1. `std.mcp` and a `Toolset` trait.** An Oska program becomes a capability-confined MCP server. It runs today on `Stdin` and `Stdout`, the authority report is the exact blast radius of every tool, and read-only hints can be computed from authority instead of claimed.
2. **A2. `std.llm`, signature-first, with a fake model in the test host.** Model output is `Untrusted<String>` by construction, so the OWASP LLM05 injection classes become compile errors. Tests are deterministic and cannot reach a real model.
3. **A4 and A5. A capability-attenuated tool loop, with approval tokens and a runtime-backed budget.** Human approval and spending limits become values the checker enforces, not prompt instructions.

**The three most promising tooling ideas:**

1. **B1. A small Claude Code plugin, published from this repository as a marketplace.** It holds a skill written to the open Agent Skills format and generated from `AGENTS.md`, plus the fixed check hook. Later it gains `oskac mcp`.
2. **B4. Authority review on pull requests through `anthropics/claude-code-action`.** A deterministic `oskac check --baseline` gate decides, and Claude only explains widenings and new audit sites.
3. **B3. `oskac mcp` bundled into the plugin.** This is C1 in the AI-driven note; this note adds only that the plugin is its distribution channel.

**Plugin recommendation:** build it now, but small. Ship the skill and the corrected hook first, once the hook defect described as A1 in the AI-driven note is fixed. Add the MCP server, an authority-reviewer subagent and an LSP server as each underlying command exists. Keep `AGENTS.md` as the vendor-neutral source and generate the plugin's skill from it. Details are in [the recommendation](#recommendation-a-claude-code-plugin-for-oska).

## Part 1: what the sources say

### 1. The Claude API surface an Oska client would need

- **Tool use.** "Client tools … run in your application. Claude responds with `stop_reason: "tool_use"` and one or more `tool_use` blocks. Your code runs the operation and sends back a `tool_result`." Tools are defined with a JSON Schema `input_schema` ([tool use overview](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)).
- **Strict tool use.** "Setting `strict: true` on a tool definition guarantees Claude's tool inputs match your JSON Schema by constraining the model's token sampling to schema-valid outputs." The tool `name` "is always valid" ([strict tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/strict-tool-use)).
- **Structured outputs.** JSON outputs come through `output_config.format`, with "schema-compliant responses through constrained decoding". The supported schema subset excludes "Recursive schemas", "Numerical constraints (such as `minimum`, `maximum`, `multipleOf`)" and "String constraints (`minLength`, `maxLength`)", and `additionalProperties` must be `false` ([structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)). Schema validity is not semantic correctness, so the program still has to validate values.
- **Cost and limits.** Each response reports `usage`, including `cache_creation_input_tokens` and `cache_read_input_tokens`. Cache reads cost "0.1 times the base input tokens price" by default, with per-model exceptions ([prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)). Rate limits are per-minute requests and input and output tokens. A 429 may carry `retry-after`, and a spend-cap 429 never does ([rate limits](https://platform.claude.com/docs/en/api/rate-limits)).
- **Hosted alternatives.** Managed Agents is "a pre-built, configurable agent harness that runs in managed infrastructure", in beta, with cloud or self-hosted sandboxes ([Managed Agents](https://platform.claude.com/docs/en/managed-agents/overview)). The Agent SDK gives "the same tools, agent loop, and context management that power Claude Code, programmable in Python and TypeScript". Other languages can "run the CLI as a subprocess" with `-p` ([Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview)).
- **Local models.** Ollama serves `/v1/messages` with streaming, "Tools (function calling)" and "Tool results", but not `tool_choice` ([Ollama](https://docs.ollama.com/api/anthropic-compatibility)). One wire format can therefore serve Claude and a local model, as long as the client does not depend on `tool_choice`.

### 2. Agent security: what is known to work

- **Prompt injection has no complete fix.** OWASP: "it is unclear if there are fool-proof methods of prevention for prompt injection". Its mitigations include "Enforce privilege control and least privilege access" and "Segregate and identify external content" ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)).
- **Excessive agency has three root causes:** "excessive functionality", "excessive permissions" and "excessive autonomy". Mitigations include "Limit the extensions that LLM agents are allowed to call to only the minimum necessary", human-in-the-loop approval, and "Implement authorization in downstream systems rather than relying on an LLM to decide if an action is allowed" ([LLM06](https://genai.owasp.org/llmrisk/llm062025-excessive-agency/)).
- **Improper output handling** includes "LLM-generated SQL queries … executed without proper parameterization". The fixes are zero trust, parameterized queries and "context-aware output encoding" ([LLM05](https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/)).
- **Unbounded consumption** includes "Denial of Wallet". The fixes are rate limits, quotas and timeouts ([LLM10](https://genai.owasp.org/llmrisk/llm102025-unbounded-consumption/)).
- **CaMeL** wraps the model in an interpreter that "explicitly extracts the control and data flows from the (trusted) query", so "the untrusted data retrieved by the LLM can never impact the program flow". It attaches capabilities to values and checks policies at each tool call. On AgentDojo it solved "77% of tasks with provable security", against 84% for an undefended system ([Debenedetti et al., arXiv 2503.18813](https://arxiv.org/abs/2503.18813)). Its stated limits: "users needing to codify and specify security policies and maintain them", and some side channels ([full text](https://arxiv.org/html/2503.18813)).
- **Design patterns.** The design-patterns paper names six: Action-Selector, Plan-Then-Execute, LLM Map-Reduce, Dual LLM, Code-Then-Execute and Context-Minimization ([arXiv 2506.08837](https://arxiv.org/html/2506.08837), §3.1).
- **Object capabilities.** The theory behind Oska's model, and behind CaMeL's capabilities on values, is Mark S. Miller's *Robust Composition* (2006) ([abstract](https://papers.agoric.com/papers/robust-composition/abstract)).

### 3. Agent and tool design guidance

- **Start simple.** "We suggest that developers start by using LLM APIs directly". Workflows run "through predefined code paths", while agents "dynamically direct their own processes and tool usage". Agents mean "higher costs, and the potential for compounding errors", so test "in sandboxed environments, along with the appropriate guardrails". "Poka-yoke your tools" ([building effective agents](https://www.anthropic.com/engineering/building-effective-agents)).
- **Tools are contracts.** "Tools are a new kind of software which reflects a contract between deterministic systems and non-deterministic agents". "More tools don't always lead to better outcomes". Return "only high signal information". Make error responses give "specific and actionable improvements" ([writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)).
- **MCP tools.** Tools have `inputSchema`, an optional `outputSchema` and `annotations`, and "clients MUST consider tool annotations to be untrusted unless they come from trusted servers". Servers "MUST validate all tool inputs, implement proper access controls, rate limit tool invocations, sanitize tool outputs". Clients "SHOULD … log tool usage for audit purposes". On state across calls: "a handle is a name, not a capability" ([MCP 2026-07-28, tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- **MCP stdio transport.** "The client launches the MCP server as a subprocess." Messages are newline-delimited JSON-RPC that "MUST NOT contain embedded newlines", and the server "MUST NOT write anything to its `stdout` that is not a valid MCP message" ([transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)).
- **MCP local-server risks.** Local servers "may have direct access to the user's system". The spec recommends launching them "with restricted access to the file system, network, and other system resources" ([security best practices](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices)).

### 4. Comparable language and runtime features

| System | What it offers | Limit relevant to Oska |
| --- | --- | --- |
| Deno | "No access to I/O by default"; `--allow-net=example.com`, `--allow-read=./data` ([docs](https://docs.deno.com/runtime/fundamentals/security/)) | Permissions are per thread, not per module: "It is not possible for different modules to have different privilege levels within the same thread" |
| WASI | A component "starts with no ambient authority and can only do what the host explicitly grants" ([wasi.dev](https://wasi.dev/)) | Authority is fixed at the module boundary. Oska's [design notes](../design.md) already plan WASM component imports as its first FFI |
| Pydantic AI | Tool schemas come from type hints and docstrings, and dependencies come through `RunContext` ([tools](https://pydantic.dev/docs/ai/tools-toolsets/tools/)). `TestModel`, `FunctionModel`, and `ALLOW_MODEL_REQUESTS=False` "to make sure we don't accidentally make real requests to the LLM while testing" ([testing](https://pydantic.dev/docs/ai/guides/testing/)) | Python has ambient authority: a tool can still open any file |
| Vercel AI SDK | The tool schema "is consumed by the LLM, and also used to validate the LLM tool calls". It has `InvalidToolInputError`, `repairToolCall` and multi-step `stopWhen` ([tools](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling)) | JavaScript has ambient authority |

The pattern *(inference)*: typed tool calling and fake models are well established. What none of these has is per-tool authority that the type checker enforces. Deno comes closest and stops at the thread.

### 5. Claude Code distribution channels

- **Plugins.** "A Claude Code plugin is a directory of skills, agents, hooks, MCP servers, or other components that Claude Code installs and loads as one unit". The manifest is `.claude-plugin/plugin.json`. An enabled plugin costs context on every turn, because each model-invocable skill's "name and description are in Claude's context". Plugins run "as you" ([plugins](https://code.claude.com/docs/en/plugins)).
- **Marketplaces.** A marketplace is "a directory or repository with a `.claude-plugin/marketplace.json` file". Users run `claude plugin marketplace add <owner>/<repo>`, and `claude plugin validate` checks it. Plugin sources can be a relative path, `github`, `git-subdir`, a git `url`, and more. Anything not named by Anthropic is a "Third-party" marketplace. A plugin can also be submitted to Anthropic's directory ([create a marketplace](https://code.claude.com/docs/en/plugin-marketplaces)).
- **Agent Skills** is an open format. A skill is a directory with `SKILL.md`, whose `name` has at most 64 lowercase characters and must match the directory, and whose `description` has at most 1024 characters. Optional directories are `scripts/`, `references/` and `assets/`. Loading is progressive: about 100 tokens of metadata, a body under 5,000 tokens recommended, and resources on demand ([Agent Skills spec](https://agentskills.io/specification)).
- **GitHub Actions.** `anthropics/claude-code-action@v1` runs in interactive mode, triggered by `@claude`, or in automation mode with a `prompt`. It accepts `plugin_marketplaces` and `plugins` inputs. "On public repositories, GitHub withholds secrets from runs triggered by fork pull requests". The docs advise `--max-turns` and workflow timeouts for cost ([GitHub Actions](https://code.claude.com/docs/en/github-actions)).

## Part 2: where Oska stands today

- **Capabilities that run:** `Stdin`, `Stdout`, `Stderr`, `Fs` and its attenuations, `Env` with `Env.secret`, `Clock` and `Random`. `Net`, `Spawn` and `Sched` check but are refused at run time with `E0370` ([capabilities guide](../guide/capabilities.md)).
- **The networking surface is declared but not implemented.** `NetConnect.connect(host: String, port: Int)` has a structural host, and `Connection.receive` returns `Untrusted<String>` ([`prelude.osk:171-189`](../../crates/oska-check/src/prelude.osk)). There is no HTTP, no `Bytes` yet (milestone 3) and no TLS (milestone 4, [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)).
- **Signature-only `std` modules are an established pattern.** `std.sql` and `std.proc` check but are refused at run time with `E0371`. `std.sql.Db` is a `cap struct` over `NetConnect` ([`std/sql.osk`](../../crates/oska-check/src/std/sql.osk)). A `std.llm` can follow the same path.
- **`std.json` is named in the design but does not exist.** The design lists it among modules that need `use` ([`design.md:157`](../design.md)), but `crates/oska-check/src/std/` holds only `sql`, `proc`, `html` and `rt`.
- **The test host already has the right shape.** `t.fs`, `t.clock`, `t.random` and a recording `t.stdout` are deterministic stand-ins, and `#[integration]` opts into the real world ([testing guide](../guide/testing.md)). The evaluation harness design already plans in-process doubles for SQL, processes and the network ([`design.md`, Evaluation](../design.md)).
- **Language constraints that shape every proposal:**
  - There are no trait objects, so "a collection that holds different types uses an enum" ([`design.md:29`](../design.md)). A tool registry is a generic over a trait, an enum, or a list of `cap fn`.
  - There are no user macros ([ADR 0003](../adr/0003-no-user-defined-macros.md)), but built-in derivations exist for "a fixed set of traits (equality, hashing, serialization)" ([`design.md:33`](../design.md)). Schema generation would be one more built-in derivation.
  - Values are reference-counted with value semantics ([ADR 0001](../adr/0001-value-semantics-with-reference-counting.md)), so shared mutable state such as a spending counter cannot live in an ordinary Oska value. It has to sit behind a runtime handle, as `Clock` and `Random` do.
  - The only attribute is `#[integration]`, and new ones need a decision.
  - System calls are allowed only in `std.rt` ([ADR 0013](../adr/0013-system-calls-only-in-std-rt.md)), and `unsafe` only in `oska-sys` ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)). The toolchain uses no external crates.

### A probe: a generic tool loop is reported `pure`

**Input (probe):** I wrote a 25-line program in the scratchpad (not in the repository):

- a trait `Toolset` with `call(self, name: String, input: Untrusted<String>)`;
- `cap struct NoteTools { notes: DirRead }`, whose `call` reads `self.notes.read_to_string(Path.from(input))`;
- `fn run<T: Toolset>(tools: T, input: Untrusted<String>)`, which calls `tools.call("read", input)`;
- a test that passes a confined `t.fs` capability and a model-style path `"../secret.txt"`.

**Output:**

```text
fn NoteTools.call: DirRead (self)
fn run: pure
fn main: Stdout (io), FsRead (fs)
  main.osk:18:34: FsRead -> DirRead: restrict_to "notes" (bytes 574..597)
fn :
  main.osk:24:34: Fs -> Dir: restrict_to "/" (bytes 765..786)
  ...
```

The test passed: the confined capability refused the path that climbs out of `notes`.

**What this shows:**

- **The good news.** Confinement holds for a path a model would choose, and the authority of the tool (`NoteTools.call: DirRead`) and of the program root (`main`) are both visible.
- **The gap.** `run` is reported `pure`, but it reads a file through `T`. The checker's rule is that "inside a generic body a type parameter is never capability-bearing" ([`checker.md:29`](../checker.md)). The glossary says a pure function "cannot perform effects, and it is deterministic" ([GLOSSARY](../../GLOSSARY.md)). For an agent loop, generic over its toolset, that label is wrong in exactly the place a reviewer would look. *Suggested fix (inference):* report a parameter whose type is a type parameter with trait bounds as `generic T (tools)`, the way a `cap fn` parameter is reported as `cap fn (f)`. The function is then not called pure.
- **A smaller defect.** Attenuations inside a `test` block are listed under an empty function name (`fn :`).

Both are now filed: the `pure` report as [#149](https://github.com/aparsons/oska/issues/149) and the empty test name as [#150](https://github.com/aparsons/oska/issues/150).

## Part 3: proposals for AI in Oska programs (runtime)

Each proposal gives what it is, the reasoning, fit with Oska's ADRs, dependencies, effort (S, M or L) and risks.

### A1. `std.mcp` and a `Toolset` trait: Oska programs as MCP servers

- **What:** A pure-Oska library that serves MCP over stdio. One trait describes a set of tools, and one function serves it:

  ```oska
  use std.json
  use std.mcp

  cap struct NoteTools { notes: DirRead, drafts: DirWrite }

  impl mcp.Toolset for NoteTools {
      fn specs(self) -> List<mcp.ToolSpec> {
          [
              mcp.ToolSpec(name: "read_note", description: "Read one note by file name.",
                  input: json.Schema.object([json.Field.text("file")])),
              mcp.ToolSpec(name: "save_draft", description: "Save a draft note.",
                  input: json.Schema.object([json.Field.text("file"), json.Field.text("text")])),
          ]
      }

      fn call(self, name: Untrusted<String>, input: json.Value) -> mcp.ToolResult {
          if name == "read_note" {
              match input.text("file") {
                  Some(file) => mcp.ToolResult.from(self.notes.read_to_string(Path.from(file)))
                  None => mcp.ToolResult.error("read_note needs a `file` string")
              }
          } else {
              mcp.ToolResult.error("unknown tool")
          }
      }
  }

  fn main(stdin: Stdin, out: Stdout, fs: Fs) -> Result<(), mcp.McpError> {
      let notes = fs.restrict_to("notes")
      let tools = NoteTools(notes: notes.read_only(), drafts: notes.restrict_to(Path.new("drafts")).write_only())
      mcp.serve(stdin, out, tools)
  }
  ```

  `name` and every text in `input` are untrusted, because the model chose them. Comparing them with literals is free, since "inspecting untrusted text is free" ([untrusted data](../guide/untrusted-data.md)).

- **Reasoning:**
  - **The authority report is the server's exact blast radius.** `main` reads one signature, and the attenuations show the roots `notes` and `notes/drafts`. Compare Deno, where every module on a thread shares one privilege level ([Deno](https://docs.deno.com/runtime/fundamentals/security/)). The MCP spec asks that local servers run "with restricted access to the file system" ([MCP security](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices)). Oska gives that per tool, by construction.
  - **Annotations can be computed, not claimed.** MCP clients must treat annotations as untrusted ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)). `std.mcp` can derive `readOnlyHint` for a tool from its authority. A tool whose `call` arm reaches no write capability is read-only *(inference; needs per-arm authority, which the checker does not compute today. A first version can mark the whole server read-only when its `Toolset` holds no write capability)*.
  - **Server MUSTs map onto existing rules.** "Validate all tool inputs" is the untrusted flag plus validators, and "implement proper access controls" is capability confinement.
  - **It answers "a dedicated agent for the software".** An application ships an MCP mode, and any MCP client, including Claude Code, operates the app only through the attenuated capabilities `main` hands to the toolset.
- **Fit:**
  - Pure Oska, with no new capability and no runtime code.
  - It needs `std.json` (A0) and a trait in `std`.
  - The registry is a generic over a trait, which fits "no trait objects".
  - It depends on the probe's reporting fix, so that `serve<T: Toolset>` is not reported pure.
- **Dependencies:** `std.json` (A0); `Stdin.read_line` (exists); the probe fix.
- **Effort:** M. The JSON-RPC framing and the `initialize`, `tools/list` and `tools/call` subset are small, and the work is mostly in `std.json`.
- **Risks:**
  - The **MCP spec moves fast**: 2025-11-25 and 2026-07-28 differ in `_meta` and result types. Pin one version and test against Claude Code.
  - **Long-running stdio:** reading line by line is fine for v0 concurrency, but it cannot cancel a running tool.
  - A tool that returns a whole file is **not "high signal"** ([writing tools](https://www.anthropic.com/engineering/writing-tools-for-agents)). Give the library pagination helpers.

### A0. `std.json`, trust-aware (the prerequisite)

- **What:** A JSON parser and printer in Oska. `json.parse(text: Untrusted<String>) -> Result<json.Value, json.Error>`. Every string extracted from a `Value` parsed from untrusted text is `Untrusted<String>`, and numbers and booleans are trusted, as the trust rules already give. Printing escapes text, so a model or user string can never break the framing that MCP stdio's "MUST NOT contain embedded newlines" rule depends on.
- **Reasoning:** JSON is the wire format of the Claude API, MCP and structured outputs. Untrusted-by-default JSON strings carry the trust flag through to every sink. *(Inference:)* `json.Value` holds text, so ADR 0006's "holds untrusted text" rule covers it without a new mechanism, provided `Value` stores its strings as `Untrusted<String>`.
- **Fit:** Pure Oska, and already named in the design. It is the first non-trivial parser of hostile input in `std`, a small precursor to the TLS and X.509 parsers of ADR 0012.
- **Effort:** M. **Risks:** deep-nesting panics (cap the depth), and `Int` overflow on large numbers. Checked arithmetic panics, so parse numbers into `Option<Int>`, never by computing.

### A2. `std.llm`: a model client, designed signature-first

- **What:** A `std` module with the API declared now, refused at run time (`E0371`) until the network runs, exactly as `std.sql` is:

  ```oska
  // std.llm: signature-only until NetConnect and TLS run.
  pub cap struct Model { conn: NetConnect, key: Secret<String> }

  // `host` and `model` decide where data goes, so they are structural (trusted).
  pub fn connect(net: NetConnect, host: String, model: String, key: Secret<String>) -> Result<Model, LlmError>

  pub struct Request {
      pub instructions: String,             // structural: trusted text only
      pub data: List<Untrusted<String>>,    // data position: documents, user input, tool output
      pub max_tokens: Int,
  }

  pub struct Reply {
      pub text: Untrusted<String>,          // model output is untrusted, always
      pub usage: Usage,
      pub stop: Stop,
  }

  impl Model {
      pub fn ask(self, budget: Budget, request: Request) -> Result<Reply, LlmError>
          requires request.max_tokens > 0
  }
  ```

  ```oska
  fn summarize(model: llm.Model, budget: llm.Budget, page: Untrusted<String>) -> Result<Untrusted<String>, llm.LlmError> {
      let reply = model.ask(budget, llm.Request(
          instructions: "Summarize the document in three sentences.",
          data: [page],
          max_tokens: 400,
      ))?
      Ok(reply.text)
  }
  ```

- **Reasoning:**
  - **Model output is untrusted by construction.** `Reply.text` is `Untrusted<String>`, so passing it to `db.query` text, `proc.run`'s program name, `TrustedHtml.new` or a `restrict_to` root is `E0300` or `E0341`. That turns OWASP LLM05's examples into compile errors, and they are exactly the "LLM-generated SQL" and unescaped-HTML cases ([LLM05](https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/)). It is the strongest Oska-specific claim in this note, and it needs no new checker rule.
  - **Instructions are a structural position, data is a data position.** This makes OWASP's "Segregate and identify external content" ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)) a type rule: untrusted text cannot be put in `instructions` without an audit site. **Caveat:** unlike SQL, the model does not enforce the separation. The rule stops the *programmer* from mixing them by accident. It does not stop indirect injection carried in `data`. That is what A4 is for.
  - **Secrets stay out of prompts.** A `Secret` cannot be interpolated, so a key or token cannot reach `instructions` or `data` without `.expose()`, which is an audit site ([LLM02](https://genai.owasp.org/llmrisk/llm022025-sensitive-information-disclosure/)). The client's own `.expose()` of the API key for the request header is the one audit site a reviewer expects in `std.llm`.
  - **Signature-first matches project practice.** `std.sql` and `std.proc` were designed this way, so the API can be argued over and type-checked in real programs before the network exists.
- **Fit:**
  - Should `Model` be a `cap struct` over `NetConnect`, or a capability kind of its own? As a `cap struct`, a dependency that gains model access shows only as `NetConnect` in package authority ([ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md)), the same as any network client. See [open question 1](#open-questions-for-the-user).
  - Choosing the host is already structural (`NetConnect.connect`), which blocks SSRF-style redirection of the client.
- **Dependencies:** A0. For running: the `NetConnect` runtime, an HTTP/1.1 client in Oska, and TLS for the Claude API (milestone 4). Plain-HTTP loopback suffices for a local model (A7).
- **Effort:** S for the signatures and checker wiring. M for the HTTP and JSON client once `NetConnect` runs.
- **Risks:**
  - **API churn:** the Claude API is versioned by header, as `anthropic-version` in the [tool-use examples](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview) shows. Keep the wire layer thin.
  - **Vendor lock-in:** name it `std.llm`, not `std.claude`, and keep the request shape to the subset Ollama also serves ([Ollama](https://docs.ollama.com/api/anthropic-compatibility)).

### A3. A fake model in the test host: `t.model`

- **What:** A scripted, recording `llm.Model` stand-in, in the style of `t.stdout`:

  ```oska
  test "the summary is never used as a query" {
      t.model.reply_with("SELECT * FROM users; --")
      let text = summarize(t.model, t.budget, "some page")
      t.assert(text.is_ok())
      t.assert_eq(t.model.requests().len(), 1)
  }
  ```

  It also provides `t.model.reply_with_tool(name, json)` for tool-loop tests, and a recorded `t.model.requests()` for asserting what was sent, including that untrusted text appeared only in `data`.
- **Reasoning:**
  - Pydantic AI needs `TestModel`, `FunctionModel` and `ALLOW_MODEL_REQUESTS=False` "to make sure we don't accidentally make real requests to the LLM while testing" ([Pydantic AI testing](https://pydantic.dev/docs/ai/guides/testing/)). In Oska that guarantee is structural: a `test` gets only `t.*` stand-ins, so it *cannot* reach a real model unless it is `#[integration]`.
  - It gives model code the determinism the [testing guide](../guide/testing.md) promises everywhere else, and it is the double the evaluation harness design already wants for networked sinks.
- **Fit:** Same mechanism as `t.fs` and `t.clock`, in the interpreter's and `std.rt`'s test hosts. The differential suite covers both backends.
- **Dependencies:** A2's signatures. **Effort:** S–M. **Risks:** scripted replies test plumbing, not model behavior. Pair them with `#[integration]` tests and the evaluation harness for behavior.

### A4. A capability-attenuated tool loop, with approval tokens

- **What:**
  - **`llm.run_tools<T: Toolset>(model, budget, request, tools: T, max_steps: Int)`.** The `tool_use` and `tool_result` loop ([tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)), reusing A1's `Toolset`, so one tool definition serves both an embedded agent and an MCP server. Tool definitions are sent with `strict: true`, so tool names and inputs match the schema ([strict tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/strict-tool-use)). Inputs are still untrusted values.
  - **An approval token, as a library pattern.** A value with private fields, so it can be made only by a function that asked a human:

    ```oska
    pub struct Approval { summary: String }

    pub fn confirm(stdin: Stdin, out: Stdout, summary: String) -> Option<Approval>

    fn delete_note(notes: DirWrite, file: Path, ok: Approval) -> Result<(), IoError>
    ```

    A tool that deletes needs an `Approval`, and only `confirm` makes one. Oska's privacy already makes such values unforgeable, the way it makes capabilities unforgeable ([capabilities](../guide/capabilities.md#you-cannot-forge-one)).
  - **A Plan-Then-Execute helper.** The planner model gets only trusted `instructions` and the tool *specs*. It returns a plan that Oska code validates into an enum of allowed actions. Untrusted data is processed afterwards, by a call with no tools ([arXiv 2506.08837](https://arxiv.org/html/2506.08837), Plan-Then-Execute and Dual LLM).
- **Reasoning:**
  - **Excessive agency is bounded by the checker.** OWASP's first two mitigations are minimizing tools and their permissions, and "Implement authorization in downstream systems rather than relying on an LLM" ([LLM06](https://genai.owasp.org/llmrisk/llm062025-excessive-agency/)). In Oska the toolset's capabilities *are* the authorization. A prompt-injected agent with `NoteTools` cannot read outside `notes`, because the probe's confinement holds whatever path the model picks.
  - **Human approval becomes a type requirement, not a prompt instruction.** That is OWASP's human-in-the-loop, enforced where it cannot be argued away.
  - **It is a static cousin of CaMeL.** CaMeL attaches capabilities to values and checks a policy at every tool call ([arXiv 2503.18813](https://arxiv.org/html/2503.18813)). Oska's structural and data positions are a fixed, compile-time version of one policy: untrusted data never chooses code, commands, hosts or roots. It needs no user-written policy, which removes CaMeL's main stated burden. It is also weaker *(inference)*: one trust bit, no per-value provenance, and no check of *which* untrusted data flows into a data position. Exfiltration through a data position, such as `Connection.send`, stays possible.
- **Fit:**
  - It needs the probe fix, or `run_tools` reports pure.
  - `max_steps` and the budget (A5) bound the loop. Panics are uncatchable, so the loop must return `Err` on a bad tool call, never panic.
  - `Approval` needs no language change.
- **Dependencies:** A1, A2, A3 and A5. **Effort:** M.
- **Risks:**
  - **Injection still steers the agent within its authority**, for example by saving a misleading draft. Say so in the docs: Oska bounds the blast radius and does not prevent the blast.
  - **Approval fatigue.** Reserve approvals for consequential tools.

### A5. A runtime-backed `Budget`

- **What:** A capability-like handle whose counters live in the runtime, as `Clock` and `Random` hold a `handle: Int` ([`prelude.osk`](../../crates/oska-check/src/prelude.osk)). `budget.limit(tokens: 20_000, calls: 10)` attenuates into a child budget that draws from its parent. Every `ask` debits the reply's `usage`, counting cache reads and writes, and returns `Err(LlmError.OverBudget)` before a request would exceed the limit.
- **Reasoning:**
  - "Denial of Wallet" ([LLM10](https://genai.owasp.org/llmrisk/llm102025-unbounded-consumption/)) and runaway agent loops are cost problems, and agents mean "higher costs" ([building effective agents](https://www.anthropic.com/engineering/building-effective-agents)).
  - A budget is authority to spend, so it should flow like other authority: passed down, narrowed, never widened.
  - **Value semantics forces the runtime design.** A budget held in an ordinary Oska value would be copied when passed down, and each copy would spend the full amount ([ADR 0001](../adr/0001-value-semantics-with-reference-counting.md)). The counter must live behind a runtime handle *(inference)*.
- **Fit:**
  - Either a built-in capability, whose root `main` must declare so the program's spending is visible in its signature, or a `std.llm` type the runtime backs.
  - The API's own spend limits ([rate limits](https://platform.claude.com/docs/en/api/rate-limits)) are per organization. A program-level budget is the per-task control those cannot give.
- **Dependencies:** A2. **Effort:** M. **Risks:**
  - Token counts are known only after a reply, so a single call can overshoot by up to its `max_tokens`. Reserve `max_tokens` before the call.
  - Retrying after a 429 needs waiting, which needs `Clock` and a sleep. `Sched` does not run yet.

### A6. Structured output as a built-in `Schema` derivation

- **What:** A built-in derivation that turns a struct or enum into a JSON Schema in the supported subset: objects with `additionalProperties: false`, enums of strings, no recursion. `model.extract<T: Schema>(budget, instructions, data) -> Result<T, LlmError>` then decodes the reply. Every text field of `T` must be declared `Untrusted<String>` or be a validated type. A plain `String` field is an error, so decoding cannot launder model output into trusted text. Numeric ranges that the API cannot enforce ("Numerical constraints … `minimum`, `maximum`" are unsupported, per [structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)) are written as the type's contracts or validators and checked after decoding.
- **Reasoning:**
  - OWASP: "Define and validate expected output formats" with deterministic code ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)). Constrained decoding gives the shape, and Oska's contracts give the values.
  - Pydantic AI and the Vercel AI SDK derive schemas from types too ([Pydantic AI](https://pydantic.dev/docs/ai/tools-toolsets/tools/), [AI SDK](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling)). Oska's addition is the trust rule on text fields.
- **Fit:** The design allows "a fixed set" of built-in derivations, and user macros are ruled out ([ADR 0003](../adr/0003-no-user-defined-macros.md)), so this has to be built in. It needs an ADR for the derivation syntax, since the only attribute today is `#[integration]`.
- **Dependencies:** A0, A2. **Effort:** M. **Risks:** the supported schema subset changes with the API, and recursive types, which Oska allows, cannot be expressed. Reject them with a clear diagnostic.

### A7. Before TLS: local models over loopback HTTP

- **What:** Once the `NetConnect` runtime exists (milestone 4 bundles it with TLS, but it does not depend on TLS), `std.llm.connect(net, "localhost", ...)` can talk plain HTTP to Ollama's `/v1/messages` on `localhost:11434`, which supports streaming, tools and tool results but not `tool_choice` ([Ollama](https://docs.ollama.com/api/anthropic-compatibility)).
- **Reasoning:** It gives a real, offline, zero-cost model for development and the `#[integration]` tier, with the same client code as Claude. The host is structural, so a program that talks only to `localhost` shows it in the code.
- **Fit:** No crypto is needed. *(Inference:)* `NetConnect`'s runtime could ship ahead of TLS if milestone 4 is split, which is worth a decision.
- **Dependencies:** the `NetConnect` runtime and an HTTP/1.1 client in Oska. **Effort:** M (HTTP). **Risks:**
  - Local-model quality differs.
  - Loopback is still network authority: the host attenuation for `NetConnect` (A8) should exist before anything relies on "localhost only".

**What does not work before TLS.** Calling `api.anthropic.com` directly. Two workarounds exist, and I recommend neither:

- *A local TLS-terminating proxy* moves the API key and the trust boundary out of Oska.
- *Running `claude -p` or `curl` through `std.proc`* needs `Spawn`, which is the broadest capability and does not run yet. The program name is structural, but the child process runs outside every Oska guarantee, as Deno warns about `--allow-run` ([Deno](https://docs.deno.com/runtime/fundamentals/security/)).

### A8. Longer term

- **Host-attenuated network capabilities.** `net.connect_only().restrict_to_host("api.anthropic.com")`, with the host printed in the authority report the way a directory root is. This is the network analogue of [ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md), and Deno's `--allow-net=example.com` is the coarse precedent ([Deno](https://docs.deno.com/runtime/fundamentals/security/)). It also lets `Model` hold a host-confined capability. Effort M, after the `NetConnect` runtime.
- **An MCP client.** Calling external MCP servers needs `Spawn` for stdio or `NetConnect` for HTTP. Every result is untrusted, and server-supplied tool annotations are ignored for any security decision, because they are untrusted ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)). Effort M–L.
- **Agents that operate a running app.** An app with `NetListen` serves MCP over Streamable HTTP. The spec's rules ("validate the `Origin` header", "bind only to localhost", "MUST NOT use sessions for authentication") belong in `std.mcp` defaults ([transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports), [security](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices)). This needs `NetListen`, `Sched` and TLS for anything remote. Effort L.
- **Provenance beyond one bit.** Distinguishing model-origin text from user-origin text, CaMeL-style. ADR 0006 deliberately chose one flag, and nothing so far shows the extra cost would pay off. I recommend deferring it until the evaluation harness (#11) shows real agent code failing in a way one bit cannot catch.

## Part 4: proposals for AI around Oska (tooling and ecosystem)

### B1. A Claude Code plugin, `oska`, distributed from this repository

- **What:** A plugin directory, and a `.claude-plugin/marketplace.json` that lists it, so users run `claude plugin marketplace add aparsons/oska` and `claude plugin install oska@<marketplace-name>` ([marketplaces](https://code.claude.com/docs/en/plugin-marketplaces)). Contents, in the order they become available:
  1. **`skills/oska/SKILL.md`**, valid under the open Agent Skills spec (a lowercase `name` matching the directory, a `description` of at most 1024 characters, a body under 500 lines with `references/` for the prelude reference) ([Agent Skills spec](https://agentskills.io/specification)). It is generated from `site/agent-setup/AGENTS.template.md` and the agent guide, so the vendor-neutral text stays the source, as the AI-driven note's C4 advises.
  2. **`hooks/hooks.json`**: the post-edit check hook, *after* it is fixed to check the package and not the edited file alone (A1 in the AI-driven note found that it reports false errors in packages today), and the `Stop` gate.
  3. **`.mcp.json`** for `oskac mcp` once it exists (C1 in the AI-driven note).
  4. **`agents/authority-reviewer.md`**: a read-only subagent that reads the authority diff and audit sites and explains them. It ships with B4's `--baseline`.
  5. **An LSP server** once `oskac lsp` exists (C2 in the AI-driven note). Plugins can carry LSP servers, as [the factory note](2026-11-claude-code-software-factory.md) records.
- **Reasoning:**
  - "Use a plugin when you want several skills, subagents, hooks, or MCP servers packaged as one unit", with "updates from its marketplace" ([plugins](https://code.claude.com/docs/en/plugins)).
  - A marketplace in the same repository as `oskac` lets plugin versions follow toolchain releases and keeps the existing `agent_setup` tests, which already hold the setup files to the real CLI, in one place ([AI-driven note, §4](2026-11-ai-driven-development.md)).
  - The same plugin format installs in GitHub Actions through `plugin_marketplaces` and `plugins` ([GitHub Actions](https://code.claude.com/docs/en/github-actions)), so B4 reuses it.
- **Fit:** No code in the compiler. The plugin only invokes `oskac`. It runs "as you" ([plugins](https://code.claude.com/docs/en/plugins)), so it must request no permissions beyond running `oskac`, as the existing hook already must.
- **Effort:** S for items 1 and 2; each later item is S once its command exists.
- **Risks:**
  - **Context cost on every turn** ([plugins](https://code.claude.com/docs/en/plugins)): one skill, a tight `description`, and path scoping, per the factory note.
  - **Vendor coupling:** mitigated by generating from `AGENTS.md` and by following the open skills format.
  - **Third-party trust tier:** users see it as a third-party marketplace. Sign releases as `oskac` is signed, and consider submitting to Anthropic's directory later.

### B2. Agent Skills beyond the main one

- **What:** Small, task-scoped skills in the same plugin:
  - `oska-capabilities`: choosing and attenuating capabilities;
  - `oska-untrusted`: writing a validator instead of `.assume_trusted()`;
  - once A1 and A2 exist, `oska-mcp-server` and `oska-llm`. These teach agents to put model output in data positions and tools behind capabilities.
- **Reasoning:** Progressive disclosure keeps each skill's cost to about 100 tokens until it is used ([Agent Skills spec](https://agentskills.io/specification)). The factory note records that `claude plugin eval` can measure whether a skill helps.
- **Effort:** S each. **Risk:** too many skills dilute triggering. Add one only when the evaluation harness shows a failure class it fixes.

### B3. `oskac mcp`, distributed through the plugin

This is C1 in the AI-driven note, unchanged: check, fix, test, explain and authority diff as typed MCP tools, with `run` left out. This note adds two points:

- **It should be the Rust toolchain, not an Oska program.** An Oska MCP server (A1) cannot call the checker without `Spawn`, and `Spawn` is the broadest capability.
- **A1 gives a dogfood example.** `examples/` can carry a small Oska MCP server, and `oskac mcp` can be tested against the same JSON-RPC fixtures.

### B4. Authority review on pull requests with `claude-code-action`

- **What:** A workflow on `pull_request` that:
  1. runs `oskac check --baseline` (B1 in the AI-driven note), which fails the job on any unapproved widening;
  2. runs `anthropics/claude-code-action@v1` with the `oska` plugin and a prompt that invokes the `authority-reviewer` agent. Claude posts a comment explaining each widening, new audit site and new attenuation root, in plain language, with the file and line.
- **Reasoning:**
  - The factory note's main finding was "Rules that must hold belong in the harness, not in prose". The deterministic gate decides, and the model explains. This is the *workflow* shape, "predefined code paths", recommended for predictable tasks ([building effective agents](https://www.anthropic.com/engineering/building-effective-agents)).
  - The guide calls reading the report the review step ([capabilities guide](../guide/capabilities.md)). A comment puts it in front of a human reviewer.
- **Fit:** CI only; no compiler change beyond `--baseline`. Use `--max-turns` and a job timeout ([GitHub Actions](https://code.claude.com/docs/en/github-actions)), and the factory note's `timeout-minutes` rule.
- **Effort:** S once `--baseline` exists. **Risks:**
  - **Fork PRs get no secrets** ([GitHub Actions](https://code.claude.com/docs/en/github-actions)), so contributor PRs get only the deterministic gate.
  - **Running a model on attacker-written diffs is itself an injection surface.** Give the action read-only permissions and only the comment tool.

### B5. Purpose-built agents: use the CLI first, the Agent SDK later

- **Candidates:**
  - **A migration agent** for breaking prelude changes. ADR 0008 changed the prelude "with no deprecation period". The mechanical part belongs to `oskac fix` (A2 in the AI-driven note), and an agent handles the rest.
  - **A test generator** that writes property tests from contracts (E1 in the AI-driven note).
  - **The evaluation-harness agent** (#11), already designed around `claude -p`.
- **Recommendation:** build these as plugin skills run by `claude -p` or `claude-code-action`, not as Agent SDK applications. The SDK is Python and TypeScript only, and other languages can "run the CLI as a subprocess" ([Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview)). A Python or TypeScript service would be the project's first non-Rust, non-Oska production code. "Start by using LLM APIs directly" and add complexity only when needed ([building effective agents](https://www.anthropic.com/engineering/building-effective-agents)).
- **Managed Agents** would suit long, scheduled jobs such as a nightly evaluation run ([Managed Agents](https://platform.claude.com/docs/en/managed-agents/overview)), but it is in beta and not eligible for zero data retention. The harness runs fine on the Pi with `claude -p` today.
- **Effort:** S–M each, as skills. **Risk:** agents that write tests can encode their own misunderstanding. Contracts as oracles (E1) are the independent check.

### B6. A claude.ai and Cowork variant

The same plugin format installs on claude.ai and in Cowork, but "a different set of components loads" there ([plugins](https://code.claude.com/docs/en/plugins)). There is no local `oskac` and no hooks, so only the skills would help. Low value until there is a hosted checker; not recommended now.

## Ranked roadmap

The ranking weighs security value given Oska's goals, what can run without networking, dependency order, and effort. Items marked **(prereq)** unblock others.

**Near term: no network needed.**

1. **Fix the probe's gap: report generic capability-reaching parameters.** S–M. **(prereq)** for A1 and A4. It is also a correctness issue in the report today.
2. **B1, steps 1–2: the plugin with the generated skill and the fixed hook.** S, after the AI-driven note's A1 hook fix.
3. **A0: `std.json`, trust-aware.** M. **(prereq)**
4. **A1: `std.mcp` and `Toolset` over stdio.** M. The first runtime AI feature that actually runs.
5. **A2 signatures and A3 `t.model`.** S–M. Lets `std.llm` be designed and tested against real programs.
6. **A4's `Approval` pattern, as a documented library idiom.** S.
7. **B4: authority review in CI,** once `--baseline` lands. S.

**Mid term: the `NetConnect` runtime, no TLS.**

8. **An HTTP/1.1 client in Oska, and A7 local models over loopback.** M.
9. **A5: runtime `Budget`.** M.
10. **A4: `run_tools` and the Plan-Then-Execute helper.** M.
11. **A6: the `Schema` derivation,** with an ADR. M.
12. **B3: `oskac mcp` in the plugin; B2's extra skills,** if evaluations justify them. M, S.

**Long term: TLS (ADR 0012) and beyond.**

13. **A2 running against the Claude API,** and A8's host-attenuated `NetConnect`. L, dominated by TLS.
14. **A8: the MCP client, and MCP over HTTP from running apps.** L.
15. **B5: skill-based migration and test-generation agents,** where the evaluation harness shows value. M.

## Recommendation: a Claude Code plugin for Oska

**Build it now, small, and from this repository.** First version:

- **One skill, `oska`,** generated from `AGENTS.template.md` and the agent guide, valid under the Agent Skills spec. It carries the loop (check, fix, test, authority), the untrusted-data rules, and a `references/` copy of the prelude reference once D2 in the AI-driven note exists. Scope it to `**/*.osk` and `oska.toml`.
- **The post-edit check hook and the `Stop` gate,** only after the hook checks the package rather than the edited file. Shipping today's hook in a plugin would distribute its false errors to every user.
- **A marketplace file at the repository root,** validated in `scripts/check.sh` with `claude plugin validate` if the CLI is available on CI, and with a JSON-shape test otherwise.
- **Tests,** by extending `crates/oskac/tests/agent_setup.rs`, so the plugin's files, like today's setup files, mention only flags and commands that exist.

Add `oskac mcp`, the authority-reviewer agent and the LSP server as each lands. Do not ship runtime skills (`oska-llm`, `oska-mcp-server`) until A1 and A2 exist. Do not build an Agent SDK service.

## Caveats

- **Nothing here is measured on Oska.** The claim that Oska's guarantees make agent code safer rests on the mapping to OWASP's mitigations and on one probe, not on an evaluation. The evaluation harness (#11) could add an "LLM application" task class, such as "summarize a page and store the result in SQL", to test it.
- **The guarantees bound the blast radius; they do not stop injection.** OWASP says no fool-proof prevention is known ([LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)). An injected agent can still misuse every capability it holds, and can leak data through data positions.
- **Fast-moving sources.** The Claude API, Claude Code plugins and MCP all changed during 2026: MCP has versions dated 2025-11-25 and 2026-07-28, and the plugin docs mention features gated on recent Claude Code versions. Re-check the cited pages before implementing.
- **The comparison table relies on vendor docs.** Deno's, Pydantic AI's and the AI SDK's own documentation describe their features. I did not test them.
- **The probe ran once,** on a debug build, with one program. The `pure` label for generics follows from the documented rule ([`checker.md:29`](../checker.md)), so I expect it to hold generally, but I tested only this case.
- **Not read:** the CaMeL and design-patterns papers beyond their abstracts and §3; the full Pydantic AI retry behavior (the tools page did not describe `ModelRetry`); Miller's thesis beyond its abstract page.

## Open questions for the user

1. **Should model access be its own capability kind?** A `std.llm.Model` that is a `cap struct` over `NetConnect` is cheap. A distinct kind, attenuated from `NetConnect` and counted in package authority, makes "this dependency now calls a model" a widening that needs approval. *Recommended:* the `cap struct` now (A2), and decide on a distinct kind together with A8's host attenuation, since both change how network authority is counted.
2. **The probe's gap.** Should a generic function that calls trait methods on a type parameter be reported as not pure, for example `generic T (tools)`? *Recommended:* yes, as a bug fix before A1, filed as [#149](https://github.com/aparsons/oska/issues/149). I did not file it. The empty `fn :` heading for test attenuations is a separate, smaller issue.
3. **Split milestone 4?** Shipping the `NetConnect` runtime and an HTTP client before TLS enables local models (A7) and plain-HTTP testing months earlier. *Recommended:* split, if the TLS work is long.
4. **Budget as a root capability.** Should `main` have to declare a `Budget` (visible spending authority), or should `std.llm` back it silently? *Recommended:* declared in `main`, consistent with "a function signature tells you everything the function can do".
5. **Where the plugin lives.** In this repository (versioned with `oskac`) or in a separate `oska-claude` repository? *Recommended:* this repository, so the `agent_setup` tests cover it.
6. **How far should runtime AI go in `std`?** `std.mcp` and `std.llm` as standard library, or as first-party packages once packages exist (#12)? *Recommended:* `std` for the trust-critical pieces (`json`, the `Toolset` trait, `Reply.text` being untrusted), and packages for provider-specific clients.

## All sources fetched

| Source | Kind | Used for |
| --- | --- | --- |
| [Claude API: tool use overview](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview) | primary | Client tools, `tool_use`/`tool_result`, pricing |
| [Claude API: strict tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/strict-tool-use) | primary | Schema-conformant tool inputs |
| [Claude API: structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs) | primary | Supported schema subset |
| [Claude API: prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) | primary | Usage fields, cache pricing |
| [Claude API: rate limits](https://platform.claude.com/docs/en/api/rate-limits) | primary | 429s, spend caps, headers |
| [Claude Managed Agents](https://platform.claude.com/docs/en/managed-agents/overview) | primary | Hosted harness, beta, data retention |
| [Claude Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview) | primary | Languages, CLI subprocess option |
| [Claude Code: plugins](https://code.claude.com/docs/en/plugins) | primary | Plugin components, context cost, trust tiers |
| [Claude Code: create a marketplace](https://code.claude.com/docs/en/plugin-marketplaces) | primary | `marketplace.json`, sources, validation |
| [Claude Code: GitHub Actions](https://code.claude.com/docs/en/github-actions) | primary | `claude-code-action`, plugins input, fork secrets |
| [Agent Skills specification](https://agentskills.io/specification) | primary | `SKILL.md` format, progressive disclosure |
| [MCP 2026-07-28: tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) | primary | Tools, annotations, security considerations |
| [MCP 2025-11-25: transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) | primary | stdio framing, Streamable HTTP security |
| [MCP 2025-11-25: security best practices](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices) | primary | Local server compromise, sessions, SSRF |
| [OWASP LLM Top 10 2025](https://genai.owasp.org/llm-top-10/) | primary | Risk list |
| [OWASP LLM01 prompt injection](https://genai.owasp.org/llmrisk/llm01-prompt-injection/) | primary | Mitigations, no fool-proof prevention |
| [OWASP LLM05 improper output handling](https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/) | primary | Parameterization, encoding |
| [OWASP LLM06 excessive agency](https://genai.owasp.org/llmrisk/llm062025-excessive-agency/) | primary | Root causes, mitigations |
| [OWASP LLM10 unbounded consumption](https://genai.owasp.org/llmrisk/llm102025-unbounded-consumption/) | primary | Denial of wallet, quotas |
| [Anthropic: building effective agents](https://www.anthropic.com/engineering/building-effective-agents) | primary | Workflows vs agents, start simple, ACI |
| [Anthropic: writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents) | primary | Tool design principles |
| [CaMeL, arXiv 2503.18813](https://arxiv.org/abs/2503.18813) ([HTML](https://arxiv.org/html/2503.18813)) | primary | Capabilities on values, P-LLM/Q-LLM, limits |
| [Design patterns for securing LLM agents, arXiv 2506.08837](https://arxiv.org/html/2506.08837) | primary | Six patterns, guiding principle |
| [Miller, *Robust Composition* (2006)](https://papers.agoric.com/papers/robust-composition/abstract) | primary | Object-capability background |
| [Deno security and permissions](https://docs.deno.com/runtime/fundamentals/security/) | primary | Permission flags, per-thread privilege |
| [WASI](https://wasi.dev/) | primary | No ambient authority for components |
| [Pydantic AI: tools](https://pydantic.dev/docs/ai/tools-toolsets/tools/) | primary | Schemas from types, `RunContext` |
| [Pydantic AI: testing](https://pydantic.dev/docs/ai/guides/testing/) | primary | `TestModel`, `ALLOW_MODEL_REQUESTS` |
| [Vercel AI SDK: tool calling](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling) | primary | Validated tool calls, repair, multi-step |
| [Ollama: Anthropic compatibility](https://docs.ollama.com/api/anthropic-compatibility) | primary | Local `/v1/messages` endpoint |
