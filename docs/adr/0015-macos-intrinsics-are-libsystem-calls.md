# On macOS, a system-call intrinsic is one libSystem call

- **Status:** Accepted (2026-11-17), for spec #154. On 2026-11-17 the user accepted all six decisions the research recommends (its §4, Q1–Q6). Amends [ADR 0013](0013-system-calls-only-in-std-rt.md). The research behind it is [docs/research/2026-11-macos-libsystem-calls.md](../research/2026-11-macos-libsystem-calls.md).

[ADR 0020](0020-windows-intrinsics-are-kernel32-and-ntdll-calls.md) amends this decision for Windows: there, a system-call intrinsic is one kernel32 or ntdll function (or `ProcessPrng`), with the Windows x64 call form around it.

## Decision

A system-call intrinsic in `std.rt` is **one call of the operating system's interface**:

- on Linux, one system call by number;
- on macOS, one function of `libSystem`, which is the only supported way to call the macOS kernel ([ADR 0011](0011-oska-generates-native-code-itself.md)).

The backend may add only these things around that one call:

- **The platform's calling convention.** That means loading the function's address from the executable's import table, placing variadic arguments where the processor's ABI puts them, and sign-extending a 32-bit result.
- **One result convention.** A failure reaches `std.rt` as a negative `errno`, on every platform. On macOS the backend reads `errno` through `__error()` right after a failing call.
- **Constant trailing arguments** that the operating system's call takes and the intrinsic does not, such as `rt_sigaction`'s mask size or `sigaction`'s old action.
- **No call at all,** for an intrinsic the operating system has no use for. On Linux, opening a directory for listing needs no call.

It adds no loops, no buffers and no decisions. Anything more than this is Oska code in `std.rt`, built on the table of the target's numbers, flags and layouts.

A macOS executable imports functions from `/usr/lib/libSystem.B.dylib` only. It imports only the functions listed in its target's `Platform` in `oska-native`. That list is part of the reviewed surface of the operating-system interface, like the `RUNTIME` intrinsic table.

ADR 0013's other rules are unchanged:

- The intrinsics exist only in `std.rt`.
- The checker refuses them elsewhere (`E0209`).
- The intrinsic guard in `scripts/check.sh` enforces this.

## Why

macOS has no stable system-call interface. Every call goes through a C function, which differs from a Linux system call in how it is named (a symbol bound by dyld), how it fails (`-1` and a thread-local `errno`) and how it is called (Apple's arm64 ABI puts variadic arguments on the stack). Some code around each call is unavoidable. ADR 0013 rejected system-call wrappers inside the code generator because they hide the system-call surface in Rust. This ADR keeps the part that must be in the backend fixed and small, and leaves every behaviour in Oska where the checker sees it.

One result convention means `std.rt`'s error handling is written once, for every operating system.

## Considered options

- **`std.rt` reads `errno` itself, through a new intrinsic.** Rejected: every error path in `std.rt` would test a different failure value on each operating system.
- **Per-operating-system wrappers in the backend** that give macOS calls Linux's exact shape, such as copying `readdir`'s record into a buffer or measuring `getcwd`'s result. Rejected: this is the hidden surface ADR 0013 rejected. Where shapes differ, the intrinsic's contract is made neutral instead, and `std.rt` handles it.
- **Direct `svc`/`syscall` to the macOS kernel.** Rejected: Apple does not support it, and the numbers and conventions change between releases (ADR 0011).

## Consequences

- `oska-native`'s `Platform` describes each call as a system call or a library import. The IR gains a library-call instruction, and Mach-O gets a GOT bound by chained fixups.
- Some intrinsics get contracts that both operating systems meet: `rt_sys_getcwd`, `rt_sys_random`, `rt_sys_read_dir`, and the new `rt_sys_open_dir` and `rt_sys_close_dir`. Linux makes the same system calls as before.
- Windows (v0.6.0) follows the same rule, with kernel32 and ntdll as its libraries.
