# Research: how `std.rt` calls Windows through kernel32 and ntdll

- **Date:** 2026-11-19
- **Status:** proposed, for #176 (spec #155). Nothing here is implemented. §4 lists the decisions for the user, each with a recommendation.
- **Question:** ADR 0015 made a `std.rt` system-call intrinsic "one call of the operating system's interface" and said Windows follows the same rule with kernel32 and ntdll. What is the smallest shape that lets `std.rt` run small programs on Windows (#176), then all of them (#177–#179), without changing what Linux and macOS executables do?
- **Method:**
  - Read the Oska sources at `origin/main` `704dc89` (2026-11-19). Claims about Oska cite the file and the function or line there.
  - Fetched Microsoft's documentation sources on 2026-11-19 from GitHub and read them where cited:
    - `MicrosoftDocs/win32` `desktop-src/Debug/pe-format.md` (the PE format), `desktop-src/ProcThread/thread-stack-size.md`, `desktop-src/SecCNG/processprng.md`.
    - `MicrosoftDocs/cpp-docs` `docs/build/x64-calling-convention.md`, `x64-software-conventions.md`, `stack-usage.md`, `docs/c-language/parsing-c-command-line-arguments.md`.
    - `MicrosoftDocs/sdk-api` (`sdk-api-src/content/`): `WriteFile`, `ReadFile`, `VirtualAlloc`, `VirtualFree`, `ExitProcess`, `GetCommandLineW`, `GetEnvironmentStringsW`, `AddVectoredExceptionHandler`, `SetThreadStackGuarantee`, `GetSystemTimePreciseAsFileTime`, `GetLastError`, `RtlGenRandom`, `BCryptGenRandom`, `NtCreateFile`, `GetFileType`.
    - `MicrosoftDocs/Console-Docs`: `GetStdHandle`, `GetConsoleMode`, `WriteConsole`, `SetConsoleOutputCP`.
  - No Windows machine was available, so nothing was run on Windows. Claims that no fetched source settles are marked **unverified**. The Windows CI runner settles them.

## Summary

macOS needed a calling convention, a symbol table and an `errno` read around POSIX-shaped calls. Windows is further away: it has no file descriptors, no `mmap`, no signals and no `errno`, and most of its functions give their result through an out-parameter and say only `TRUE` or `FALSE`. A Windows call cannot be a POSIX intrinsic with different numbers, and ADR 0015 forbids the backend from adding buffers or decisions to make it look like one.

So the design keeps ADR 0015's rule (**one intrinsic, one call**) and moves the difference up one level, into `std.rt`:

1. **Windows gets its own intrinsics**, one per kernel32 or ntdll function, named for it (`rt_sys_write_file`, `rt_sys_virtual_alloc`, …). Each takes the function's own arguments, out-parameters included. A platform lowers the intrinsics it does not have to a constant failure, so the import list stays fixed and nothing is called.
2. **`std.rt` has two bodies for each operating-system leaf function** (about 20 of them, such as `write_all`, `map`, `process_init`), chosen by a new table field, `windows: Bool`. Everything above the leaves (text, lists, the allocator's free lists, panics, the confinement walk's logic) is shared.
3. **The result convention stays one rule.** A failing call gives `-code`, where `code` is the Win32 error code (`GetLastError`) or, for an ntdll call, the code `RtlNtStatusToDosError` gives. `std.rt` already maps codes to Oska's wording through its table (`errors_at`, `errors_count`), and `oska-sys` has the Windows table (`WINDOWS_ERRORS`).
4. **The process runs on the thread's own stack**, sized by the PE header, instead of a stack `std.rt` maps and switches to. Windows' exception dispatch and stack growth trust the thread's stack bounds in its TEB.
5. **Faults go to a vectored exception handler** that `std.rt` installs through a small generated entry, and the process ends with the exception's code, which `oskac` reports as `E0392`.

Linux and macOS executables make the same calls as today. Their tables gain the new fields with values that keep today's behaviour.

## 1. What exists today

- **Intrinsics.** `std.rt` declares 23 `rt_sys_` intrinsics, all POSIX-shaped (`rt.osk`, "System calls"). `oska-native` maps each to an `OsCall` per platform: `Trap` (Linux), `Library` (macOS) or `Same` (`intrinsics.rs`, `Platform.call`).
- **Where `std.rt` calls them.** In about 25 call sites, all in leaf functions: `map` and `stack_new` (`mmap`), `free` and `stdin_fill` (`munmap`), `write_all` (`write`), `stdin_fill` and `read_all` (`read`), `process_init`, `set_action`, `handle_faults` and `on_fault` (signals), `stdout_buffer` (`ioctl`), `exit`, `clock_now_millis`, `random_next_int`, and the confinement functions (`open_name`, `make_dir`, `lone_file`, `read_all`, `write_beneath`, `working_directory`, `link_target`, `open_dir`, `entries`).
- **Library calls.** `Inst::LibCall` calls through an import-table slot. `lower.rs`'s `library_call` turns `-1` or `NULL` into `-errno` by calling the platform's `errno` function (`___error`), which gives the address of `errno`.
- **Platform table.** `"No function holds such a value, or asks which platform it runs on"` (`rt.osk`, "Platforms"). Every difference is a value in `Platform`.
- **Entry.** `_start` calls `process_init(argc, argv, on_fault)`, then `stack_new(size)`, then `_main` on that stack (`Inst::CallOnStack`) (`lower.rs`, `Lowering::entry`). `process_init` reads `envp` after `argv`'s null, and refuses an argument that is not UTF-8.
- **x86_64 code.** The backend's own convention passes arguments in `rdi, rsi, rdx, rcx, r8, r9`. `rbx`, `r12`–`r14` hold temporaries across calls, `r15` counts call depth, and `rbp` is the frame pointer. Stack slots are at `[rsp + 8 * slot]`. All addressing is `rip`-relative, so code needs no relocation (`x86_64.rs`, module comment). `LibCall` uses the System V convention (`x86_64.rs`, `Inst::LibCall`).
- **Errors.** `std.rt` words a system error through `system_errors()`, a `|`-separated list with one part per operating system (`rt.osk`, `errno_message`). `oska-sys` holds `WINDOWS_ERRORS`, the Win32 codes of each `IoError` kind (`wording.rs`).
- **Windows confinement in the interpreter.** `oska-sys/src/windows.rs` walks beneath a directory handle with `NtCreateFile` and `FILE_OPEN_REPARSE_POINT`, and refuses reparse points and files with more than one link (#175). `std.rt` will do the same in #178.
- **CI.** `windows.yml` runs `cargo test` for `oska-run` and `oskac` on `windows-latest` on every push to `main`, `ticket/**` and every pull request that changes `crates/`. A `#[cfg(windows)]` test in `oskac` runs there with no workflow change.

## 2. What Windows requires (primary sources)

### Calling convention (x64-calling-convention.md, x64-software-conventions.md, stack-usage.md)

- Integer arguments go in `RCX`, `RDX`, `R8`, `R9`. "Any parameters beyond the first four must be stored on the stack after the shadow store before the call."
- "The caller must always allocate sufficient space to store four register parameters, even if the callee doesn't take that many parameters." The callee may write those 32 bytes, the **shadow store**, at `[rsp]`…`[rsp+31]` as the caller sees them before the `call`.
- `RAX`, `RCX`, `RDX`, `R8`–`R11` and `XMM0`–`XMM5` are volatile. `RBX`, `RBP`, `RDI`, `RSI`, `RSP`, `R12`–`R15` and `XMM6`–`XMM15` are nonvolatile.
- The stack pointer is 16-byte aligned at a call. Variadic functions take arguments as other functions do.
- `int`/`BOOL`/`DWORD` results are 32 bits in `EAX`; the top 32 bits of `RAX` are not specified (**unverified**: the documents say the return value is in `RAX` and say nothing of the top half for 32-bit types; compilers treat them as undefined).

What this means for Oska:

- Oska's stack slots are at `[rsp]` upward. A Windows callee's shadow store is the same 32 bytes. **So a Windows `LibCall` must lower `rsp` by 32 bytes (plus the stack arguments, rounded to 16) before the call and restore it after.** Without that, every kernel32 call may overwrite four of the caller's temporaries.
- `rsi` and `rdi` are nonvolatile on Windows. Oska keeps no temporary in them across a call, so it loses nothing. But code that Windows calls and that returns to Windows (the exception handler, if it returns) must preserve them.
- `rbx`, `r12`–`r15` and `rbp` are nonvolatile in both conventions. The register model does not change.

### The executable (pe-format.md)

- Headers: an MS-DOS stub with `e_lfanew` at 0x3C, `PE\0\0`, the COFF header (`IMAGE_FILE_MACHINE_AMD64` 0x8664), and the PE32+ optional header (magic 0x20B).
- `TimeDateStamp` is "the low 32 bits of the number of seconds since 00:00 January 1, 1970". Writing 0 is valid, and the file is then the same on every host. The `CheckSum` is checked only for drivers, boot-time DLLs and DLLs loaded into critical processes, so 0 is valid for an executable.
- `IMAGE_FILE_RELOCS_STRIPPED` (0x0001): "the file does not contain base relocations and must therefore be loaded at its preferred base address". Oska's code is position-independent, so this flag stays clear.
- `DllCharacteristics`: `HIGH_ENTROPY_VA` 0x0020, `DYNAMIC_BASE` 0x0040, `NX_COMPAT` 0x0100, `TERMINAL_SERVER_AWARE` 0x8000.
- `SectionAlignment` defaults to the page size (4 KiB). `FileAlignment` "should be a power of 2 between 512 and 64 K"; default 512.
- `SizeOfStackReserve`: "Only SizeOfStackCommit is committed; the rest is made available one page at a time until the reserve size is reached." thread-stack-size.md: the system commits pages "until either the stack reaches the reserved size minus one page (which is used as a guard page to prevent stack overflow)", and rounds the reserve up to the allocation granularity (typically 64 KiB).
- The import directory: one `IMAGE_IMPORT_DESCRIPTOR` per DLL, each with an import lookup table and an import address table (IAT) of 8-byte entries, ending with a null entry, and a hint/name table. The loader fills the IAT.
- **Unverified:** whether the loader applies ASLR to an image with `DYNAMIC_BASE` but no `.reloc` section. The format document does not say. The MinGW toolchain's history suggests that such an image is loaded at its preferred base, with no randomization. §3.6 adds a minimal `.reloc` so that the question does not matter.

### Functions

| Function | DLL | Result | Failure | Source |
| --- | --- | --- | --- | --- |
| `GetStdHandle(n)` | kernel32 | `HANDLE` | `INVALID_HANDLE_VALUE` (-1); `NULL` if there is none | Console-Docs |
| `WriteFile(h, buf, n, &done, NULL)` | kernel32 | `BOOL` | `FALSE`, then `GetLastError` | sdk-api |
| `ReadFile(h, buf, n, &done, NULL)` | kernel32 | `BOOL` | `FALSE`, then `GetLastError` | sdk-api |
| `GetConsoleMode(h, &mode)` | kernel32 | `BOOL` | `FALSE` when `h` is not a console | Console-Docs |
| `WriteConsoleW(h, buf, n, &done, NULL)` | kernel32 | `BOOL` | `FALSE`, then `GetLastError` | Console-Docs |
| `VirtualAlloc(addr, size, type, protect)` | kernel32 | address | `NULL`, then `GetLastError` | sdk-api |
| `VirtualFree(addr, 0, MEM_RELEASE)` | kernel32 | `BOOL` | `FALSE`; `size` must be 0 and `addr` the base `VirtualAlloc` gave | sdk-api |
| `ExitProcess(code)` | kernel32 | does not return | | sdk-api |
| `GetCommandLineW()` | kernel32 | `LPWSTR` | cannot fail | sdk-api |
| `GetEnvironmentStringsW()` | kernel32 | block | `NULL` | sdk-api |
| `AddVectoredExceptionHandler(first, h)` | kernel32 | handle | `NULL` | sdk-api |
| `SetThreadStackGuarantee(&size)` | kernel32 | `BOOL` | `FALSE` | sdk-api |
| `GetSystemTimePreciseAsFileTime(&ft)` | kernel32 | none | cannot fail; 100 ns since 1601-01-01 UTC | sdk-api |
| `GetLastError()` | kernel32 | `DWORD` | cannot fail | sdk-api |
| `ProcessPrng(buf, n)` | bcryptprimitives | `BOOL` | "Always returns TRUE" | win32 |
| `NtCreateFile`, `NtQueryInformationFile`, `NtQueryDirectoryFile`, `NtClose`, `RtlNtStatusToDosError` | ntdll | `NTSTATUS` (negative on error) / code | | sdk-api, `oska-sys/src/windows.rs` |

Other facts:

- `WriteFile` to a pipe whose reader has gone fails with `ERROR_NO_DATA` (232) or `ERROR_BROKEN_PIPE` (109). `oska-sys` words both as "broken pipe" (`WINDOWS_ERRORS`). **Unverified** which one a closed anonymous pipe gives; `std.rt` treats both as a closed pipe.
- Win32 error 4 is `ERROR_TOO_MANY_OPEN_FILES`, not `EINTR`, and 32 is `ERROR_SHARING_VIOLATION`, not `EPIPE`. `std.rt` tests `== -4` (retry) and `== -32` (closed stdout) in shared code (`write_all`, `write_stdout`, `stdin_fill`, `entries`). Those numbers must move into the table, or a Windows write that fails with error 4 would retry for ever.
- A console decodes `WriteFile`'s bytes in its output code page, which is often not UTF-8. `WriteConsoleW` takes UTF-16 and shows any text correctly. `SetConsoleOutputCP` changes the console's code page for every process attached to it, and the change outlives the program.
- `GetCommandLineW` gives the whole command line as one UTF-16 string. The C runtime splits it by the rules in parsing-c-command-line-arguments.md (backslashes before a quote, `""` inside quotes, the program name split differently). `CommandLineToArgvW` is in shell32, which the spec does not allow.
- Windows has no `SIGPIPE`, so nothing needs to be ignored at start.

## 3. The design

### 3.1 Windows intrinsics

Each kernel32 or ntdll function that `std.rt` calls gets one intrinsic, named for it in snake case, with its arguments in its order:

```text
rt_sys_get_std_handle(n)                       GetStdHandle
rt_sys_write_file(h, buf, n, done)             WriteFile, with NULL for the OVERLAPPED
rt_sys_read_file(h, buf, n, done)              ReadFile, with NULL
rt_sys_get_console_mode(h, mode)               GetConsoleMode
rt_sys_write_console(h, buf, n, done)          WriteConsoleW, with NULL
rt_sys_virtual_alloc(addr, size, type, prot)   VirtualAlloc
rt_sys_virtual_free(addr)                      VirtualFree, with 0 and MEM_RELEASE
rt_sys_exit_process(code)                      ExitProcess
rt_sys_command_line()                          GetCommandLineW
rt_sys_environment()                           GetEnvironmentStringsW
rt_sys_add_exception_handler(handler)          AddVectoredExceptionHandler, with first = 1
rt_sys_stack_guarantee(size)                   SetThreadStackGuarantee
rt_sys_system_time(ft)                         GetSystemTimePreciseAsFileTime
rt_sys_process_prng(buf, n)                    ProcessPrng
```

#178 adds the ntdll file calls (`NtCreateFile`, `NtQueryInformationFile`, `NtQueryDirectoryFile`, `NtSetInformationFile`, `NtClose`) on the same pattern, with the code from `RtlNtStatusToDosError`.

The backend may add around each one only what ADR 0015 already allows: the calling convention, one result convention, and constant trailing arguments (`NULL` for `OVERLAPPED`, `0, MEM_RELEASE` for `VirtualFree`, `1` for "first handler").

The POSIX intrinsics lower on Windows to a constant failure, and the Windows intrinsics lower on Linux and macOS to the same: a new `OsCall::Absent` whose result is a fixed negative value and which makes no call. `std.rt` never reaches them, because it chooses the body by the table's `windows` field. The constant keeps a mistake harmless and visible instead of calling something.

### 3.2 `std.rt`: two bodies per leaf

The table gains `windows: Bool`. Each leaf function that calls the operating system tests it first and runs the Windows body (`if platform.windows { return win_write_all(fd, buf, count) }`). This changes one line of `rt.osk`'s "Platforms" comment: functions still hold no numbers, but about 20 leaves ask whether the platform is Windows.

The rest of `std.rt` stays shared. In particular:

- **Handles in place of descriptors.** A Windows handle is a word, so it fits wherever `std.rt` keeps a descriptor. `process_init` stores the three standard handles from `GetStdHandle` in writable words, and the Windows `write_all` maps the descriptors 0, 1 and 2 to them. Real handles are multiples of 4, so they never collide with 0, 1 and 2 (**unverified**, from Windows' handle-table layout; not in a fetched source).
- **Arguments and environment.** On Windows, `process_init` gets no `argc`/`argv`. It reads `GetCommandLineW()` and `GetEnvironmentStringsW()`, converts them from UTF-16 to UTF-8 in Oska, splits the command line by the C runtime's rules, and builds the same `argv`/`envp` arrays of C strings that Linux's kernel gives. `args_get`, `env_lookup` and the UTF-8 checks then work unchanged. An unpaired surrogate is invalid text, refused as a non-UTF-8 argument is today (exit 101), and an environment variable with one has no value, as `std::env::var` behaves.
- **Memory.** The Windows `map` is `VirtualAlloc(0, length, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)`; `free` of a large object is `VirtualFree(addr, 0, MEM_RELEASE)`. Every `munmap` in `std.rt` already unmaps a whole mapping from its base.
- **Standard output.** On Windows, `stdout_buffer` asks `GetConsoleMode` in place of `ioctl(TCGETS)`. When stdout is a console, a write converts UTF-8 to UTF-16 in Oska and calls `WriteConsoleW`. Otherwise `WriteFile` writes the bytes as they are, with no newline translation.
- **Clock and random.** The Windows `clock_now_millis` converts a `FILETIME` (100 ns since 1601) to Unix milliseconds. `random_next_int` reads its seed from `ProcessPrng`.

### 3.3 Errors

- A kernel32 function that fails with `FALSE` or `NULL` gives `-GetLastError()`. A new `Fails::Zero` covers `BOOL` and `NULL` results, and `Platform` names its error function with how to read it: an address to load from (`___error`) or the value itself (`GetLastError`).
- ntdll functions give an `NTSTATUS`. Its intrinsics (#178) give `-RtlNtStatusToDosError(status)` when the status is negative. That is a second import, `RtlNtStatusToDosError`, read the same way.
- The table gains `e_intr` and `e_pipe`, and a second broken-pipe code `e_pipe_alt`, which replace the literals `-4` and `-32` in shared code. Linux and macOS keep 4 and 32; Windows has no interrupted call (0, which no failure gives) and uses 232 and 109.
- `system_errors()` gains a Windows part. Win32 codes go up to 1460 (`ERROR_TIMEOUT`), so the part is generated from `WINDOWS_ERRORS` as the others are, and `std_rt_errors.rs` checks it against `oska-sys`. **Open:** a 1460-entry part adds about 1.5 KB of read-only text to every executable on every platform. A sparse form (code and wording pairs) is an alternative for #177.

### 3.4 The stack

On Linux and macOS, `_start` maps a stack with a guard and switches to it (`stack_new`, `CallOnStack`). On Windows, the design keeps the thread's own stack and sizes it in the PE header:

- `SizeOfStackReserve` is the size `Lowering::size_stack` computes, rounded up to 64 KiB, plus 1 MiB for Windows' own use. `SizeOfStackCommit` stays small.
- Windows grows a stack one guard page at a time, so a frame larger than a page must touch each page in order. On Windows, a function whose frame is more than 4 KiB probes each page in its prologue, as MSVC's `__chkstk` does. Oska's frames are `8 * temps + 104` bytes, so this is rare.
- `_start` skips `stack_new` and calls `_main` directly, with the call-depth counter set as `CallOnStack` sets it. `process_init` calls `SetThreadStackGuarantee` so that the fault handler has stack to run on after an overflow.

**Why not map our own stack, as on Linux?** Windows records each thread's stack bounds in its TEB. Exception dispatch and the stack's guard page use those bounds (**unverified** in a fetched source; this is why fiber and coroutine libraries on Windows update the TEB when they switch stacks). A fault on a stack Windows does not know may end the process before any handler runs. The header's reserve gives the same depth with no undocumented state.

### 3.5 Faults

- `process_init` calls `AddVectoredExceptionHandler(1, entry)`. `entry` is a small function the backend generates, like `_start`. It takes `EXCEPTION_POINTERS *` in `rcx`, moves it to the backend's first argument register, and calls `std.rt`'s `on_exception(pointers)`.
- `on_exception` reads the exception code (the first field of `EXCEPTION_RECORD`). For a fault code (access violation 0xC0000005, illegal instruction 0xC000001D, breakpoint 0x80000003, integer divide by zero 0xC0000094, stack overflow 0xC00000FD, and the other `STATUS_*` codes with severity "error"), it writes what stdout holds and a report naming the exception, as `on_fault` does for a signal. It then ends the process with `ExitProcess(code)`, so the exit status is the exception code, as Windows gives a crashed process.
- For any other code (a debugger's or another DLL's first-chance exception), it returns `EXCEPTION_CONTINUE_SEARCH` (0). The generated entry saves and restores `rsi` and `rdi` around the call, since Windows expects them kept. Oska uses no `XMM` register, so nothing else needs saving.
- `oskac` reports a native process that exits with an `NTSTATUS` error code (0xC0000000 and above) as `E0392`, as it reports a fault signal on Unix. `P0021` is unchanged: the generated code counts calls and panics before the stack runs out.

### 3.6 The PE writer (`pe.rs`)

- Sections: `.text` (read, execute), `.rdata` (read: the program's read-only data, then the import directory, lookup tables and hint/name table), `.data` (read, write: the IAT and the zero-filled writable data), `.reloc` (read).
- `ImageBase` 0x1_4000_0000 (the linker's default for a 64-bit executable), `SectionAlignment` 0x1000, `FileAlignment` 0x200, subsystem `WINDOWS_CUI` (3), OS and subsystem version 6.0.
- `Characteristics`: `EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE`. `DllCharacteristics`: `HIGH_ENTROPY_VA | DYNAMIC_BASE | NX_COMPAT | TERMINAL_SERVER_AWARE`. No section is both writable and executable.
- `TimeDateStamp` 0, `CheckSum` 0: the file depends on the program and the target alone.
- `.reloc` holds one block for the first page of `.text` with a single `IMAGE_REL_BASED_ABSOLUTE` (padding) entry: a valid relocation table with no fixup, so the loader may move the image (§2, **unverified**).
- The IAT is in `.data`, which is writable, because the loader writes it. A later ticket can add the load-config directory to make the IAT read-only after binding. **Open:** Control Flow Guard is not set, since it needs a table of every indirect-call target.
- Imports: kernel32 first, then ntdll, then bcryptprimitives. Each executable imports the platform's whole list in a fixed order, as on macOS, so the import table depends on the target alone.

## 4. Decisions for the user

**Q1. How Windows calls appear in `std.rt`.**
- (a) **Windows-named intrinsics, one per function, and two bodies per leaf in `std.rt`, chosen by `platform.windows`.** Keeps ADR 0015's one-call rule. About 20 leaves gain a Windows body. *Recommended.*
- (b) Keep the POSIX intrinsics and give each a Windows shim in the backend (an out-parameter slot for `WriteFile`, argument reordering for `VirtualAlloc`, a `GetStdHandle` for fd 1). Rejected by ADR 0015 ("no loops, no buffers and no decisions").
- (c) A separate `std.rt` source for Windows. Doubles the shared code above the leaves.

**Q2. Errors.** (a) **`-GetLastError()` and `-RtlNtStatusToDosError(status)`, so `std.rt` keeps one "negative is a failure" rule and words Win32 codes with `oska-sys`'s `WINDOWS_ERRORS`.** *Recommended.* (b) Separate NTSTATUS wording. Two tables for one platform.

**Q3. The stack.** (a) **The thread's own stack, sized by `SizeOfStackReserve`, with page probes for frames over 4 KiB.** *Recommended*: no reliance on undocumented TEB fields. (b) Map a stack as on Linux and update the TEB's stack bounds when switching. Writes undocumented structures.

**Q4. Faults.** (a) **A vectored exception handler through a generated entry; exit with the exception code; `oskac` reports codes ≥ 0xC0000000 as `E0392`.** *Recommended.* (b) `SetUnhandledExceptionFilter`. It runs only after Windows unwinds frames, which needs unwind data (`.pdata`) that Oska does not write.

**Q5. Random bytes.** (a) **`ProcessPrng` from `bcryptprimitives.dll`: a third DLL, which the spec allows "if kernel32 and ntdll offer none".** They don't document one. It cannot fail ("Always returns TRUE"), and Rust's standard library uses it (**unverified**: from memory of Rust's `library/std/src/sys/pal/windows/rand.rs`, not fetched). *Recommended.* (b) `RtlGenRandom` (advapi32 `SystemFunction036`): its page says it "may be altered or unavailable in subsequent versions". (c) `BCryptGenRandom(NULL, buf, n, BCRYPT_USE_SYSTEM_PREFERRED_RNG)` (bcrypt.dll): one call with constant arguments, but it gives an `NTSTATUS` that can fail.

**Q6. Console output.** (a) **`WriteConsoleW` with UTF-16 converted in Oska when stdout or stderr is a console, `WriteFile` with the exact bytes otherwise.** *Recommended*: correct text on any code page, and byte-exact output to files and pipes. (b) `SetConsoleOutputCP(65001)` at start. Changes the user's console for every process after the program ends.

**Q7. The record.** (a) **A new ADR 0020, "On Windows, a system-call intrinsic is one kernel32 or ntdll call", which amends ADR 0015 for Windows** (several libraries, `GetLastError` as a value, and the `windows` field). *Recommended*, as ADR 0015 amended ADR 0013. (b) Edit ADR 0015.

**Q8. How #176 is cut.** (a) **#176: the PE writer, the Windows `LibCall` convention, the Windows bodies for the standard streams, memory, arguments, exit and faults, and a `#[cfg(windows)]` test in `oskac` that runs small programs on `windows-latest` and compares them with the interpreter. #177: environment, stdin, clock, random, the console path, and the differential suite. #178: confinement.** *Recommended*: small programs need start-up, memory, stdout and exit, and nothing else works without them.

## 5. Risks

- **ASLR without fixups.** If the loader still refuses to move an image whose only relocation is padding, the image loads at its preferred base. The program still runs, but without randomization. A test on `windows-latest` that runs a program twice and compares a code address would catch it. That needs a way to print an address, so it may wait for #177.
- **Exceptions on the main stack.** A stack overflow leaves only the guaranteed stack for the handler. `on_exception` allocates nothing, as `on_fault` doesn't.
- **First-chance exceptions.** The vectored handler sees every exception in the process. Returning `EXCEPTION_CONTINUE_SEARCH` for anything but a fault keeps other DLLs working.
- **Console detection.** `GetConsoleMode` fails on a redirected handle, so a pipe or a file always takes the byte-exact path.
- **Windows Defender and SmartScreen** may scan or warn on an unsigned new executable. That is the developer's step (spec #155, Authenticode), and does not affect CI's runs.
