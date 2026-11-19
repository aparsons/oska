# What is "code mode" (Anthropic and Cloudflare), and how can Oska be used in, during, or to support development with it?

- **Date of access:** 2026-11-19 (all web sources).
- **Method:** I read 13 web pages from Anthropic, Cloudflare and the MCP specification, and the Oska repo: `README.md`, `GLOSSARY.md`, `docs/adr/`, `docs/guide/what-works-today.md`, `docs/guide/capabilities.md`, `docs/guide/cli-reference.md`, `docs/design.md`, and the open spec issues [#229](https://github.com/aparsons/oska/issues/229), [#367](https://github.com/aparsons/oska/issues/367), [#206](https://github.com/aparsons/oska/issues/206) and [#207](https://github.com/aparsons/oska/issues/207). I did not run code from the web.
- **Related research:** [Agent scripting: Python vs Oska](2026-11-agent-scripting-python-vs-oska.md) (Oska as the agent's own script language) and [Claude Code software factory](2026-11-claude-code-software-factory.md) (hooks, sandboxing and MCP in this project's agent loop).
- Statements that are my reasoning and not a source's are marked **Inference:**.

## Summary

- **Code mode** means: the model does not call tools one at a time. It writes a program that calls the tools as functions. The program runs in a sandbox. Only the program's final output goes back to the model.
- **Anthropic** describes it as a pattern (MCP servers presented as code APIs) and ships one form of it, *programmatic tool calling*, in the Claude API. The model writes Python in an Anthropic-managed container.
- **Cloudflare** ships it as *Code Mode* in the Agents SDK. The model writes TypeScript or JavaScript. The code runs in a fresh V8 isolate (a Dynamic Worker). The isolate can reach only the bindings that the host gives it.
- **Why it helps:** fewer tokens (from 20–40% in Anthropic's production figures up to 98.7% and 99.9% in single examples), fewer model round trips, and data that stays out of the model's context.
- **Oska fit:** Oska's model (capabilities as the only authority, an authority report before anything runs, a trust flag on text) matches the security problem of code mode closely. But Oska today cannot be the code-mode language: `main` takes only built-in capabilities, there is no FFI, no JSON, and the network does not run.
- **Recommendation:** support code mode from the *server* side first, through `std.mcp` (#229): add an output schema to `ToolSpec` and export per-tool authority as a manifest. Measure the development-side use in the eval harness (#207). Treat Oska as a code-mode language as a later, open question.

## 1. What code mode is

### 1.1 Anthropic

**The pattern (engineering post, 2025-11-04).**

- The post names two problems of direct tool calls. Tool definitions fill the context window, and "Every intermediate result must pass through the model" ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).
- The proposal is to "present MCP servers as code APIs rather than direct tool calls" ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).
- Each server is a folder and each tool is a file with a typed (TypeScript) wrapper around the MCP call. The agent lists folders and reads only the files it needs. A `search_tools` tool is an alternative ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).
- The post states the cost: running agent code needs "a secure execution environment with appropriate sandboxing, resource limits, and monitoring" ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).

**The product: programmatic tool calling (PTC).**

- Claude writes code that calls your tools inside a code execution container. You enable it per tool with `"allowed_callers": ["code_execution_20260120"]` ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- The tools appear as async Python functions. Each takes one dict and returns a string ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- At each tool call, the code pauses and the API returns a `tool_use` block. The client runs the tool and returns the result. The result is "not loaded into Claude's context window" ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- `allowed_callers` "is not a hard API-level block on direct calls … Do not rely on `allowed_callers` as a security boundary" ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- Limits: tools from an MCP connector cannot be called from code; `strict: true` tools are not supported; results must be strings or text blocks ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- The container runs Python 3.11 on Linux, with 5 GiB RAM, 5 GiB disk and 1 CPU. "Internet access: Completely disabled for security" ([code execution tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)). With PTC, each REPL cell has a 90-second wall-clock limit ([code execution tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)).

