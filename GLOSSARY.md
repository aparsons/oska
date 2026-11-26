# Oska

Oska is a secure-by-default programming language designed for AI agents to write and humans to review. These terms describe its security model.

## Authority

**Capability**:
An unforgeable value that grants authority to perform one kind of effect, such as reading files, opening network connections, reading the clock, or generating random numbers. Holding a capability is the only way to perform its effect.
_Avoid_: permission, effect handler, token

**Ambient authority**:
Authority available to code without being explicitly handed to it, such as global state or unrestricted system calls. Oska has none.

**Root of authority**:
The program's entry point, `main`. The runtime grants `main` exactly the capabilities its signature declares, and every other capability in the program comes from those.
_Avoid_: root token, `Root`

**Attenuation**:
Producing a narrower capability from one you already hold, for example a directory capability from `Fs`. Attenuation never widens authority.
_Avoid_: derive, narrow, restrict (as nouns for the concept)

**Directory capability**:
A capability confined to one directory and everything beneath it, such as `DirRead`. All access to a directory goes through one.
_Avoid_: restricted Fs, sandboxed filesystem

**File capability**:
A capability confined to a single file, such as `FileRead`. Using it takes no path.

**Host capability**:
A capability confined to one host and one port, `HostConnect`. It alone can connect, and `connect()` takes no destination. `Net` and `NetConnect` only attenuate to one, with `restrict_to(host, port)` (ADR 0025). A loopback listener, `LoopbackListen`, is the listening counterpart, for a local test server.
_Avoid_: restricted NetConnect, connect-to-any

**Stub resolver**:
`std.dns`, the name resolver written in Oska that `connect()` uses on both backends. It is a public module, and its two prelude methods, `HostConnect.exchange_datagram` and `NetConnect.system_name_servers`, are public (ADR 0026). The trust configuration for TLS is `x509.TrustConfig`, and a test's `t.net_attempts()` lists where code tried to connect.
_Avoid_: system resolver, `RootStore`, `net_requests`

**Budget**:
The capability to spend model tokens and calls, which a model client such as `std.llm` takes. Its counters live in the runtime, so every copy of a budget draws from the same ones. `main`'s is unlimited, and `limit` attenuates it to a child budget that draws from its parent and never has more than the parent has left. It counts tokens, not money.
_Avoid_: quota, allowance (for the capability), wallet

**Reservation**:
The tokens and one call that a budget holds for a request before it is sent. It is settled once: a debit spends what the reply used and returns the rest, and a release returns everything. A reservation the budget, or any budget it was limited from, cannot cover is refused with `OverBudget`, before anything is spent.

**Spend**:
To use up tokens and calls of a budget: what a reservation's debit does. What a budget has spent includes what every budget limited from it has spent.
_Avoid_: charge, consume (for budgets)

**Capability-bearing type**:
A type that is a capability or that contains one, directly or transitively. This includes containment through type arguments, as in `List<Stdout>`, and closures that capture a capability.

**Authority**:
For a function, the set of capabilities it can reach through its capability-bearing parameters. For a package, the union of the authority of its public functions, counted in built-in capability kinds. Which directory or file a capability is confined to is the caller's choice and is not part of a package's authority.

**Authority safety**:
The property that running a function performs only effects of capability kinds in its authority, or of kinds that the closures passed to its `cap fn` parameters captured. It is one of the three theorems stated for Oska Core, with progress and preservation (ADR 0018).
_Avoid_: capability safety, effect soundness

**Widening**:
A change after which a function's, a program's or a dependency's authority includes a capability kind it did not have, or a broader kind in place of a narrower one. A widening must be approved before the build or the authority gate accepts it.
_Avoid_: escalation, upgrade (for this concept)

**Unused authority**:
A capability-bearing parameter, or a field of a `cap struct`, that a function never uses, either directly or by passing it on. The function declares more authority than it exercises, so its authority report overstates what it does. The release check rejects it.
_Avoid_: dead capability, over-privilege (for this concept)

**Authority baseline**:
A saved record of a program's authority report that later reports are compared with. It is committed with the program, and updating it is how a widening is approved.
_Avoid_: authority snapshot, authority lockfile

