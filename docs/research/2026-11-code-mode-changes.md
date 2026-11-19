# What changes would Oska need to support "code mode" best: as an MCP tool provider for code-mode hosts, as the language the model writes in code mode, and as the language agents write during development? Ranked by value and cost.

- **Date of access:** 2026-11-19 (all web sources and issues).
- **Builds on:** [What is "code mode", and how can Oska be used with it?](2026-11-code-mode.md) (the prior note: what code mode is, the vendor figures, the first fit and gap analysis). This note does not repeat it. It turns its gaps into ranked, testable changes. It also uses [Agent scripting: Python vs Oska](2026-11-agent-scripting-python-vs-oska.md) for measured agent behaviour.
- **Method:** I read 10 web pages and ran 3 web searches (list at the end). In the repo I read `README.md`, `GLOSSARY.md`, `docs/design.md`, `docs/runtime.md`, the ADRs, `docs/guide/` (what-works-today, capabilities, cli-reference), `crates/oska-check/src/prelude.osk` and `crates/oska-check/src/std/*.osk`, and the open issues #229, #248, #249–#256, #367, #206, #207, #228, #209, #275, #181 and #3 (through the REST API). I ran no code from the web.
- **Roles:** **(a)** Oska programs as MCP tools that a code-mode host calls. **(b)** Oska as the language the model writes in the sandbox. **(c)** Oska as the language agents write during development.
- **Value:** High, Medium or Low, for code mode only. **Cost:** S (one ticket, no new language rule), M (several tickets or one ADR), L (a milestone or a new backend). Statements that are my reasoning are marked **Inference:**.

## Summary table