### 1.2 Cloudflare

**The pattern (blog, 2025-09-26).**

- The Agents SDK fetches an MCP server's schema and generates a TypeScript API. Each tool becomes a typed async function. The agent gets one tool that runs TypeScript ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)).
- The argument: "LLMs have seen a lot of code. They have not seen a lot of "tool calls"" ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)).
- The code reaches MCP servers only through bindings. Global `fetch()` and `connect()` throw. The supervisor holds the access tokens, so "the AI cannot possibly write code that leaks any keys" ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)).

**The sandbox: Dynamic Workers (blog, 2026-03-24).**

- A Worker starts another Worker with code given at run time, in its own V8 isolate. An isolate "takes a few milliseconds to start and uses a few megabytes of memory", "around 100x faster and 10x-100x more memory efficient than a typical container" ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)).
- `globalOutbound: null` blocks internet access; a callback can inspect, rewrite or block each request instead ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)).
- "Custom bindings follow a capability-based security model: a Dynamic Worker can only access what you explicitly give it." "Stubs have no global identifier and cannot be forged" ([Dynamic Workers bindings docs](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)).
- The post says "your agent needs to write JavaScript"; Python and WebAssembly run too, but slower to load ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)).

**The SDK (`@cloudflare/codemode`).**

- `DynamicWorkerExecutor` needs a Worker Loader binding, has a 60 000 ms default timeout, and `globalOutbound` defaults to `null` ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)).
- `generateTypesFromJsonSchema()` turns JSON Schema into declarations; unsupported schemas become `unknown` ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)).
- Tools with `needsApproval` are left out of the simple API; durable approval needs connectors with `requiresApproval: true` ([Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)).
- `McpConnector` "creates one typed sandbox method for each discovered MCP tool" and "derives the method types from the MCP schemas". The model can call `codemode.search()` and `codemode.describe()` to load definitions on demand ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)).
- Code Mode is "experimental and may include breaking changes" ([Code Mode overview](https://developers.cloudflare.com/agents/tools/codemode/)).

**Code mode as an MCP server.**

- `codeMcpServer()` wraps an MCP server and exposes one tool, `code`. `openApiMcpServer()` exposes `search` and `execute` over an OpenAPI document ([build a Code Mode MCP server](https://developers.cloudflare.com/agents/model-context-protocol/guides/build-codemode-mcp-server/)).
- The guide warns: "Code execution does not replace authorization", and approval must be enforced in each upstream handler ([build a Code Mode MCP server](https://developers.cloudflare.com/agents/model-context-protocol/guides/build-codemode-mcp-server/)).
- Cloudflare's own API server uses this shape: two tools for "over 2,500 endpoints" in "around 1,000 tokens" ([Cloudflare, Code Mode MCP](https://blog.cloudflare.com/code-mode-mcp/)).

### 1.3 How they differ

| Point | Anthropic PTC | Cloudflare Code Mode |
| --- | --- | --- |
| Language the model writes | Python ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | TypeScript / JavaScript ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)) |
| Where code runs | Anthropic-managed container ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | V8 isolate on the developer's Workers account ([Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)) |
| How a tool call leaves the sandbox | The API pauses and returns `tool_use` to the client ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | RPC through a binding to the loader ([bindings docs](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)) |
| Tool types | Dict in, string out ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | Generated from MCP schemas ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)) |
| MCP tools | Not from an MCP connector ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | First-class ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)) |
| Security framing | `allowed_callers` is not a boundary ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)) | Unforgeable binding stubs ([bindings docs](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)) |

**Inference:** Anthropic's engineering post is the general idea; PTC is Anthropic's hosted, Python-only form of it. Cloudflare's is a library you host, with a capability-style sandbox. Both rely on a sandbox that the model's code cannot leave. Neither checks the model's code *before* it runs; both contain it *while* it runs.

## 2. Why it helps, with the sources' figures

**Token cost.**