**Authority grant**:
The most authority an embedder allows one piece of code to have for one run. Unlike an authority baseline, it is set by the embedder, is not committed with the code, and cannot be updated by the code's author: code whose authority goes past it is refused, never approved.
_Avoid_: grant file, sandbox policy, allowlist (for this concept)

**Pure function**:
A function with no capability-bearing parameters and no parameter whose type is or holds a bounded type parameter, whose type argument could be capability-bearing. It cannot perform effects, and it is deterministic.

**Tool**:
One operation that an agent, such as an MCP client or a model, may call by name, with JSON arguments that match its input schema. A tool reaches only what its toolset holds.
_Avoid_: function, command, action (for this concept)

**Toolset**:
A value that describes a set of tools and runs them: a `cap struct` holding only the narrowed capabilities its tools need, implementing `std.mcp`'s `Toolset` trait.
_Avoid_: server, tool registry (for the value itself)

**MCP server**:
An Oska program that calls `std.mcp`'s `serve` with a toolset: it speaks MCP over standard input and output, one JSON-RPC message per line, and returns when standard input closes. It serves two protocol eras, the modern one (2026-07-28, each request carrying its own `_meta`) and the legacy one (2025-11-25, which starts with `initialize`).
_Avoid_: agent (the client is the agent), plugin

**Derived annotation**:
A tool's `readOnlyHint`, `destructiveHint` or `openWorldHint`, which `serve` computes from the capabilities the checker found the tool reaches. A program has no way to set one by hand, so a hint is never more permissive than the tool's authority.
_Avoid_: hint (alone), claimed annotation (a derived one is never claimed)

**Protocol era**:
One of the two versions of the MCP wire protocol that `serve` speaks. The modern era (2026-07-28) is stateless: every request carries its protocol version and client capabilities in `_meta`. The legacy era (2025-11-25) starts with `initialize` and has `ping`. `initialize` chooses the legacy era for the rest of the session.
_Avoid_: mode, version (alone)

**Session**:
What `std.mcp`'s `handle` remembers between messages: the server's info, the tools as listed when the session began, and whether the client chose the legacy era. It is a value that `handle` takes and returns, not a connection.
_Avoid_: connection, client state

**Per-tool authority**:
The authority report's entry for one tool of a toolset: the capabilities the code that runs that tool can reach, computed from its arm of the `match` on the tool's name, not the union of the whole toolset.
_Avoid_: tool permissions, tool annotations (for this concept)

**Pre-state value**:
A value in a postcondition captured with `old(...)` when the function is entered, before a mutating method changes its receiver.
_Avoid_: snapshot, previous value

**Program input**:
Data given to `main` that grants no authority, such as `Args`.
_Avoid_: calling it a capability

**Mutating method**:
A method that declares `var self`. It can change its receiver in place, and can only be called on a `var` binding.
_Avoid_: inout, mut ref

**Prelude**:
The built-in declarations, written in Oska, that every program can use without importing them, such as `Int`, `Option`, `Result`, and the built-in capabilities.
_Avoid_: stdlib (for this subset), builtins

**Checked program**:
A program the checker has accepted, with every name, method call, and conversion already resolved, and every expression and slot typed. Both the interpreter and the native backend run only checked programs.
_Avoid_: IR, compiled program

**Panic**:
A runtime failure that stops the current task and cannot be caught, such as integer overflow or an out-of-bounds index. A panic means the program has a bug; an `Err` returned from `main` does not.
_Avoid_: exception, crash

## Untrusted data

**Untrusted value**:
Text or bytes that came from outside the program, through a capability or a program input. Text and byte operations keep a value untrusted. Converting it to a validated type, or reading a number or a `Bool` out of it, gives a trusted value.
_Avoid_: tainted, dirty, unsafe input

**Bytes**:
The prelude's buffer of raw bytes, for binary data that is not text. It is a value, like every Oska value, and it carries the same trust flag as text.
_Avoid_: byte array, blob, binary string

