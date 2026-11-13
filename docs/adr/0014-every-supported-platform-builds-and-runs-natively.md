# Every supported platform builds and runs natively, with one behaviour

## Decision

Oska keeps **feature parity** across its **supported platforms**: Linux on aarch64 and x86_64, macOS on aarch64 and x86_64, and Windows on x86_64, the platforms `oskac` is released for.

- **Catch up, then gate.** New language features are frozen until every supported platform has a native backend: no new syntax, types, prelude or `std` APIs, or capabilities. Bug fixes, docs, the site, CI and tooling that does not change what a program does carry on. Once all five are native, the **parity gate** applies: a feature is done only when it works natively, with the same output and the same security guarantees, on every supported platform.
- **The order**, one new piece at a time: x86_64 Linux (a new instruction set), then macOS on aarch64 (Mach-O, dyld and libSystem, ad-hoc signing), then macOS on x86_64 (both pieces exist), then Windows on x86_64 (PE, kernel32 and ntdll imports). A minor release marks each: v0.4.0, v0.5.0 for both macOS platforms, v0.6.0, which also turns the gate on.
- **Any host builds for any platform.** `oskac build --target <platform>` (or `--target all`) works on every host, and defaults to the host. The same program built by the same `oskac` gives the same bytes on every host.
- **Release.** `oskac` builds an application's executables for every platform and writes a `SHA256SUMS` signed with the developer's SSH key, the scheme `oskac`'s own releases use. Apple notarization and Authenticode signing stay the developer's own step, documented, because they need paid accounts and the vendors' services.
- **One behaviour where operating systems differ.** Oska defines the result: `IoError` messages come from one Oska table instead of the operating system's text, paths print with `/`, output is never newline-translated, and exit codes are Oska's. A program's output is byte-identical on every platform.
- **The same confinement everywhere.** On Windows, each path component is opened beneath its parent directory's handle, and reparse points (symlinks, junctions) and hard-linked files are refused, as on Linux and macOS ([ADR 0008](0008-file-access-goes-through-confined-capabilities.md)). This applies to the interpreter (`oska-sys`) and to `std.rt`.
- **The interpreter is the reference, not a fallback.** It stays as the differential suite's oracle and behind `--interpret`. At the gate, a program the native backend cannot compile is an error on every platform; it no longer runs on the interpreter after a note.
- **Verification.** The differential suite runs natively on each platform's CI runner. Those checks are not required to merge: the gate is a project rule, held by review and the pre-push hook.

## Why

A language whose security model is the same everywhere cannot have a platform where confinement races, or where a feature silently runs a different engine. Agents write code once and expect it to behave the same wherever it is built and run; byte-identical output and builds make that checkable by one test on every platform. Doing the platforms before more language work keeps the cost of the gate small: every later feature is written for five targets from the start, not ported afterwards.

## Considered options

- **A parity gate from today.** Rejected: four platforms have no backend, so every feature would stall until they do. Catching up first gives the same end state with a clear sequence.
- **Parity of behaviour only, with the interpreter where native is missing.** Rejected: native code becomes a speed feature some platforms never get, and the interpreter fallback can hide a gap on one platform.
- **Build only for the host.** Rejected: releasing an application would need five machines. Oska writes its own executables with no outside linker ([ADR 0011](0011-oska-generates-native-code-itself.md)), so cross-building costs little.
- **Signing with Apple Developer ID and Authenticode inside `oskac`.** Rejected for now: it needs certificate handling and crypto in Oska ([ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md)), and notarization is a vendor service.
- **Keep the operating system's error text, documented per platform.** Rejected: output would differ by platform, so tests could not compare messages.
- **Keep Windows confinement path-based, as a documented exception.** Rejected: it is the one place the security guarantee would differ.

## Consequences

- This changes parts of ADR 0011: the interpreter no longer runs what the native backend refuses (`E0390` becomes an error at the gate), and `oskac build` targets every supported platform, not only aarch64 Linux.
- The `Bytes` milestone (`Bytes`, bitwise and wrapping integer operations and secret integers, for ADR 0012) waits for v0.6.0.
- The native backend gets an x86_64 code generator, and Mach-O and PE writers beside the ELF one. `std.rt`'s system-call intrinsics need a lowering per operating system: Linux system calls by number, and imports from libSystem on macOS and from kernel32 and ntdll on Windows.
- Development on the aarch64 Linux machine checks other platforms' output through CI, so their feedback loop is slower.
- The gate is not enforced by GitHub: a change that breaks one platform can be merged by overriding review.
