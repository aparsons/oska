# `oskac build --obfuscate` is an opt-in native-backend pass, not a packer

- **Status:** Accepted (2026-11-23). The user settled each decision below in a grilling session on 2026-11-23. The research behind it is [docs/research/2026-11-binary-hardening.md](../research/2026-11-binary-hardening.md).

## Decision

`oskac build --obfuscate` runs an obfuscation pass in the native backend ([ADR 0011](0011-oska-generates-native-code-itself.md)) over a compiled program, with the full set of transforms in its first version:

- string and constant literals encrypted, decrypted on first use;
- control-flow flattening;
- opaque predicates;
- junk instruction insertion;
- basic-block reordering.

The flag is opt-in: a program built without it is unaffected. [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)'s parity gate applies to `--obfuscate` itself, so it ships on Linux, macOS and Windows (aarch64 and x86_64) in one change, not platform by platform. The differential suite must pass unchanged with `--obfuscate` on: the pass reshapes a program's machine code, never its output, exit code or panics.

This is a separate flag from [ADR 0023](0023-compiled-programs-get-a-stack-canary-and-cfi-landing-pads.md)'s `--harden` (the stack canary and CFI landing pads); a user can set either, both, or neither. `--obfuscate` is the user's own choice, for their own compiled program, and raises an analyst's cost of reading it; it does not, and cannot, keep a secret from someone who runs the program on hardware they control (the research note's framing, following the impossibility result for general-purpose obfuscation). This project does not build or ship packers, anti-debugging tricks, or whole-binary encryption: they would conflict with code signing and with the reproducible, signed-release pipeline that already defends `oskac` itself.

## Why

Oska owns every instruction its native backend emits, with full knowledge of the checked program's structure. That is a real advantage over a tool that rewrites someone else's compiled object code after the fact: these transforms can be done with full semantic knowledge, not by pattern-matching machine code. The research note found this to be the one area with genuine, buildable resistance to casual reverse engineering, as long as it is sold to users as raising cost, not as protecting a secret.

## Considered options

- **String and constant encryption only, for the first version.** Rejected for the first version: the user chose to ship the full set now rather than grow it transform by transform.
- **No obfuscation feature at all, hardening only.** Rejected: the user chose to build this, judging the raised analyst cost worth the engineering and review cost, for programs `oskac` compiles that are not `oskac` itself.
- **A VM-based bytecode layer.** Rejected: fights the project's own native, no-linker architecture (ADR 0011), and VM-dispatch deobfuscation is an established technique.
- **Packing, anti-debugging, or whole-binary encryption.** Rejected as security theater: a packed or self-decrypting image must exist, runnable, in memory at some point, so a debugger recovers it; anti-debugging tricks are a one-line patch to defeat and misfire on legitimate infrastructure (crash reporters, sandboxes, containers); and both conflict with code signing on macOS and Windows and with reproducible builds.

## Consequences

- The native backend's lowering and optimization stages get a new, optional pass between them and the per-architecture emitters, gated by `--obfuscate`.
- The differential suite's corpus must be run with `--obfuscate` on as part of its coverage, on every supported platform, before this ships.
- Documentation for `--obfuscate` must say plainly that it raises the cost of reading a compiled program, and does not protect a secret the program uses at run time, so a user does not rely on it for something it cannot do.