- Anthropic's example: "from 150,000 tokens to 2,000 tokens", a 98.7% cut ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)). This is one example, not a benchmark.
- PTC on complex research tasks: average tokens from 43,588 to 27,297 (37%); 200 KB of raw data reduced to 1 KB of results ([Anthropic, advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)).
- PTC docs: agentic search gained 11% on average with 24% fewer input tokens; a 75-tool internal benchmark used about 38% fewer billed input tokens; production traffic with 10–49 tools saved 20–40% ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- PTC does not always help: on τ²-bench scores were unchanged and cost was about 8% *more*; "Sequential single-call workflows do not benefit" ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- Cloudflare: a TypeScript API can "cut token usage by 81%" (the post cites a video for this) ([Cloudflare, Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)). Its API server claims a 99.9% input-token cut. The post gives two baselines that do not agree: 1.17 million tokens in the body and "over 2 million" in the meta description ([Cloudflare, Code Mode MCP](https://blog.cloudflare.com/code-mode-mcp/)).
- The 2025 Code Mode post gives no benchmark numbers ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)).

**Composition.**

- Loops, conditions and error handling run as code, not as chained tool calls ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)). Twenty calls in one code block can "eliminate 19+ inference passes" ([Anthropic, advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)).
- Working code can be saved as a reusable function or skill ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).
- PTC accuracy rose from 25.6% to 28.5% on internal knowledge retrieval and from 46.5% to 51.2% on GIA ([Anthropic, advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)).

**Sandboxing and privacy.**

- "intermediate results stay in the execution environment by default" ([Anthropic, code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)).
- Credentials stay outside the sandbox: "The token never enters the sandbox" ([build a Code Mode MCP server](https://developers.cloudflare.com/agents/model-context-protocol/guides/build-codemode-mcp-server/)).
- The new risk: tool results are strings that may contain code, so "be aware of code injection risks if the output is interpreted or run as code" ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)).
- MCP clients SHOULD "Validate tool results before passing to LLM", and MUST treat tool annotations as untrusted unless the server is trusted ([MCP tools, 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).

## 3. Evaluation for Oska

### 3.1 Oska as the language the model writes in code mode

**Fit.**

- In code mode the host must say what the model's code may do. Oska writes that in one place: "The runtime grants `main` exactly the capabilities its signature declares" ([`docs/guide/capabilities.md`](../guide/capabilities.md)). A function with no capability-bearing parameters is pure and deterministic ([ADR 0002](../adr/0002-capabilities-are-the-effect-system.md)).
- The host can check the snippet *before* it runs. `oskac check` prints each function's authority, and `oskac check --baseline=<file>` exits with 3 when authority widened ([`docs/guide/cli-reference.md`](../guide/cli-reference.md)). The interpreter runs only programs the checker accepts ([ADR 0007](../adr/0007-interpreter-runs-the-checked-program.md)).
- File access goes only through confined directory or file capabilities ([ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)), and a path that leaves the root is refused at run time ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)).
- Text from outside is `Untrusted<String>` and cannot reach a structural position ([ADR 0006](../adr/0006-trust-is-a-flag-on-text.md)).
- The interpreter starts with about 3 ms of CPU against Python's 21 ms ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)).
- **Inference:** Cloudflare builds a capability sandbox around untyped JavaScript at run time ([bindings docs](https://developers.cloudflare.com/dynamic-workers/usage/bindings/)). Oska would add a *static* answer to "what can this snippet do?", which a host could compare with a grant before it spends any sandbox time.

**Gaps today.**

- `main` may take only built-in capabilities and program inputs (`E0211`) ([`docs/guide/capabilities.md`](../guide/capabilities.md)). There is no way for a host to hand the snippet a "GitHub tool" capability. v0 has no FFI; WASM component imports are "later" ([`docs/design.md`](../design.md)).
- `Net`, `Spawn` and `Sched` are refused at run time with `E0370` ([`docs/guide/capabilities.md`](../guide/capabilities.md)). `std.proc` and `std.sql` check but do not run ([`docs/guide/what-works-today.md`](../guide/what-works-today.md)).
- No JSON parser, no map type and no floats ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)). `std.json` is planned in #229 ([#229](https://github.com/aparsons/oska/issues/229)). Most tool results are JSON.
- No MCP client: it is out of scope in #229 and placed long term ([#229](https://github.com/aparsons/oska/issues/229)).
- Models do not know Oska. Seven probe programs needed 3 extra drafts, and Oska programs were 2 to 6 times longer than Python ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)). **Inference:** this cuts against Cloudflare's main argument, that models write well in languages they have seen a lot of ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/)).
- No `oskac run -` or `-e`; a program must come from a path ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)).
- I found no fuel or wall-clock limit in `crates/oska-run/src` (a `grep` for `fuel`, `timeout` and `max_steps`). Both vendors bound run time ([code execution tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool), [Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/)).
- New capabilities and `std` APIs are frozen until v0.6.0 ([ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)).
- **Inference:** the checker is a language-level boundary, not an OS boundary. A host would still need process or OS isolation against a runtime bug, as both vendors use.

