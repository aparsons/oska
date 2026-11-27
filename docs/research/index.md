# Research

Each note answers one question, with its sources and the date it was written. A later decision can overrule a note; what the project decided is in the [decision records](../adr/). The notes are grouped by topic. Within a topic, read them from the top.

## The language and what it is for

- [Designing a secure-by-default, AI-first language](2026-11-ai-first-secure-language.md) (2026-11-08): the research the language started from. Four foundations: memory safety, capabilities with no ambient authority, effects in every signature, and machine-checked contracts with designs that make whole classes of vulnerability impossible to write.
- [AI inside Oska programs, and around the toolchain](2026-11-ai-in-oska.md) (2026-11-12): how capabilities and untrusted text answer the OWASP risks of LLM applications, such as prompt injection, by design rather than by prompting.
- [How long until Oska can replace Rust?](2026-11-oska-replacing-rust.md) (2026-11-17): what is missing before Oska can replace Rust for command-line tools and services, and for this repository's own code.
- [An operating system built with Oska](2026-11-oska-operating-system.md) (2026-11-19): what an operating system, or an environment like one, for Oska and AI agents could look like, and a staged roadmap. Mostly proposals.
- [A secure-by-default `std.json`](2026-11-json-parser.md) (2026-11-19): how `std.json` should parse, with the lessons of nine JSON parsers. Strict by default, with every limit on.
- [Math and data science for Oska](2026-11-math-and-data-science.md) (2026-11-22): what the main math and data libraries of ten languages provide, and what belongs in `std`. Build a `Float` with pinned IEEE 754 results first, then a deterministic map, `std.math`, `std.stats`, a seeded `Rng` and `std.csv`. Not Parquet, BLAS, tensors or plotting yet.
- [Data classification for Oska](2026-11-data-classification.md) (2026-11-26): yes. A classification flag on text and bytes like the trust flag, a ceiling label on log, network and `std.llm` sinks, and a `Declassify` capability. Open: implicit flows and declassification policy.
- [Knowledge graphs and ontologies in Oska](2026-11-knowledge-graphs-and-ontology.md) (2026-11-27): which W3C and ISO standards own the facts, and how capabilities, untrusted text and `std.llm` fit building, running and maintaining a graph. Build a strict RDF codec, diff, SHACL-core and canonical hashing first. Not an OWL reasoner or a SPARQL engine. Open: GQL and SPARQL Update were not read.

## Agents and code mode

- [Tooling for AI-driven development](2026-11-ai-driven-development.md) (2026-11-12): what Oska's agent loop already has (stable codes, exact spans, fixes an agent can apply) and what would improve it.
- [Should agents write throwaway scripts in Oska instead of Python?](2026-11-agent-scripting-python-vs-oska.md) (2026-11-15): not yet. Speed is not the reason: 7 of the 29 uses need JSON or SQLite, which Oska cannot do yet, and the rest come out 2 to 6 times longer in Oska.
- [What "code mode" is, and how Oska fits it](2026-11-code-mode.md) (2026-11-19): code mode means the model writes a program that calls its tools, run in a sandbox. Anthropic's and Cloudflare's forms of it, and where Oska could serve.
- [The changes Oska needs for code mode](2026-11-code-mode-changes.md) (2026-11-19): the follow-up to the note above. The changes, ranked by value for their cost.

## Running natively on every platform

- [Calling macOS through libSystem](2026-11-macos-libsystem-calls.md) (2026-11-17): how `std.rt` calls macOS, which has no stable system-call numbers. Accepted before the macOS backend was built; see [ADR 0015](../adr/0015-macos-intrinsics-are-libsystem-calls.md).
- [Calling Windows through kernel32 and ntdll](2026-11-windows-kernel32-calls.md) (2026-11-19): the same question for Windows, which has no file descriptors, signals or `errno`. Accepted before the Windows backend was built; see [ADR 0020](../adr/0020-windows-intrinsics-are-kernel32-and-ntdll-calls.md).
- [An `oskac` that does not need glibc](2026-11-oskac-without-glibc.md) (2026-11-19): how the Linux `oskac` can run on any Linux, a `FROM scratch` container included, by building it for musl. See [ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md).
- [Why the musl build of `oskac` is slower](2026-11-musl-oskac-performance.md) (2026-11-20): the time goes to musl's C library (its allocator, returning memory to the kernel, and `memcpy`), but the cheapest fix is in Oska: allocate and copy less. Measured on x86_64 only; aarch64 is pending.
- [How to replace `memcpy` in the musl build, and what else would speed up native performance](2026-11-memcpy-replacement.md) (2026-11-22): replace only `memcpy` and `memmove`, on x86_64 musl, in `oska-sys`: measured to close about a third of the interpreter gap and over half of the compile gap, before PR #570 (after it, less). `lto = "fat"` is the cheaper, larger win (10 to 22% on the glibc interpreter, less on musl). aarch64 is not measured.

