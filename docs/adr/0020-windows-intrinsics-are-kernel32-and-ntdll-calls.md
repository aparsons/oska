# On Windows, a system-call intrinsic is one kernel32 or ntdll call

- **Status:** Accepted (2026-11-19), for spec #155 (#176). On 2026-11-19 the user accepted all eight decisions the research recommends (its §4, Q1–Q8). Amends [ADR 0015](0015-macos-intrinsics-are-libsystem-calls.md) for Windows. The research behind it is [docs/research/2026-11-windows-kernel32-calls.md](../research/2026-11-windows-kernel32-calls.md), whose §4 lists the decisions (Q1–Q8).
- **Amended by:** [ADR 0024](0024-name-resolution-is-an-oska-stub-resolver-and-windows-asks-iphlpapi-for-its-servers.md), which adds one function, `GetNetworkParams` of `iphlpapi.dll`, for the system's DNS servers.

## Decision

ADR 0015's rule holds on Windows: a `std.rt` system-call intrinsic is **one call of the operating system's interface**. On Windows that is one function of `kernel32.dll` or `ntdll.dll`, or `ProcessPrng` of `bcryptprimitives.dll` for random bytes, which neither of the two provides.

Because Windows' functions are not POSIX-shaped, Windows has **its own intrinsics**, one per function, named for it and taking its arguments in its order, out-parameters included (`rt_sys_write_file(handle, buf, count, done)` for `WriteFile`). A platform lowers an intrinsic it does not have to a constant failure, with no call.

`std.rt` chooses between the two kinds by one table field, `Platform.windows`. Only the leaf functions that call the operating system test it. Everything above them is shared.

The backend may add around a Windows call only what ADR 0015 allows:

- **The calling convention:** the Windows x64 convention, with the 32-byte shadow store below the arguments.
- **One result convention:** a failure reaches `std.rt` as a negative number. For a kernel32 function that fails with `FALSE` or `NULL`, it is `-GetLastError()`. For an ntdll function, it is `-RtlNtStatusToDosError(status)` when the status is an error. `std.rt` words the Win32 code with Oska's table, as it words `errno`.
- **Constant trailing arguments,** such as `WriteFile`'s `NULL` for `OVERLAPPED`.

A Windows executable imports only the functions in its target's `Platform` in `oska-native`, from those three DLLs, always the whole list. The list is part of the reviewed surface, like macOS's.

Two runtime services differ in shape on Windows, and the backend owns them:

- **The stack.** The program runs on the thread's own stack, sized by the PE header's stack reserve, not on one `std.rt` maps. Frames over one page probe each page.
- **Faults.** `std.rt` installs a vectored exception handler through a small entry the backend generates. A fault ends the process with the exception's code, which `oskac` reports as `E0392`.

## Why

Windows has no file descriptors, no `mmap`, no signals and no `errno`, and most of its functions return their result through an out-parameter. Making them look like POSIX calls needs buffers and decisions in the backend, which ADR 0013 and ADR 0015 reject. Giving Windows its own intrinsics keeps every call visible in `std.rt` under the checker, and keeps the backend's part fixed and small. One field that chooses a leaf's body is a smaller exception to "no function asks which platform it runs on" than a second runtime.

## Considered options

- **Backend shims that give Windows calls POSIX shapes.** Rejected: hidden surface (ADR 0013, ADR 0015).
- **A separate `std.rt` for Windows.** Rejected: the shared code above the leaves would exist twice.
- **A `std.rt`-mapped stack with the TEB's stack bounds updated.** Rejected: writes undocumented structures.
- **`SetUnhandledExceptionFilter`.** Rejected: it runs after frame unwinding, which needs unwind data Oska does not write.
- **`SetConsoleOutputCP(65001)`** for console text. Rejected: it changes the user's console after the program ends. `std.rt` converts to UTF-16 and calls `WriteConsoleW` instead.

## Consequences

- `oska-native`'s `Import` names its DLL, and `Platform` says how its error function is read (an address, or the value). `Fails` gains `Zero`. `OsCall` gains `Absent`.
- The x86_64 code generator gets the Windows call form, and a page probe for large frames on Windows.
- The `std.rt` table gains `windows`, `e_intr`, `e_pipe` and `e_pipe_alt`, and `system_errors()` gains a Windows part from `oska-sys`'s `WINDOWS_ERRORS`. Linux and macOS keep today's values and make the same calls.
- The PE writer emits a minimal `.reloc`, so that the loader may randomize the image's address. Whether the loader does so is **unverified** until a Windows test compares addresses across runs.
- The IAT sits in writable `.data`. Making it read-only after binding needs the load-config directory, which a later ticket can add.