**Map**:
The prelude's `Map<K, V>`: a value that holds entries and finds one by its key. Keys need `Eq` and `Hash`; `Float` and secret values are refused as keys (ADR 0028). Like a list, it is copied on write.
_Avoid_: dictionary, hash table, hash map

**Entry**:
One key and its value in a map. `entries()` gives them in insertion order.
_Avoid_: pair, item, member (a `std.json` member is a different type)

**Insertion order**:
The order in which a map's keys were first added. Iteration always follows it, on every platform and both backends. Updating a key keeps its place; removing it and adding it again moves it to the end.
_Avoid_: hash order, sorted order

**Validator**:
A function that inspects an untrusted value and, only if the value passes its checks, returns a trusted value of a validated type, such as an `Email`.
_Avoid_: sanitizer

**Trust assertion**:
An explicit, audited point where code declares untrusted text or bytes trusted, normally inside a validator.
_Avoid_: untaint, sanitize, cast

**Audit site**:
A place in the source where code deliberately weakens a guarantee, such as a trust assertion or a secret exposure. Tooling lists every audit site for review.
_Avoid_: escape hatch (informal)

**Secret value**:
A value whose contents are never displayed, logged, interpolated, or compared with `==`. Making a value secret is not an audit site; the only way to read it is an explicit, auditable exposure.
_Avoid_: sensitive, private

**Secret integer**:
A secret value of a fixed-width unsigned integer type: `Secret<U8>`, `Secret<U32>` or `Secret<U64>`. Code may compute with it in constant time, but never branch on it, index with it, divide by it, shift by it, or order it.
_Avoid_: constant-time int, CtU32

**Constant time**:
The property of code that no conditional branch or memory address depends on a secret value, and no secret is a divisor or a shift amount, so the path the code takes and the memory it touches do not reveal the secret. It holds for native code, not for the interpreter, and not for what the hardware does with the instructions.
_Avoid_: timing-safe, side-channel free

**Approval**:
A person's yes to one consequential action, such as a tool deleting a file, recorded with the action's summary and details. Only an approver makes one, and it is single use: the tool it is given to consumes it, and the runtime refuses a second use. It is not the approval of a widening, which is a change to an authority baseline that a reviewer makes before a build.
_Avoid_: consent, confirmation token

**Approver**:
Who an approval comes from: the person at a terminal, asked over standard error with the answer read from standard input, or a test's scripted stand-in. An approver over standard input is only as strong as whoever controls that stream.

**Approval summary**:
The trusted text that says what an approval is for, shown above its untrusted details. Both are shown with control and bidirectional characters made visible, so neither can disguise the action.

**Sink**:
An API where data leaves the program or affects how it behaves, such as SQL queries, process execution, HTML output, filesystem paths, network hosts, or logs.

**Sink literal**:
A string literal tagged with the name of a sink, such as `sql"..."`. It provides the text for a structural position and can never interpolate values.
_Avoid_: tagged string, raw query

**Data position**:
An argument to a sink that is treated only as data. It accepts untrusted values. Examples are SQL parameters, escaped HTML text, and process arguments.

**Structural position**:
An argument to a sink that decides code, commands, paths, or destinations. It rejects untrusted values. Examples are SQL query text, a program name, `TrustedHtml`, an attenuation directory, and a network host.

## Packages

**Module**:
One source file. Its items, methods and struct fields are private to it unless marked: an item or method with `pub(pkg)` or `pub`, a field with `pub`. The methods of an `impl` belong to the module the `impl` is in.

**Package**:
A set of modules published together from one git repository and named by its URL. Its `pub` items are its API, and its authority is counted over that API alone.
_Avoid_: crate, library (for this concept)

**Manifest**:
The file in which a package states its identity, its direct dependencies, and the authority approved for each one.
_Avoid_: package file, config

**Lockfile**:
The record of the exact version, commit, content hash and computed authority of every package in the dependency graph.

**Direct dependency**:
A package named in the manifest. Only direct dependencies need authority approval.

## Execution

**Interpreter**:
The toolchain's way of running a checked program directly, with no executable. It is the reference for what every program does, except that a program in Oska Core is judged by Oska Core's semantics (ADR 0018).
_Avoid_: VM, evaluator