## Installing, releases and the site

- [Installing, updating and uninstalling Oska](2026-11-install-update-uninstall.md) (2026-11-12): what the installers did at v0.3.0, the gaps (nothing announced a new release, and the unsigned `latest.json` chose the release), and what would close them.
- [A review of oskalang.dev](2026-11-site-review.md) (2026-11-12): the site as a person and an agent see it, with ranked fixes. Specs [#183](https://github.com/aparsons/oska/issues/183) and [#288](https://github.com/aparsons/oska/issues/288) came from it.

## Security and compliance

- [FIPS 140-3 and a FIPS mode for Oska](2026-11-fips-140-3.md) (2026-11-21): FIPS 140-3 validates a cryptographic module, not a language, and Oska has no certificate. It covers what Oska can honestly claim in a self-assessment and three ways to offer a FIPS mode. Spec [#569](https://github.com/aparsons/oska/issues/569) came from it.
- [Hardening `oskac` and the programs it compiles against reverse engineering](2026-11-binary-hardening.md) (2026-11-23): obfuscating `oskac` itself buys little since it is open source; the real answer is the signed-release pipeline already in place. Stripping, PIE, stack protector, RELRO and CFI are cheap, real hardening for both `oskac` and its compiled output. Packers, anti-debugging and whole-binary encryption are security theater that would conflict with code signing and reproducible builds.
- [Built-in Let's Encrypt and ACME support for Oska](2026-11-lets-encrypt-acme.md) (2026-11-27): ship `std.acme` as a library first, not a flag on a server. Oska has no signing code (ECDSA, Ed25519 and RSA only verify), no TLS server and no non-loopback listener, so the blocker is a server-side stack, not the protocol. HTTP-01 first, then DNS-01. Open: RFC 8555 section 8 was not readable.

## How the project itself is built

These notes are about this repository's development process, not about the language.

- [An AI-driven software factory on Claude Code](2026-11-claude-code-software-factory.md) (2026-11-12): the project's spec, ticket, review and merge flow, and what would make it a complete factory.
- [Keeping Claude and the development process fast](2026-11-development-throughput.md) (2026-11-15): why checks were slow on the development machine (several heavy runs sharing four cores, and repeated checks), and what to change.
- [Could Oska speed up the project's own tooling?](2026-11-oska-for-development-tooling.md) (2026-11-15): not yet, and speed is the smallest reason.
- [Oska versions of grep, sed, awk and jq?](2026-11-oska-text-tools.md) (2026-11-15): no. Those tools take 2% of the agents' command time, and an Oska version would be slower.
- [Bottlenecks in development, testing and release](2026-11-bottlenecks-dev-test-release.md) (2026-11-20): the biggest wait is for a human merge or a release tag, not the machines. The changes, ranked by impact.
- [Could Oska tests detect and report coverage?](2026-11-test-coverage.md) (2026-11-26): yes. The Rust code of `oskac` is cheap to cover with `cargo-llvm-cov` in a CI job. Oska programs need counters added to the interpreter and the native backends, with line coverage as the first report.
- [AI-first, secure-by-default data stores for Oska](2026-11-ai-first-data-stores.md) (2026-11-27): build an embedded, single-writer, typed store (hash-chained append-only log plus snapshot) that carries trust and classification labels, is encrypted at rest from a capability key, and is exposed through `std.mcp`. Not a SQL server first. Open: the prelude has no append, fsync, rename or lock, and `std.sql` has no runtime.