Rank is by value per cost, then by what can start now. [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md) freezes new syntax, types, `std` APIs and capabilities until the parity gate ([#181](https://github.com/aparsons/oska/issues/181)); tooling "that does not change what a program does" carries on ([ADR 0014, line 7](../adr/0014-every-supported-platform-builds-and-runs-natively.md)). The "Now?" column applies that rule (**Inference**).

| # | Change | Role(s) | Value | Cost | Covered by | Depends on | Now? |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `oskac check` as a snippet verifier: source from stdin, a grant file, one JSON verdict | b, c | High | S | new (builds on `--baseline`, `--format=json`) | none | Yes |
| 2 | Output schema and structured results on `std.mcp` tools | a | High | S | partly #229 / #250 (`structuredContent` yes, `outputSchema` no) → amend | 4 | Spec now; code after #181 |
| 3 | Per-tool authority manifest, mapped to MCP annotations and to host approval | a | High | S | #229 stories 41–49, #249; the export to `tools/list` is new | 2 | Spec now |
| 4 | `std.json` (trust-aware) | a, b, c | High | M | #248 (#229 part 1) | #209, #181 | No |
| 5 | Trust flag on tool results, visible on the wire | a, b | Medium | S | partly #229 (in-language); wire marking is new | 2, 3 | Spec now |
| 6 | Resource limits: steps, wall clock, memory, output size | a, b | High | M | new (only `P0021`/`P0037` exist; #251 `Budget` is tokens) | none | Yes, as `run` flags |
| 7 | `oskac mcp`: the toolchain as an MCP server shaped for programmatic calls | c | Medium | M | named, out of scope in #206, #367, #275 → new spec | 1 | Yes |
| 8 | Schemas derived from Oska types (input and output) | a, b | Medium | M | out of scope in #229 ("A6", needs an ADR) → new | 2, 4 | ADR now |
| 9 | Typed TypeScript and Python declarations for Oska tools | a | Medium | M | new | 2, 8 | After 2 |
| 10 | Agent ergonomics: raw string literal, `oskac run -`, a short scripting card | b, c | Medium | S–M | new (agent-scripting note items 2, 3, 7); docs part near #256 | none | Docs and CLI yes; syntax no |
| 11 | Host-granted capabilities (a binding model that lifts `E0211`) | b | High (for b) | L | new; design notes name WASM imports "later" | 4, 8 | ADR only |
| 12 | An embeddable runtime: the interpreter or a backend on WebAssembly | b | Medium | L | new; WASM backend out of scope for v0 in #3 | 11 | No |
| 13 | Streamable HTTP transport for `std.mcp` | a | Medium | L | out of scope in #229; needs #228 | #228 | No |
| 14 | A code-mode variant in the evaluation harness | b, c | High (decides 11–12) | S | #207 (add a variant) | 1, 7 | Yes |
| 15 | Floats and a map type | b, c | Medium | L | new (`std.json` keeps numbers as text in #229) | #181 | No |

## 1. `oskac check` as a snippet verifier

**What code mode needs.**
- Both vendors contain the model's code while it runs. They do not check it before it runs (prior note, [§1.3](2026-11-code-mode.md)).
- Cloudflare's executor takes "the code string" and returns `result`, `error` and `logs` ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)). A verifier must take a string, not a file.
- `normalizeCode` turns common model output, with Markdown fences, into the form the executor runs ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)). Inference: model output often needs a cleaning step before a check.

**What Oska has today.**
- `oskac check --format=json` reports diagnostics and each function's authority ([cli-reference.md, line 11](../guide/cli-reference.md); [design.md, line 131](../design.md)).
- `--baseline=<file>` exits with 3 when authority widened ([cli-reference.md, line 80](../guide/cli-reference.md)).
- `check` takes a file or a package; a path it cannot read is exit 1 ([cli-reference.md, lines 65 and 72](../guide/cli-reference.md)). `oskac run -` fails ([agent-scripting note, §2](2026-11-agent-scripting-python-vs-oska.md)).
- Startup is about 4 ms of CPU for `check` ([agent-scripting note, §3](2026-11-agent-scripting-python-vs-oska.md)).

**The change.**
- Accept `-` as the path for `check` and `run`: read one module from stdin.
- Add `--grant=<file>`: an authority baseline that the host writes. A widening is exit 3. This is `--baseline` under a host's name. Inference: an alias may be enough.
- With `--format=json`, print one object with `verdict` (`ok`, `diagnostics`, `widened`), the diagnostics and the authority.
- Strip one surrounding Markdown code fence, with a note.

**Acceptance criteria.**
- `printf '%s' "$SRC" | oskac check - --format=json` gives the same diagnostics as the same text in a file, with `path` `"<stdin>"`.
- `oskac check - --grant=g.json` exits 0, 1 or 3 as `--baseline` does, and `verdict` matches the exit code.
- `oskac run - --interpret -- args` runs the checked stdin program. Its stdin is then empty, and the docs say so.
- A fenced snippet checks the same as the unfenced one and prints one note.
- A CLI golden test covers each case on both backends.

## 2. Output schema and structured results on `std.mcp` tools

**What code mode needs.**
- An MCP tool may have an `outputSchema`. If it does, servers "MUST provide structured results that conform to this schema", and clients "SHOULD validate" them ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- The schema gives "type information for better integration with programming languages" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- A tool with structured content "SHOULD also return the serialized JSON in a TextContent block" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- Anthropic PTC gives each tool to code as a function that returns a string. Claude parses it, for example with `json.loads` ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- PTC advises: "document the format (JSON structure and field types)" in the tool description ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- A Claude API tool definition has no output-schema field ([Define tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)).

**What Oska has today.**
- `ToolSpec` holds a name, a title, a description and an input schema. `ToolResult` holds text, optional structured content and an error flag ([#229, Implementation Decisions](https://github.com/aparsons/oska/issues/229)).
- A structured result also carries its JSON text ([#229, story 25](https://github.com/aparsons/oska/issues/229)).
- No output schema. The tickets are #249 (`Toolset`) and #250 (server) ([#229 tickets list](https://github.com/aparsons/oska/issues/229)).

**The change.**
- Add an optional `output: Option<json.Schema>` to `ToolSpec`.
- `tools/list` prints it as `outputSchema`.
- `serve` validates each `ToolResult.json(...)` against it before it sends. A mismatch is an `isError: true` result that names the JSON path, never a non-conforming result.
- Add a helper that renders the output schema as a short text block. The developer can append it to the description, for PTC hosts that see only the description (**Inference**).

**Acceptance criteria.**
- A peer transcript shows `outputSchema` in `tools/list` for a tool that declares it, and no key for one that does not.
- A result that does not match its schema gives `isError: true` with the path. A matching one gives `structuredContent` and the same JSON as text.
- A tool with an output schema that returns only `ToolResult.text` fails at `serve` start or at the call, as the spec decides, with a stable error.
- The schema subset is the one #229 already uses for input.

## 3. Per-tool authority manifest, mapped to annotations and approval

**What code mode needs.**
- MCP annotations are `readOnlyHint`, `destructiveHint`, `idempotentHint` and `openWorldHint`. Clients "MUST consider tool annotations to be untrusted unless they come from trusted servers" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- Cloudflare's code-mode hook marks a method `requiresApproval: true`, and the run pauses before that method ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)). The page does not map annotations to approval ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)).
- Anthropic's MCP connector has per-tool allow and deny lists. It suggests a deny list "when building read-only assistants" ([MCP connector](https://platform.claude.com/docs/en/agents-and-tools/mcp-connector)).

**What Oska has today.**
- The checker gives each tool its own authority entry, from the literal `match` arms of `call` ([#229, stories 41–43](https://github.com/aparsons/oska/issues/229)).
- `serve` derives `readOnlyHint`, `destructiveHint` and `openWorldHint`; `idempotentHint` is never sent; the developer cannot set them ([#229, Implementation Decisions](https://github.com/aparsons/oska/issues/229)).
- The same entries are in `oskac check --format=json` ([#229, story 49](https://github.com/aparsons/oska/issues/229)).

**The change.**
- Also publish the per-tool authority on the wire, in each tool's `_meta`, under one namespaced key (for example the capability kinds and roots). Inference: a host can then decide approval from the authority itself, not from four booleans.
- Add `oskac check --tools-manifest=<file>`: one JSON file per server with each tool's name, annotations and authority. A host can pin it, as it pins a baseline.
- Document one mapping for hosts: `destructiveHint: true` or any network authority → `requiresApproval` on Cloudflare, deny-by-default on the Anthropic connector (**Inference**).

**Acceptance criteria.**
- For the example server, the `_meta` authority in `tools/list` equals the per-tool entry in `oskac check --format=json`, in a golden test.
- The manifest file is byte-identical on every platform and changes only when a tool's authority changes.
- A tool that gains a write capability changes its manifest entry and its `destructiveHint`, and `--baseline` exits 3.
- The guide shows the host mapping and says that annotations stay hints for an untrusted server.

## 4. `std.json`

**What code mode needs.**
- Most code-mode tool results are JSON that the model's code parses ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- MCP structured content "can be any JSON value" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).

**What Oska has today.**
- No JSON parser. 6 of 29 measured agent scripts parsed JSON and could not be written in Oska ([agent-scripting note, §1](2026-11-agent-scripting-python-vs-oska.md)).
- #248 asks for `json.parse` that returns an untrusted value, `json.decode`, canonical `json.encode`, and limits ([#248](https://github.com/aparsons/oska/issues/248)).
- #229 keeps numbers as decimal text, so no float type is needed ([#229, Implementation Decisions](https://github.com/aparsons/oska/issues/229)).
- It waits for #209 and #181 ([#229, Further Notes](https://github.com/aparsons/oska/issues/229)).

**The change.** No new change. Keep #248 first in the order, because changes 2, 3, 5, 8 and 11 need it. Inference: it is the one prerequisite shared by all three roles.

**Acceptance criteria.** Those of #248. Add one: a code-mode example (a tool result string from a host) parses to a `Value`, and an extracted string is rejected in a structural position (`E0300`).

## 5. Trust flag on tool results, on the wire

**What code mode needs.**
- PTC: results "can contain any content, including code snippets or executable commands"; "be aware of code injection risks" ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- MCP clients SHOULD "Validate tool results before passing to LLM", and servers MUST "Sanitize tool outputs" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).
- MCP content can carry annotations for audience and priority ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)). The spec has no trust or provenance field ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).

**What Oska has today.**
- A trust flag on text; untrusted text cannot reach a trusted slot ([ADR 0006, lines 5–9](../adr/0006-trust-is-a-flag-on-text.md)).
- `ToolResult` text is an untrusted value. Printing a `Value` that holds untrusted text gives an untrusted string ([#229, story 11 and Implementation Decisions](https://github.com/aparsons/oska/issues/229)).
- The flag stops at the process edge: the JSON-RPC output has no mark (**Inference** from #229's wire format).

**The change.**
- In role (a): when a result's text or structured content holds untrusted text, `serve` adds one namespaced `_meta` flag to the result, set by the runtime, not the developer. Inference: a host that trusts the server can then wrap that content as data.
- In role (b): every value that a host tool returns into Oska code is untrusted text (falls out of change 11).
- Say plainly that this marks provenance; it does not stop injection ([#229, story 93](https://github.com/aparsons/oska/issues/229)).

**Acceptance criteria.**
- A tool that echoes an argument gets the flag; a tool that returns only literals does not.
- The developer has no API to clear the flag except an audited `assume_trusted`, which `oskac audit` lists.
- Peer transcripts cover both cases on both backends.

## 6. Resource limits

**What code mode needs.**
- PTC: each REPL cell has a 90-second wall-clock limit; a pending tool call times out after about 4 minutes ([code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool); [PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- Cloudflare: executor timeout 60,000 ms by default ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)); per-run `cpuMs` and `subRequests`, and the Worker "immediately throws" at a limit ([Dynamic Workers limits](https://developers.cloudflare.com/dynamic-workers/usage/limits/)).
- MCP servers MUST rate-limit tool calls; clients SHOULD time them out ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).

**What Oska has today.**
- A call-depth limit (`P0021`, 50,000 calls) and a 256 MiB text limit for `repeat` (`P0037`) ([runtime.md, lines 137 and 140](../runtime.md)).
- No step, time or memory limit in `crates/oska-run/src` (a `grep` for `fuel`, `timeout`, `deadline` and `max_steps` finds nothing).
- `Budget` (#251) counts model tokens and calls, not CPU ([#229, Implementation Decisions](https://github.com/aparsons/oska/issues/229)).

**The change.**
- `oskac run` flags: `--max-steps=<n>` (interpreter fuel), `--timeout=<ms>` (wall clock), `--max-memory=<bytes>` and `--max-output=<bytes>`.
- Each limit stops the program with its own stable panic code, exit 101, on both backends.
- Inference: a native backend needs a step counter at loop back-edges and calls; the differential suite must hold both to the same step count, or the native limit is time only.

**Acceptance criteria.**
- `while true {}` under `--max-steps=1000000` stops with the new code on both backends, in a differential test.
- `--timeout=100` stops a sleeping or looping program within 100 ms plus a documented tolerance.
- A program that writes 10 MiB to stdout under `--max-output=1048576` stops after at most 1 MiB.
- With no flag, behaviour is unchanged (a golden test).

## 7. `oskac mcp`: the toolchain as tools for programmatic calls

**What code mode needs.**
- PTC suits fan-out and loops; "sequential single-call workflows do not benefit" (prior note, [§2](2026-11-code-mode.md)).
- Results should be machine-readable, with a documented format ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- MCP connector tools cannot be called from PTC code ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)), and the connector reaches only public HTTPS servers, not stdio ([MCP connector](https://platform.claude.com/docs/en/agents-and-tools/mcp-connector)).

**What Oska has today.**
- `oskac mcp` is named and put out of scope in #206, #367 and #275 ([#206](https://github.com/aparsons/oska/issues/206), [#367](https://github.com/aparsons/oska/issues/367), [#275](https://github.com/aparsons/oska/issues/275)). #275 says it "may later share this core" with `oskac lsp` ([#275](https://github.com/aparsons/oska/issues/275)).
- `check`, `fix`, `explain` and the post-edit hook already print JSON ([cli-reference.md, lines 9–61 and 241](../guide/cli-reference.md)).

**The change.** Write the spec. Tools: `check`, `fix`, `explain`, `authority_diff`, `run` (with change 6's limits), each with an output schema (change 2's shape). Keep it stdio and local. Inference: for PTC, the developer's client wraps these as client tools, since connector tools are excluded.

**Acceptance criteria.**
- `tools/list` lists each tool with `inputSchema` and `outputSchema`; each result validates against its schema.
- `check`'s structured result equals `oskac check --format=json` for the same input, in a golden test.
- The server reaches no network and writes only the files `fix` writes.

## 8. Schemas derived from Oska types

**What code mode needs.**
- Cloudflare generates the API from tool schemas; unsupported schemas become `unknown` ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)). Inference: a precise schema is what makes the generated types useful.
- PTC rejects tools whose input schema has a recursive `$ref` ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).

**What Oska has today.**
- Schemas are built by hand with `json.Schema`. "The built-in `Schema` derivation from types (A6), which needs its own ADR" is out of scope ([#229, Out of Scope](https://github.com/aparsons/oska/issues/229)).
- No user macros ([ADR 0003](../adr/0003-no-user-defined-macros.md)), so derivation must be built in.

**The change.** An ADR for one built-in derivation: a struct or enum of supported field types gives a closed object schema and a decoder, in the #229 subset. Recursive types are refused (`$ref` is not in the subset).

**Acceptance criteria.**
- For each supported type shape, the derived schema equals a fixture, and `decode(encode(x)) == x` in a property test.
- A recursive type is a checker error with a stable code.
- A derived output schema used in `ToolSpec` passes change 2's validation for every value of the type.

## 9. Typed TypeScript and Python declarations for Oska tools

**What code mode needs.**
- Cloudflare: "Give your agent TypeScript type declarations with doc comments describing each method" ([Dynamic Workers bindings](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)); a TypeScript API is "fewer tokens and much easier to understand" ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)).
- Anthropic's pattern presents each tool as a file with a typed wrapper (prior note, [§1.1](2026-11-code-mode.md)). PTC exposes tools as async Python functions ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).

**What Oska has today.** Nothing. Inference: the checker already knows each tool's name, schemas and authority (changes 2, 3), so it can print declarations offline.

**The change.** `oskac mcp-types <package> --lang=ts|python`: one declaration per tool, with input and output types from the schemas, the description as a doc comment, and the authority and annotations as a comment line.

**Acceptance criteria.**
- For the example server, the TypeScript output type-checks with `tsc --strict` and the Python output with a type checker, in CI where available.
- The TypeScript output equals what `generateTypesFromJsonSchema()` would need: no `unknown` for any schema in the subset (**Inference**; checked by a fixture, not by Cloudflare code).
- Output is deterministic and byte-identical on every platform.

## 10. Agent ergonomics

**What code mode needs.**
- Cloudflare's main argument: models write well in languages they "have seen a lot of" (prior note, [§1.2](2026-11-code-mode.md)).

**What Oska has today.**
- 7 probe programs needed 3 extra drafts; programs were 2 to 6 times longer than Python ([agent-scripting note, §3](2026-11-agent-scripting-python-vs-oska.md)).
- `{` in a literal starts interpolation; a brace around a name in scope substitutes silently ([agent-scripting note, §2](2026-11-agent-scripting-python-vs-oska.md)).
- Machine fixes have an `applicability` of `machine`, `maybe` or `placeholders` ([cli-reference.md, line 328](../guide/cli-reference.md)). None of the codes the probes hit had a fix ([agent-scripting note, §4](2026-11-agent-scripting-python-vs-oska.md)).

**The change.**
- A raw string literal with no escapes and no interpolation (syntax: frozen until #181).
- `oskac run -` (change 1).
- A one-page "code-mode card" in the docs and in `llms.txt`: `main`'s capabilities, `restrict_to`, `"{a}{b}"`, call-style constructors, `Untrusted`. Near #256 (agents docs).
- Machine fixes for `E0223` (`+` on text) and the Rust struct-literal form.

**Acceptance criteria.**
- The card is under 2 KB and every example in it checks in CI.
- `E0223` on `a + b` with text operands has a `machine` fix to `"{a}{b}"`.
- After the raw literal lands, the agent-scripting class A example needs no escapes.

## 11. Host-granted capabilities (a binding model)

**What code mode needs.**
- Cloudflare: a Dynamic Worker "can only access what you explicitly give it"; stubs "cannot be forged"; "The only way to obtain one is to receive it" ([Dynamic Workers bindings](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)).
- Each call on a binding "makes a call back to your loader Worker, where the method runs", and per-user `props` stay hidden from the sandbox ([Dynamic Workers bindings](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)).
- PTC pauses the code at each tool call and returns a `tool_use` block to the client ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).

**What Oska has today.**
- `main` takes only built-in capabilities and program inputs (`E0211`) ([capabilities.md, line 36](../guide/capabilities.md)).
- v0 has no FFI; WASM component imports come "later", typed with the capabilities they need ([design.md, lines 489–490](../design.md)). The mapping to WASI is an open question ([design.md, line 535](../design.md)).
- Syscalls exist only in `std.rt` ([ADR 0013](../adr/0013-system-calls-only-in-std-rt.md)).
- Inference: the model is already capability-shaped. A `cap struct` holds only narrowed capabilities ([#229, Implementation Decisions](https://github.com/aparsons/oska/issues/229)); a host binding is one more kind of narrowed capability.

**The change.** An ADR for a **host interface**: a package declares a body-less `cap struct` (for example `GithubTools`) with typed methods. `main` may take it. The host supplies its implementation at start, over one channel (stdin/stdout JSON-RPC for a process, an import for WASM). Every returned text is `Untrusted`. The authority report names it as its own kind.
- Name: avoid "host capability", which #228 uses for a network capability confined to one host ([#228, story 1](https://github.com/aparsons/oska/issues/228)).

**Acceptance criteria.**
- A `main(gh: GithubTools, io: Stdout)` program checks; without the declaration it is still `E0211`.
- `oskac check` prints the interface kind in the function's authority, and `--grant` can widen or narrow it.
- A test host fakes the interface, as `t.model` fakes a model ([#229, story 84](https://github.com/aparsons/oska/issues/229)); tests never call the real host.
- A returned string is refused in each structural position.

## 12. An embeddable runtime (WebAssembly)

**What code mode needs.**
- Cloudflare runs code in V8 isolates; Workers "can use Python and WebAssembly", but "for small snippets of code … JavaScript will load and run much faster" ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)).
- Anthropic's container runs Python 3.11 on x86_64 Linux, with no internet and no package install ([code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)). Files can be uploaded through the Files API ([code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)).

**What Oska has today.**
- Native backends for Linux and macOS, and an interpreter in Rust ([design.md, line 502](../design.md)). A Linux executable is static ([README.md](../../README.md)).
- The WASM backend is out of scope for v0 ([#3](https://github.com/aparsons/oska/issues/3)).

**The change.**
- First: compile the interpreter (`oska-check` plus `oska-run`) to `wasm32` with an in-memory host, the one tests use ([runtime.md, line 79](../runtime.md)), and change 11's interface as imports. Inference: this reuses the checked program and the differential suite, and needs no new code generator.
- Later: a WASM backend.
- Inference, not verified: an uploaded static `oskac` for linux-x86_64 might run in Anthropic's container. The docs do not say whether uploaded binaries may run ([code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)). Test it before planning on it.

**Acceptance criteria.**
- The WASM build runs the differential corpus that needs no real OS and gives identical output to the interpreter.
- Startup and the check of a 50-line snippet are measured and published beside the JavaScript baseline.
- No filesystem, network or clock is reachable except through imports.

## 13. Streamable HTTP for `std.mcp`

**What code mode needs.** The Anthropic MCP connector needs a public `https://` server over Streamable HTTP or SSE; "Local STDIO servers cannot be connected directly" ([MCP connector](https://platform.claude.com/docs/en/agents-and-tools/mcp-connector)).

**What Oska has today.** `std.mcp` is stdio only; Streamable HTTP is out of scope ([#229, Out of Scope](https://github.com/aparsons/oska/issues/229)). The network and TLS wait for #228 ([#228](https://github.com/aparsons/oska/issues/228)).

**The change.** A follow-on spec after #228. Inference: the value is lower for code mode, because PTC cannot call connector tools ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)), and a local stdio bridge serves the developer's own client.

**Acceptance criteria.** The same peer transcripts as #229 pass over HTTP on the loopback test server of #228.

## 14. A code-mode variant in the evaluation harness

**What code mode needs.** Evidence that models write Oska well enough (prior note, [§3.1](2026-11-code-mode.md)).

**What Oska has today.** #207 specifies batch variants for ablations and records iterations to a clean check ([#207](https://github.com/aparsons/oska/issues/207)).

**The change.** Add a task class: the agent gets a small set of tools and must write one snippet that calls them, in Oska (checked by change 1) and in Python as the baseline. Record drafts, tokens and authority violations.

**Acceptance criteria.** A batch summary reports func@k, drafts to a clean check and widenings refused by `--grant`, for both languages, from recorded transcripts in tests.

## 15. Floats and a map type

**What code mode needs.** Tool results carry prices, scores and keyed records ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools), the weather example uses `22.5`).

**What Oska has today.** No float and no map in the prelude ([agent-scripting note, §2](2026-11-agent-scripting-python-vs-oska.md)). `std.json` avoids floats by keeping number text ([#229](https://github.com/aparsons/oska/issues/229)).

**The change.** New types after #181, each with an ADR. Inference: role (a) does not need them; roles (b) and (c) do.

**Acceptance criteria.** Defined by the ADRs; at least: JSON numbers with a fraction round-trip through the float accessor, and a map iterates in a deterministic order on every platform.

## Recommendation

1. **Change 1, the snippet verifier.** It is small, it is tooling and so not frozen, and it is Oska's one unique offer: a static "what can this snippet do" before any sandbox time (prior note, [§3.1](2026-11-code-mode.md)). It also feeds changes 7 and 14.
2. **Changes 2 and 3, as one amendment to #229.** Output schema and the per-tool authority on the wire are spec text now and small tickets beside #249 and #250. They make an Oska server the best-typed and best-described tool provider a code-mode host can call, and both vendors' code-mode paths read schemas ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/), [PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
3. **Change 6, resource limits.** Every code-mode host bounds time ([code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool), [Dynamic Workers limits](https://developers.cloudflare.com/dynamic-workers/usage/limits/)), and Oska has none. It is needed for roles (a) and (b), and the `run` flags can land without a language change.

Inference: do not start changes 11 and 12 until change 14 shows that models write Oska snippets at a rate near Python's. They are the largest costs, and their value depends on that data.

## Notes on page content

- The MCP specification page and the Cloudflare docs pages (Code Mode API reference, Code Mode with MCP, Dynamic Workers bindings and limits) open with a "Documentation Index" banner that tells automated readers to fetch an `llms.txt` file "before exploring further". I treated it as page content and did not follow it.
- The Dynamic Workers bindings page says "Give your agent TypeScript type declarations…". This is guidance for developers; I cite it as a fact about the vendor's advice.

## Sources

| Source | Owner | Used for |
| --- | --- | --- |
| [MCP specification: tools (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) | MCP | `outputSchema`, `structuredContent`, annotations, security rules |
| [Programmatic tool calling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling) | Anthropic | Tools as functions returning strings, output docs, limits, timeouts, injection note |
| [Code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool) | Anthropic | Container runtime, limits, no internet, file uploads |
| [MCP connector](https://platform.claude.com/docs/en/agents-and-tools/mcp-connector) | Anthropic | HTTPS only, tools only, allow and deny lists |
| [Define tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools) | Anthropic | No output-schema field on tool definitions |
| [Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/) | Cloudflare | Executor, timeout, type generation, approval, `normalizeCode` |
| [Use MCP tools with Code Mode](https://developers.cloudflare.com/agents/tools/codemode/mcp/) | Cloudflare | Types from MCP schemas, approval hook |
| [Dynamic Workers: bindings](https://developers.cloudflare.com/dynamic-workers/usage/bindings/) | Cloudflare | Capability-style bindings, unforgeable stubs, declarations advice |
| [Dynamic Workers: limits](https://developers.cloudflare.com/dynamic-workers/usage/limits/) | Cloudflare | `cpuMs`, `subRequests` |
| [Sandboxing AI agents, 100x faster](https://blog.cloudflare.com/dynamic-workers/) | Cloudflare | WebAssembly and Python load cost, TypeScript APIs |
| Web searches (3): Dynamic Workers limits; Claude MCP connector; Claude tool output schema | — | Finding the pages above |
| [aparsons/oska#229](https://github.com/aparsons/oska/issues/229), [#248](https://github.com/aparsons/oska/issues/248), [#367](https://github.com/aparsons/oska/issues/367), [#206](https://github.com/aparsons/oska/issues/206), [#207](https://github.com/aparsons/oska/issues/207), [#228](https://github.com/aparsons/oska/issues/228), [#275](https://github.com/aparsons/oska/issues/275), [#181](https://github.com/aparsons/oska/issues/181), [#3](https://github.com/aparsons/oska/issues/3); titles of #249–#256 and #209 | Oska | What is specified, out of scope, and blocked |
| [`README.md`](../../README.md), [`docs/design.md`](../design.md), [`docs/runtime.md`](../runtime.md), [`docs/guide/capabilities.md`](../guide/capabilities.md), [`docs/guide/what-works-today.md`](../guide/what-works-today.md), [`docs/guide/cli-reference.md`](../guide/cli-reference.md), [`crates/oska-check/src/std/`](../../crates/oska-check/src/std/), `crates/oska-run/src` (grep) | Oska | What exists today |
| ADRs [0003](../adr/0003-no-user-defined-macros.md), [0006](../adr/0006-trust-is-a-flag-on-text.md), [0013](../adr/0013-system-calls-only-in-std-rt.md), [0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md) | Oska | No macros, trust flag, `std.rt`, the freeze |
| [Code mode (prior note)](2026-11-code-mode.md), [Agent scripting: Python vs Oska](2026-11-agent-scripting-python-vs-oska.md) | Oska (research) | Baseline analysis, measured drafts and gaps |