**Native backend**:
The part of the toolchain that compiles a checked program into a standalone executable for a platform, with no outside assembler or linker. On Linux the executable uses no C library; on macOS it calls the system only through `libSystem`, the one library macOS supports calling the kernel through (ADR 0015); on Windows it calls only `kernel32`, `ntdll`, `ProcessPrng` and `GetAdaptersAddresses` of `iphlpapi` (ADR 0020, amended by ADR 0026). A program it cannot compile yet is refused, never compiled wrongly.
_Avoid_: codegen, JIT

**Supported platform**:
An operating system and processor that `oskac` is released for: Linux on aarch64 and x86_64, macOS on aarch64 and x86_64, and Windows on x86_64. Every feature works the same on each, natively, with the same security guarantees.
_Avoid_: OS

**glibc build**:
The Linux `oskac` that links glibc dynamically, so it runs only where glibc 2.35 or newer is installed. It is one of two builds of `oskac` for the Linux supported platform, and the faster one, so the installer picks it wherever it can run (ADR 0022).
_Avoid_: dynamic build, the Linux binary

**musl build**:
The Linux `oskac` that is a static executable with musl inside it, with no dynamic loader and no shared library, so it runs on any Linux, an empty container included. It is a variant of the Linux supported platform, not a platform of its own: it takes the same target platforms and builds the same bytes as the glibc build. It runs at about the same speed on aarch64, and up to about 1.2x slower on x86_64 for the site build and more for text-heavy work (ADR 0022; `docs/research/2026-11-musl-oskac-performance.md`).
_Avoid_: static build, Alpine build, musl platform, musl target (for `oskac`'s own build)

**Parity gate**:
The rule that a feature is done only when it works natively, with the same output and the same security guarantees, on every supported platform. Where operating systems differ, Oska defines the result: its own error wording, `/` in paths, no newline translation and its own exit codes.

**Host**:
The supported platform `oskac` itself is running on. Unlike an embedder, it is a platform, not a program.

**Embedder**:
A program outside Oska that runs code a model wrote, in a sandbox it controls, and supplies the tools that code may call, as code-mode agents do. Oska meets an embedder as a tool provider, as the language of the model's code, or as a checker of that code.
_Avoid_: host, code-mode host, MCP host (for this concept), agent harness

**Target platform**:
The supported platform an executable is built for. Any host can build for any target platform, and the same program built by the same `oskac` gives the same bytes on every host.
_Avoid_: triple, arch

**Runtime**:
What runs alongside a program to provide what the language promises it: memory, text, panics, the capabilities `main` is granted, filesystem confinement and the test host. For native executables it is `std.rt`, an Oska module owned by the toolchain and compiled into every executable, which no program can import; the interpreter provides the same itself.
_Avoid_: libc, runtime library

**System-call intrinsic**:
A body-less function in the runtime that performs one operating-system call. It is the only way native code reaches the operating system, and naming one anywhere outside the runtime is an error.
_Avoid_: syscall wrapper

**Differential suite**:
The tests that run every program of a corpus on both the interpreter and the native backend and fail on any difference in output, exit code, panic or files written.
_Avoid_: parity tests, golden tests (for this suite)

**Float**:
The number type for fractions, IEEE 754 binary64. Every result of every operation on it is a pinned result (ADR 0027). There is no implicit conversion to or from `Int`.
_Avoid_: double, real, decimal

**Canonical NaN**:
The one NaN that every `Float` operation makes. `to_bits` returns its bit pattern, so no program can see which operation produced it.
_Avoid_: NaN payload, quiet NaN (for this concept)

**Pinned result**:
A result that the spec fixes down to the bits, so it is the same on the interpreter and on every supported platform. All `Float` operations and all `std.math` functions have one; each `std.math` function is pinned in the fast tier and, where it has one, in the exact tier (ADR 0027).
_Avoid_: deterministic result, approximate result

**Obfuscation pass**:
A native-backend pass, chosen with `oskac build --obfuscate`, that reshapes a compiled program's machine code to raise the cost of static analysis: encrypted string and constant literals decrypted on first use, flattened control flow, opaque predicates, junk instructions and reordered basic blocks. It never changes a program's output, exit code or panics; the differential suite runs with it on.
_Avoid_: packer, protector (for this concept; "stack protector" is a different, always-on mitigation)

**Oska Core**:
The formal calculus that defines the typing and running of a subset of Oska. It has no generics or traits and grows one construct at a time; the core IR export refuses any program outside it. Its semantics is executable, and it is the reference that the interpreter is judged by for programs in that subset (ADR 0018).
_Avoid_: core language, kernel, mini-Oska

**Core IR**:
The versioned form of a checked program in Oska Core that Oska Core's executable semantics reads. Unlike the checked program, which is internal to the toolchain, its shape is stable, and a change to it is a new version.
_Avoid_: checked program (for this export), core AST

## Installation

**Managed install**:
An `oskac` that the installer put in place and recorded in its install receipt. Only a managed install updates or uninstalls itself; any other copy is left alone.
_Avoid_: system install, global install

**Update checker**:
The toolchain's own Oska program that fetches the signed release index and verifies releases for `oskac self check` and `oskac self update`. It is compiled when `oskac` is built and carried inside it, and it runs as a separate process holding only the network and file authority it needs.
_Avoid_: updater daemon, auto-updater

## Diagnostics

**Release check**:
`oskac check --release`: the stricter check a program must pass before it ships. Besides every ordinary error, it rejects code that is fine while working but must not ship, such as `dbg`, `todo()` and unused authority.
_Avoid_: lint, warning (Oska has neither)

**Fix applicability**:
How safe a diagnostic's fix is to apply without a person reading it: `machine` when it is certainly right, `maybe` when it is a guess at what was meant, such as the closest name to an unknown one, and `placeholders` when the program checks afterwards but code is left to write, such as `todo()` arms. It is set by the code and the situation the fix is offered in, and the code catalog records the rule.
_Avoid_: confidence, safety level

## Tooling

**Language server**:
The long-running `oskac lsp` process that editors and agents ask about Oska code over the editor protocol: errors, where a name is defined and used, and a value's type and authority.
_Avoid_: LSP (the protocol, not the server), Claude LSP server (the server is the same for every editor; Claude Code reaches it through the plugin)

**Snapshot**:
The stored rendering of a value that a test's `t.assert_snapshot(value)` compares the value with: one file per assertion, `snapshots/<test-file>/<test-name>.snap`, committed with the tests. The test runner, never the test, reads and writes it, and `oskac test --update` writes the new and changed ones. Only this sense: a value captured with `old(...)` is a pre-state value, and a saved authority report is an authority baseline.
_Avoid_: golden file, fixture (for this concept)

**Snapshot update**:
`oskac test --update`: writing each snapshot that is missing or differs from its value instead of failing the test, and listing what was written. It never deletes a snapshot, and its result is reviewed as a diff like any other change.
_Avoid_: accepting, blessing

**Coverage**:
The share of a program's lines that a test run executed, counted per function, with the lines that never ran listed. It is line coverage only: whether each side of a branch ran is a different measure and has no term yet. It describes one run, not how good the tests are.
_Avoid_: test coverage, code coverage (they suggest it grades the tests)

## Compliance

**Self-assessment**:
A report in which the project judges itself against one outside standard, such as SLSA or ISO/IEC 5230, and cites a pinned commit or release for each claim. No third party performed or certified it.
_Avoid_: audit, certification, attestation (for this report)

**Finding**:
A gap in a self-assessment that is rated High or Medium by likelihood times impact. Only findings lower a self-assessment's determination.
_Avoid_: issue, defect, non-conformity (when the gap is rated Low)

**Observation**:
A gap rated Low, or advice where nothing is unmet, in a self-assessment. Unlike a finding, an observation does not lower the determination, even when it records an unmet requirement.
_Avoid_: minor finding, note, suggestion

**Determination**:
A self-assessment's verdict: Conforms, Partially conforms or Does not conform, given beside the standard's own measure, such as an SLSA Build level or a Scorecard score. Only findings can move it below Conforms.
_Avoid_: rating, score, grade (for the headline verdict)
