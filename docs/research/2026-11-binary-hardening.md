# How can Oska best defend/harden the oskac binary and programs it compiles against decompilation and reverse engineering, and what's realistic vs. security theater?

Date of access: 2026-11-23

## 1. Framing: two very different problems

- **`oskac` is open source** (Apache-2.0, public GitHub repo). Obfuscating or "encrypting" the compiler/runtime binary itself buys almost nothing against a motivated adversary, since the source is already public. [Source: `aparsons/oska` repository license and visibility.]
- The real, legitimate goal for `oskac`'s own binary is **supply-chain integrity** (don't let a tampered binary be distributed) and **standard exploit-mitigation** (don't let a malicious `.osk` input corrupt the compiler process) — not secrecy.
- For **programs compiled by oskac**, "defend against decompilation" is a meaningful ask (protecting an ISV's logic from casual copying). Here the correct mental model, grounded in the academic impossibility result for general-purpose obfuscation (Barak et al., "On the (Im)possibility of Obfuscating Programs", CRYPTO 2001 / J. ACM 2012) is: you can raise the cost of static analysis, but you cannot hide a secret from an adversary who fully controls the execution hardware. Inference: any secret that must be *used* by code running on attacker-controlled hardware is eventually extractable; obfuscation buys analyst time, not confidentiality guarantees.

Source: Barak, Goldreich, Impagliazzo, Rudich, Sahai, Vadhan, Yang, "On the (Im)possibility of Obfuscating Programs," J. ACM 59(2), 2012. https://dl.acm.org/doi/10.1145/2160158.2160159

## 2. Repo-specific facts that shape the answer

- `oskac` emits ELF/Mach-O/PE directly — there is no external linker or assembler in the pipeline (per ADR 0011 / ADR 0014, and `crates/oska-native/src/{elf,macho,pe}.rs`). This means the compiler fully controls what goes into object/section/symbol tables, with no separate "strip the linker's output" step required: nothing needs to be emitted in the first place.
- The release pipeline (`docs/agents/release.md`, `SECURITY.md`) already signs `SHA256SUMS` with `ssh-keygen` and publishes a signed, expiring release index, double-verified by both a shell script and a second, independent verifier written in Oska itself. This is the correct tool for "protect the oskac binary from tampering," and goes further than most projects (an independent verifier in a second implementation).
- The musl release builds already avoid `PT_INTERP`/`DT_NEEDED` (static linking), which also removes import-table information that is often the first thing a reverse engineer opens in a decompiler.

Source: `docs/agents/release.md`, `SECURITY.md`, `crates/oska-native/src/elf.rs`, `macho.rs`, `pe.rs` (repository state as of 2026-11-23). Inference: these are read directly from the repository, not an external primary source, and should be re-verified against current file contents before relying on them, since this note doesn't carry a line-exact citation for each file.

## 3. What's achievable (raises real cost) vs. security theater

**Achievable, standard, defensible:**

- **Symbol/debug-info stripping.** Rust's release profile supports `strip = true` (or `strip = "symbols"` / `"debuginfo"`) to omit symbols and debug info from the final binary. Source: The Cargo Book, "Profiles," `strip` field — https://doc.rust-lang.org/cargo/reference/profiles.html#strip
- **Panic strategy and codegen-units** affect what ends up embedded for unwinding; `panic = "abort"` removes unwind tables. Source: The Cargo Book, "Profiles," `panic` field — https://doc.rust-lang.org/cargo/reference/profiles.html#panic
- **PIE (position-independent executables), stack protector, RELRO/`-z now` (full RELRO), NX/W^X, and control-flow integrity landing pads** (`BTI` on AArch64 via PAC/BTI extensions, Intel CET `ENDBR64` on x86_64) are standard, well-documented mitigations. Since `oska-native` generates every instruction and section itself, these become backend emission policy rather than a hope that an external linker applied them.
  - AArch64 PAC/BTI: Arm, "Armv8-A Architecture Reference Manual," Pointer Authentication and Branch Target Identification sections. https://developer.arm.com/documentation/ddi0487/latest
  - x86_64 CET: Intel, "Control-flow Enforcement Technology Specification." https://www.intel.com/content/www/us/en/developer/articles/technical/technical-look-at-intel-cet.html
  - RELRO/NX: Linux kernel / `man-pages`, `execve(2)` and ELF hardening discussions; also documented in the OWASP "Buffer Overflow" and binary-hardening cheat sheets. https://cheatsheetseries.owasp.org/
- **Code signing** on macOS (`codesign`, Gatekeeper, notarization) and Windows (Authenticode) is the standard, primary-source-documented way to assure users a binary is unmodified and from a known publisher — this is an integrity control, not an obfuscation control.
  - Apple: "Notarizing macOS software before distribution." https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
  - Microsoft: "Introduction to Code Signing." https://learn.microsoft.com/en-us/windows-hardware/drivers/install/introduction-to-code-signing
- **Static linking / no `PT_INTERP`** removes dynamic-import-table leakage (named libc/Win32 imports are a decompiler's first foothold, since tools like disassemblers auto-recognize library calls by name and infer behavior from them). This is a direct, well-understood trade-off: static reduces both supply-chain surface and import-name leakage, at the cost of needing OS services some other way.
- **Control-flow obfuscation and string/constant encryption** implemented as an optional codegen pass (flattening, opaque predicates, junk instructions, basic-block reordering, encrypted constants decrypted on first use) can meaningfully raise decompiler/analyst time when done with full IR/semantic knowledge (which `oska-native`'s own-codegen architecture allows, unlike a generic post-link binary rewriter). Inference: no primary source quantifies "how much" time this buys; the qualitative claim that doing these transforms with semantic knowledge is stronger than post-hoc binary rewriting is this agent's own conclusion, not sourced.

**Security theater (commonly sold, usually ineffective against a real adversary, often net-negative):**

- **Packers / self-decrypting executables.** The decrypted, runnable image must exist in memory at some point, so any debugger or memory dumper recovers it. This category (UPX through VMProtect/Themida) has an extensive public unpacking literature.
- **Anti-debugging tricks** (`ptrace` detection, timing checks, debugger-presence checks) are typically a one-line patch to defeat once found, and they actively break legitimate infrastructure: crash reporters, sandboxes, and container runtimes. Given Oska targets containerized/CI environments (per the musl/ghcr image pipeline), these are especially likely to misfire on legitimate use rather than attackers.
- **Encrypting the whole binary and decrypting at startup** conflicts directly with code signing: Gatekeeper/notarization on macOS and SmartScreen/Authenticode checks on Windows expect a static, non-self-modifying executable, and self-modifying or writable-executable pages are flagged by both platforms' security models. Source: Apple notarization docs (above); Microsoft SmartScreen documentation — https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/
- **VM-based bytecode obfuscation** for a compiled, native-codegen, performance-oriented language works against the project's stated architecture (ADR 0011/0014's own native-codegen design), and VM-dispatch-loop deobfuscation (e.g., via taint tracking) is an established, teachable technique in the reverse-engineering literature.
- **Obfuscating `oskac`'s own binary** is close to pure theater given the source is public on GitHub — effort here has near-zero payoff relative to effort spent on the areas above.

## 4. Bottom line

- Supply-chain integrity (signing, independent verification) is already the correct answer for "protect the oskac binary," and the project's existing signed-release pipeline already exceeds common industry practice.
- Standard exploit mitigations (stripping, PIE, stack protector, RELRO, NX, CFI landing pads, code signing) are inexpensive, well-documented, and should be default build hygiene for both `oskac` and the programs it emits; because `oska-native` owns code generation end-to-end, these become emission policy rather than something hoped-for from an external linker.
- Control-flow obfuscation and string/constant encryption for **compiled programs** (not `oskac` itself) is the one area with genuine, buildable RE-resistance, but it should be presented to users as "raises analyst cost," never as protecting a secret, since nothing running on attacker-controlled hardware can guarantee the latter (Barak et al. impossibility result, §1).
- Packing, anti-debugging, and whole-binary encryption should be explicitly rejected: they are the theater category, and they actively conflict with code-signing and reproducible-build goals the project already has in place.

## Prompt-injection notes

None of the sources consulted contained instructions directed at the research agent; no injection attempts were noted.

## Sources

- Barak et al., "On the (Im)possibility of Obfuscating Programs," J. ACM 59(2), 2012 — https://dl.acm.org/doi/10.1145/2160158.2160159
- The Cargo Book, "Profiles" (`strip`, `panic` fields) — https://doc.rust-lang.org/cargo/reference/profiles.html
- Arm, "Armv8-A Architecture Reference Manual" (Pointer Authentication, BTI) — https://developer.arm.com/documentation/ddi0487/latest
- Intel, "Control-flow Enforcement Technology" — https://www.intel.com/content/www/us/en/developer/articles/technical/technical-look-at-intel-cet.html
- OWASP Cheat Sheet Series (binary/build hardening) — https://cheatsheetseries.owasp.org/
- Apple, "Notarizing macOS software before distribution" — https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- Microsoft Learn, "Introduction to Code Signing" — https://learn.microsoft.com/en-us/windows-hardware/drivers/install/introduction-to-code-signing
- Microsoft Learn, SmartScreen documentation — https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/
- Repository state as of 2026-11-23: `docs/agents/release.md`, `SECURITY.md`, `crates/oska-native/src/{elf,macho,pe}.rs` (ADR 0011, ADR 0014)