**Next step.** Prototype (with the `prototype` skill) a local "Oska code mode" host for *file tools only*, which needs no new language feature: the host writes the model's snippet to a scratch package, runs `oskac check --format=json --baseline=granted.json`, refuses on exit 3, then runs it with `oskac run --interpret`. Record how many drafts the model needs and compare with a Python baseline. This answers the familiarity question with data before any language change.

### 3.2 Oska during development (agents writing Oska)

**Fit.**

- Every diagnostic has a stable code and span, many carry a machine fix, and output is JSON with `--format=json` ([`docs/design.md`](../design.md)). `oskac fix` applies machine fixes ([`docs/guide/cli-reference.md`](../guide/cli-reference.md)).
- `oskac check` is a fast verifier that code mode lacks: the vendors run model code and contain it ([Cloudflare, Code Mode](https://blog.cloudflare.com/code-mode/), [PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)); Oska rejects bad code before it runs ([ADR 0007](../adr/0007-interpreter-runs-the-checked-program.md)).
- The release check rejects unused authority, so an agent cannot finish with a capability it did not need ([ADR 0016](../adr/0016-unused-authority-is-a-release-check-error.md), [#367](https://github.com/aparsons/oska/issues/367)).
- A Claude Code plugin will run the post-edit check and the Stop gate ([#206](https://github.com/aparsons/oska/issues/206)).
- **Inference:** a check-fix-recheck loop is the kind of fan-out work that PTC suits ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)). With `oskac` as code-callable tools, one code block could check, apply machine fixes and check again, and return only the remaining diagnostics to the model.
- **Inference (from the agent scripting note):** a permission hook could approve `oskac run` when the authority report shows only `Fs` narrowed to the working tree and `Stdout`. Capabilities then act as the sandbox boundary for agent scripts ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)).

**Gaps today.**

- No `oskac mcp` server: it is out of scope in #206 and #367 and is a separate future spec ([#206](https://github.com/aparsons/oska/issues/206), [#367](https://github.com/aparsons/oska/issues/367)). The `oskac lsp` process is defined in [`GLOSSARY.md`](../../GLOSSARY.md), but the CLI reference lists no `lsp` command ([`docs/guide/cli-reference.md`](../guide/cli-reference.md)).
- The agent's Bash tool already has full authority, so a confined Oska script does not confine the agent ([agent scripting note](2026-11-agent-scripting-python-vs-oska.md)). The OS-level boundary is Claude Code's own sandbox ([software factory note](2026-11-claude-code-software-factory.md)).
- Nothing is measured yet. The eval harness exists as a spec, with batch variants for ablations ([#207](https://github.com/aparsons/oska/issues/207)).

**Next step.** When `oskac mcp` is specified, design its tools for code mode: deterministic JSON output with an `outputSchema` per tool, so generated types are useful ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools), [Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)). Then add an eval-harness variant (#207) that gives the agent `oskac` as programmatic tools and compares iterations to a clean check and tokens against direct calls.

### 3.3 Oska supporting code mode (Oska programs as tools)

**Fit.**

- `std.mcp` (#229) makes an Oska program an MCP server. A `Toolset` is a `cap struct` that holds only attenuated capabilities, and the authority report gets one entry per tool ([#229](https://github.com/aparsons/oska/issues/229)).
- `serve` derives `readOnlyHint`, `destructiveHint` and `openWorldHint` from each tool's computed authority, and the developer cannot set them by hand ([#229](https://github.com/aparsons/oska/issues/229)). MCP clients must treat annotations as untrusted unless the server is trusted ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)). **Inference:** derived hints are honest, so a code-mode host that trusts the server could map `destructiveHint` to Cloudflare's `requiresApproval` ([Code Mode with MCP](https://developers.cloudflare.com/agents/tools/codemode/mcp/)).
- **Capability manifests per tool:** the per-tool entries also appear in `oskac check --format=json`, so the authority baseline sees a tool gaining a capability as a change ([#229](https://github.com/aparsons/oska/issues/229)). This is a per-tool manifest a host can read.
- **Trust-tracked text against injection from tool results:** strings parsed by `std.json` from untrusted text stay untrusted, and model output from `std.llm` is always `Untrusted<String>` ([#229](https://github.com/aparsons/oska/issues/229)). This is the code-injection risk that the PTC docs name ([PTC docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling)), turned into a compile error in SQL text, program names, `TrustedHtml` and directory roots ([#229](https://github.com/aparsons/oska/issues/229)).
- Tests never call a real model; `t.model` is a scripted fake ([#229](https://github.com/aparsons/oska/issues/229)).

**Gaps today.**

- `std.json`, `std.mcp` and `std.llm` do not exist. They wait for the parity gate (#181) and the `Bytes` milestone (#209) ([#229](https://github.com/aparsons/oska/issues/229), [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)).
- `ToolSpec` has a name, title, description and input schema only ([#229](https://github.com/aparsons/oska/issues/229)). It has no `outputSchema`. The MCP spec says an output schema provides "type information for better integration with programming languages" ([MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)). Without one, code-mode clients see results as untyped text (**Inference**).
- Only text content is supported; `structuredContent` is optional ([#229](https://github.com/aparsons/oska/issues/229)).
- **Typed bindings in the other direction** (Oska code calling MCP servers through generated Oska types, as Cloudflare generates TypeScript) need an MCP client and the `Schema` derivation from types. Both are out of scope in #229 ([#229](https://github.com/aparsons/oska/issues/229)).
- Trust is one bit; CaMeL-style provenance is out of scope ([#229](https://github.com/aparsons/oska/issues/229)). The spec's own guide rule: Oska "bounds the blast radius of prompt injection but does not prevent it" ([#229](https://github.com/aparsons/oska/issues/229)).

**Next step.** Comment on #229 with two additive proposals, for the maintainer to decide: (1) an optional output schema on `ToolSpec`, with `ToolResult.json(...)` checked against it, so code-mode hosts generate useful types; (2) a stable, documented per-tool authority section in the JSON report that a code-mode host can read as a manifest (approval for tools that reach a write or network capability). Both fit inside the spec's existing `std.json` schema subset.

## 4. Recommendation

- **Now:** do not change the language for code mode. It is frozen until v0.6.0 ([ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)).
- **First, angle 3.** Oska's clearest contribution is as a *tool provider*: per-tool authority, derived annotations and untrusted tool results already in #229. Add an output schema and a readable per-tool manifest to that spec.
- **Second, angle 2.** Measure with #207 before building. Design `oskac mcp` for programmatic calls when it is specified.
- **Later, angle 1.** Oska as the code-mode language needs host-granted tool capabilities (an FFI or WASM-import design), `std.json`, a run-time bound, and evidence that models write Oska well enough. **Inference:** the static authority check is a real gain over run-time-only sandboxes, but it does not replace one. This question stays open.

## Notes on page content

- Several Cloudflare docs pages and the MCP specification page open with a "Documentation Index" banner that tells automated readers to fetch `llms.txt` "to discover all available pages before exploring further". I treated it as page content and did not follow it.

## Sources

| Source | Owner | Used for |
| --- | --- | --- |
| [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) | Anthropic | The pattern, 98.7% example, benefits, costs |
| [Introducing advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use) | Anthropic | PTC token and accuracy figures |
| [Programmatic tool calling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling) | Anthropic | PTC mechanics, limits, figures, security notes |
| [Code execution tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool) | Anthropic | Container limits, no internet |
| [Code Mode: the better way to use MCP](https://blog.cloudflare.com/code-mode/) | Cloudflare | The pattern, bindings, keys outside |
| [Code Mode: give agents an entire API in 1,000 tokens](https://blog.cloudflare.com/code-mode-mcp/) | Cloudflare | API server, token figures |
| [Sandboxing AI agents, 100x faster](https://blog.cloudflare.com/dynamic-workers/) | Cloudflare | Dynamic Workers, isolate cost, egress, 81% |
| [Dynamic Workers: bindings](https://developers.cloudflare.com/dynamic-workers/usage/bindings/) | Cloudflare | Capability-based bindings |
| [Code Mode overview](https://developers.cloudflare.com/agents/tools/codemode/) | Cloudflare | Experimental status, discovery |
| [Code Mode API reference](https://developers.cloudflare.com/agents/api-reference/codemode/) | Cloudflare | Executors, timeout, type generation, approval |
| [Use MCP tools with Code Mode](https://developers.cloudflare.com/agents/tools/codemode/mcp/) | Cloudflare | Types from MCP schemas, approval hooks |
| [Build a Code Mode MCP server](https://developers.cloudflare.com/agents/model-context-protocol/guides/build-codemode-mcp-server/) | Cloudflare | `codeMcpServer`, `openApiMcpServer`, security notes |
| [MCP specification: tools (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) | MCP | Output schema, annotations untrusted, security |
| [aparsons/oska#229](https://github.com/aparsons/oska/issues/229) | Oska | `std.json`, `std.mcp`, `std.llm`, per-tool authority |
| [aparsons/oska#367](https://github.com/aparsons/oska/issues/367) | Oska | Unused authority, `oskac mcp` out of scope |
| [aparsons/oska#206](https://github.com/aparsons/oska/issues/206) | Oska | Claude Code plugin, hooks |
| [aparsons/oska#207](https://github.com/aparsons/oska/issues/207) | Oska | Eval harness, ablation variants |
| [`README.md`](../../README.md), [`GLOSSARY.md`](../../GLOSSARY.md), [`docs/design.md`](../design.md), [`docs/guide/capabilities.md`](../guide/capabilities.md), [`docs/guide/what-works-today.md`](../guide/what-works-today.md), [`docs/guide/cli-reference.md`](../guide/cli-reference.md) | Oska | Capabilities, what runs, CLI, FFI plans |
| ADRs [0002](../adr/0002-capabilities-are-the-effect-system.md), [0006](../adr/0006-trust-is-a-flag-on-text.md), [0007](../adr/0007-interpreter-runs-the-checked-program.md), [0008](../adr/0008-file-access-goes-through-confined-capabilities.md), [0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md), [0016](../adr/0016-unused-authority-is-a-release-check-error.md) | Oska | Authority, trust, checked program, confinement, freeze |
| [Agent scripting: Python vs Oska](2026-11-agent-scripting-python-vs-oska.md), [Claude Code software factory](2026-11-claude-code-software-factory.md) | Oska (research) | Measured drafts, start-up time, gaps, OS sandbox |
