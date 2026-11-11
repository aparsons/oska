# Raw system calls are allowed only in `std.rt`

## Decision

The native runtime is Oska code ([ADR 0011](0011-oska-generates-native-code-itself.md)). It reaches the operating system through system-call intrinsics (`openat`, `read`, `write` and a few more) that the code generator lowers directly to machine code.

These intrinsics exist only inside one module that ships with the toolchain, `std.rt`. That module is compiled into every program:

- **The checker** rejects, with its own error code, any import of `std.rt` and any use of an intrinsic outside it, in user code and dependency code alike.
- **`scripts/check.sh`** checks that only `std.rt` uses them, in the same way the unsafe guard keeps `unsafe` inside `oska-sys` ([ADR 0009](0009-oska-sys-is-the-only-crate-with-unsafe.md)).

## Why

A raw system call is **ambient authority**: code that can call `openat` bypasses every capability. Keeping all raw OS access in one small, toolchain-owned module gives reviewers a single place to check, and the same rule as `oska-sys`.

## Considered options

- **Make system calls the first use of the future `unsafe`/FFI feature.** Rejected: it would pull the whole `unsafe` policy and the manifest allow-list into the backend work.
- **Body-less prelude functions whose system-call wrappers live inside the code generator.** Rejected: the system-call surface would be hidden in Rust code, not visible as Oska.
