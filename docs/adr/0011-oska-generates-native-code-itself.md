# Oska generates native code itself, with no external toolchain

## Decision

Oska gets its own native code generator. It reads the checked program ([ADR 0007](0007-interpreter-runs-the-checked-program.md)) and writes finished executables directly: machine code, object format and linking. It uses no LLVM, no Cranelift, no system C compiler and no system linker.

- **Written in Rust first.** The code generator is a new workspace crate. Rewriting the toolchain in Oska (self-hosting) is a goal for later, not a prerequisite.
- **Targets in order.** aarch64 Linux first, as a static ELF executable that makes system calls directly with no libc. This is the development machine's platform, so every test runs locally. Then x86_64 Linux. Then macOS (Mach-O, linked dynamically against libSystem through dyld, because macOS has no stable system-call interface, and ad-hoc signed on arm64). Then Windows (PE, importing from kernel32). The interpreter keeps running programs on targets that have no backend yet.
- **The runtime is Oska code.** Reference counting, panics, the capability host and filesystem confinement are written in Oska and compiled into every program, on top of a small set of system-call intrinsics that the code generator lowers directly ([ADR 0013](0013-system-calls-only-in-std-rt.md)).
- **One behaviour.** A differential test suite runs every program on both the interpreter and the native backend and requires identical output, exit codes and panics, until the interpreter's Rust host is retired.

## Why

The workspace takes no crates from outside the repo, so oskac can't embed LLVM, Cranelift or a WASM engine. A compiled Oska is needed because the interpreter runs about 4 million simple operations a second on a Raspberry Pi 5, 50–100 times slower than native code. That's too slow for the TLS, git and crypto code that [ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md) puts in Oska. Writing the runtime in Oska puts the security-critical confinement logic under the checker, and avoids linking Rust object code without a linker.

## Considered options

- **Emit C and call the system compiler.** Rejected: building a program would depend on outside software. It was the cheapest route to optimized code on every platform.
- **WASM with our own engine.** Rejected: a WASM interpreter is about as slow as today's interpreter, and making it fast is the native-codegen problem again.
- **WASM with an external runtime** (wasmtime, a browser). Rejected: running a program would depend on outside software. This replaces the earlier roadmap line, "WASM/WASI as the first compiled target".
- **A Rust no_std runtime and our own linker.** Rejected: confinement would be reused, but we would have to link rustc's object files and relocations on three object formats.
- **Runtime routines emitted as per-architecture machine code.** Rejected: security code as generated assembly is the hardest kind to review.

## Consequences

- Performance depends on our own optimizer. Plain code will run slower than LLVM output until the optimizer gets serious work.
- Every OS gets its own ticket: object format, system-call or import layer, and signing.
