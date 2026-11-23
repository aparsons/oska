# `oskac build --harden` adds a stack canary and CFI landing pads

- **Status:** Accepted (2026-11-23). The user settled each decision below in a grilling session on 2026-11-23. The research behind it is [docs/research/2026-11-binary-hardening.md](../research/2026-11-binary-hardening.md).

## Decision

`oskac build --harden` makes the native backend ([ADR 0011](0011-oska-generates-native-code-itself.md)) emit two standard exploit mitigations into a compiled program:

- **A stack canary**, using the "strong" heuristic: a function gets a canary check when it has a local array of any size, or takes the address of a local variable. A function with neither gets none. On a canary mismatch, the program aborts through the same crash path a panic already uses.
- **CFI landing pads** at the start of every function and indirect-branch target: `BTI` instructions on aarch64, `ENDBR64` on x86_64. These are no-ops on a CPU that does not enforce them, so they cost nothing there and gain the mitigation on one that does.

The flag is opt-in. A program built without `--harden` is unaffected. `oskac`'s own binary is unaffected by this decision either way: it keeps its symbols, and the project's existing signed-release pipeline, not binary hardening, is its defense against a tampered build.

Because [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)'s parity gate is on, `--harden` ships on Linux, macOS and Windows (aarch64 and x86_64) in one change, not platform by platform. The differential suite ([ADR 0011](0011-oska-generates-native-code-itself.md)) must keep passing unchanged with `--harden` on: these mitigations reshape a program's machine code, never its output, exit code or panics.

## Why

The native backend already writes every instruction and every section of a compiled executable, so these mitigations are backend policy, not a hope that an external linker applied them. They are inexpensive, well-documented, and standard (the Rust reference, LLVM, and the Arm and Intel architecture manuals all describe them), unlike packing or anti-debugging, which the research note found to be security theater that would conflict with code signing and reproducible builds. Making them a flag, rather than always on, keeps every existing build byte-for-byte what it was before this decision, and leaves turning them on a program's own author's choice.

## Considered options

- **On by default, with no flag.** Rejected: the user chose to make this opt-in, so that adopting it is a deliberate choice for a given build, not a change to every existing build's bytes.
- **Every function gets a canary.** Rejected: higher overhead everywhere, for no extra protection on a function with no local buffer or address-taken local.
- **Obfuscating `oskac`'s own binary.** Rejected: the source is already public, so obfuscating the compiler binary buys no real secrecy. See [ADR 0024](0024-oskac-build-obfuscate-is-an-opt-in-native-backend-pass.md) for the feature this project does want for compiled programs.

## Consequences

- Every native backend function gets a check, at the entry, for whether it has a local array or an address-taken local; this check decides whether a canary is laid out on its stack frame, when `--harden` is set.
- The differential suite must be run both with and without `--harden`, since both are real, supported shapes of the output.
- `--harden` and `--obfuscate` ([ADR 0024](0024-oskac-build-obfuscate-is-an-opt-in-native-backend-pass.md)) are independent flags; a user can set either, both, or neither.
