# Research: how `std.rt` calls into macOS's libSystem

- **Date:** 2026-11-17
- **Status:** accepted 2026-11-17: the user accepted all six recommended decisions (§4), ahead of #169 (spec #154). Nothing here is implemented. An independent review on 2026-11-17 corrected §2's `MH_NOUNDEFS` claim and settled some **unverified** claims against dyld `fd8d0c4`, Libc `71bbe35`, Linux `20eef29` and the `getrandom(2)` man page.
- **Question:** The review of PR #358 (#168) found that its `Platform` table can't express libSystem calls. What is the smallest shape that serves #169–#173 without changing what Linux executables do?
- **Method:**
  - Read the Oska sources at `origin/main` `5e335fb` (2026-11-17) and at PR #358's head `c4a5f89` (branch `spec/154-t168-std-rt-os-tables`, 2026-11-17, not merged). Claims about Oska cite the file and line at one of those commits.
  - Read the review comments of 2026-11-17 on #169, #170 and #171.
  - Fetched Apple's primary sources on 2026-11-17 and read them in full where cited:
    - [Writing ARM64 code for Apple platforms](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms) and [Writing 64-bit Intel code for Apple platforms](https://developer.apple.com/documentation/xcode/writing-64-bit-intel-code-for-apple-platforms), through their JSON form.
    - xnu headers at `apple-oss-distributions/xnu` `f6217f8` (`EXTERNAL_HEADERS/mach-o/loader.h`, `bsd/sys/{stat,dirent,signal,fcntl,mman,errno,termios,ttycom,random,cdefs}.h`, `bsd/sys/_types/_sigaltstack.h`).
    - dyld's `include/mach-o/fixup-chains.h` at `apple-oss-distributions/dyld` `fd8d0c4`.
    - Libc's `include/dirent.h` and `include/unistd.h` at `apple-oss-distributions/Libc` `71bbe35`.
    - Arm's AAPCS64 (`aapcs64/aapcs64.rst`) at `ARM-software/abi-aa` `9f9e067`.
  - No Mac was available, so nothing was run on macOS. Claims that no fetched source settles are marked **unverified**.

## Summary

The review is right: a call through libSystem is not a system call with different numbers. It differs in five ways, and #169 already meets all five, because even "small programs" go through `process_init`, `stdout_buffer` and `exit`. Those use `mmap`, `sigaction`, `sigaltstack`, `ioctl` (variadic) and `_exit`.

| | Linux system call | libSystem function |
| --- | --- | --- |
| Named by | number in `x8` / `rax` | symbol, bound by dyld at load |
| Error | result in `-4095..-1` | `-1` (or `NULL`), then `*__error()` |
| Result width | 64 bits, sign-extended | `int` results leave the top 32 bits unspecified |
| Variadic arguments | ordinary registers | on the stack (Apple arm64); `al` set (x86_64) |
| Registers clobbered | `x0` (arm64); `rcx`, `r11` (x86_64) | every caller-saved register |

The design keeps one rule: **a system-call intrinsic is one call of the operating system**. On macOS that is one libSystem function. The backend normalizes the result to the convention `std.rt` already uses, a negative `errno` on failure (`rt.osk:14-15` at `c4a5f89`). So:

1. **Naming.** `Platform.syscall` and `Platform.then` become one `Platform.call: fn(Sys) -> OsCall`. `OsCall` is either `Trap` (Linux: number and constant trailing arguments, exactly today's data) or `Library` (macOS: symbol, fixed-argument count, result width, failure convention and constant trailing arguments).
2. **Imports.** Every `Library` call loads its target from one `__got` slot in `__DATA_CONST`. dyld binds the slots through `LC_DYLD_CHAINED_FIXUPS` at load. There are no stubs and no lazy binding. Every macOS executable imports the same fixed list, all from `/usr/lib/libSystem.B.dylib`.
3. **Calling convention.** A new IR instruction, `Inst::LibCall`, is a C call through the GOT slot. Each code generator places variadic arguments as its platform requires, and sign-extends `int` results. Oska's register model already treats every caller-saved register as dead across a `Syscall`, so register allocation does not change.
4. **errno.** `lower.rs` expands a failing `Library` call into a `___error` call and a 32-bit load, then negates. `std.rt`'s error handling does not change.
5. **The `std.rt` table** gains the values macOS needs, and four intrinsics change their contracts in ways Linux already meets.
6. **ADR 0013** gets a sibling, [ADR 0015](../adr/0015-macos-intrinsics-are-libsystem-calls.md) (Accepted). It defines "one call of the operating system" for libSystem and says what the backend may add around that call.

Linux keeps every system call it makes today, with the same arguments. Its `Platform` data moves into the new type with the same values.

## 1. What exists today

All findings below are from 2026-11-17.

- **Lowering.** `Runtime::Syscall(call)` becomes `Inst::Syscall { number: (platform.syscall)(call), args ++ (platform.then)(call) }` (`lower.rs` hunk at `c4a5f89`, around line 1725).
  - The aarch64 code generator moves the arguments, then emits `movz x8, #n; svc #0` (`aarch64.rs:549-554` at `5e335fb`).
  - x86_64 emits a parallel move into `rdi, rsi, rdx, r10, r8, r9`, then `mov rax, n; syscall` (`x86_64.rs:489-499`).
- **The IR's contract** for `Inst::Syscall` is "the kernel's raw result, negative `errno` on failure" (`ir.rs:252-259`). `std.rt` relies on it everywhere, for example `if got == -4 { continue }` in `list_dir` (`rt.osk:3659` at `c4a5f89`).
- **Register model.** Register allocation marks every live range that crosses a `Call`, `CallIndirect` or `Syscall` (`regalloc.rs:247-258`). Only callee-saved registers hold temporaries across one:
  - aarch64: `x19`–`x27`, with `x28` as the call-depth counter (`aarch64.rs:18-36`).
  - x86_64: `rbx` and `r12`–`r14`, with `r15` as the depth counter (`x86_64.rs:16-34`).

  These are all callee-saved in AAPCS64 and in the System V psABI too. Neither code generator ever allocates `x18`: it is in neither register list (`aarch64.rs:52-60`).
- **Frames and stack.** Frames are a multiple of 16 bytes, and the frame pointer always addresses the frame record (`aarch64.rs:37-39`, `x86_64.rs:35-38`). Programs run on a stack `std.rt` maps, with 8 MiB beyond the deepest counted frame for `std.rt`'s own calls (`lower.rs:93`, `STACK_RESERVE`).
- **The `Platform` type at `c4a5f89`.** It is `{ table, syscall: fn(Sys) -> u16, then: fn(Sys) -> &'static [i64] }` (`intrinsics.rs:221-231`). `linux_then` appends `getrandom`'s flags and `rt_sigaction`'s old-action and mask-size arguments (`intrinsics.rs:243-249`).
- **Entry.** `_start` reads the kernel's initial stack pointer (`Inst::EntrySp`). It passes the pointer to `process_init(sp, handler)`, which reads `argc` at `sp` and `argv` at `sp + 8` (`lower.rs:372-417`; `rt.osk:1848-1856` at `c4a5f89`).

## 2. What libSystem requires (primary sources)

### Calling convention, aarch64 (Apple ABI page, fetched 2026-11-17)

- "The platforms reserve register `x18`. Don't use this register." Oska already doesn't.
- "The frame pointer register (`x29`) must always address a valid frame record." Oska already does.
- The stack pointer must be 16-byte aligned.
- **Variadic arguments:** "Assign the variadic argument to the appropriate number of 8-byte stack slots." Fixed arguments still go in `x0`–`x7`. A variadic argument goes on the stack even when registers are free, so `openat`'s `mode`, `fcntl`'s third argument and `ioctl`'s third argument are read from `[sp]` at the call.
- "The caller of a function is responsible for signing or zero-extending any argument with fewer than 32 bits." Oska passes whole 64-bit words, so this holds already.
- **Results:** Apple's page does not change AAPCS64 here. AAPCS64 returns a result "in the same registers as would be used for such an argument" (§ Result return), and "when an argument is assigned to a register any unused bits in the register have unspecified value". So a function returning `int` (`openat`, `close`, `fstat`, …) leaves bits 32–63 of `x0` unspecified. The caller must sign-extend from 32 bits. Linux's `svc` returns a full 64-bit value, so this is new.

### Calling convention, x86_64 (Apple ABI page, fetched 2026-11-17)

- Register usage "follows the standard psABI". Apple's divergences cover small-integer promotion, vectors and Swift, none of which Oska passes.
- **Variadic arguments:** the psABI passes variadic arguments like fixed ones and requires `al` to hold an upper bound on the vector registers used. Setting `eax` to 0 before every call is correct, because Oska passes no floating-point arguments. **Unverified** against the psABI text itself, which was not fetched. This is the rule clang and every psABI implementation follow.
- `int` results arrive in `eax`, and the top half of `rax` is unspecified (psABI; **unverified** text, same reason). Sign-extend with `movsxd`.

### Symbol names

- Mach-O C symbols carry a leading `_`, so `__error` is `___error` and `_exit` is `__exit`.
- **On macOS x86_64 only**, functions that take `struct stat` or `struct dirent` have a `$INODE64` suffix: `_fstat$INODE64`, `_fdopendir$INODE64`, `_readdir$INODE64`.
  - xnu's `cdefs.h` sets `__DARWIN_ONLY_64_BIT_INO_T` to 0 on x86_64 and to 1 on arm64.
  - `__DARWIN_INODE64(sym)` appends `__DARWIN_SUF_64_BIT_INO_T`, which is `"$INODE64"` only when that is 0.
  - `fstat` is declared `__DARWIN_INODE64(fstat)` (`stat.h:563`). In Libc's `dirent.h`, `readdir` is `__DARWIN_INODE64(readdir)` (`:129`) and `fdopendir` is `__DARWIN_ALIAS_I(fdopendir)` (`:177`).
  - The `$UNIX2003` suffix is empty on both processors, since `__DARWIN_ONLY_UNIX_CONFORMANCE` is 1 for x86_64 and arm64.
- So the two macOS processors share one `std.rt` table, but need two `Platform`s in the backend, because three symbol names differ.

### Values and layouts (xnu `f6217f8`, Libc `71bbe35`)

| Field | linux-aarch64 | macOS (both) | Source |
| --- | --- | --- | --- |
| `o_directory`, `o_nofollow` | 16384, 32768 | 1048576, 256 | `fcntl.h:156,125` |
| `o_wronly`, `o_creat`, `o_nonblock`, `o_cloexec` | 1, 64, 2048, 524288 | 1, 512, 4, 16777216 | `fcntl.h:99,127,115,171` |
| `at_fdcwd` | -100 | -2 | `fcntl.h:203` |
| `f_dupfd_cloexec` | 1030 | 67 | `fcntl.h:397` |
| `tty_request` | 21505 (`TCGETS`) | 1078490131 (`TIOCGETA` = `_IOR('t', 19, struct termios)`, 72-byte termios) | `ttycom.h:108`, `termios.h:263-274` |
| `map_private_anonymous`, `map_noreserve` | 34, 16384 | 4098 (`MAP_PRIVATE`\|`MAP_ANON`), 64 | `mman.h:111,133,122` |
| `struct stat`: bytes, `st_mode`, `st_nlink`, `st_size` | 128, 16 (u32), 20 (u32), 48 | 144, 4 (**u16**), 6 (**u16**), 96 | `stat.h:166-182` |
| dirent: `d_reclen`, `d_name` | 16, 19 | 16, 21 | `dirent.h:102-109` |
| `struct sigaction`: bytes, `sa_flags` | 32, 8 | 16, 12 (handler, `sigset_t` u32 mask, `int` flags) | `signal.h:385-389` |
| fault flags | 3422552064 (`0xCC000000`, with `SA_RESTORER`) | 21 (`SA_ONSTACK`\|`SA_RESETHAND`\|`SA_NODEFER`; no restorer) | `signal.h:462-469` |
| `stack_t`: `ss_size` | 16 | 8 | `_sigaltstack.h` |
| `SIGBUS` | 7 | 10 | `signal.h:98` |
| `ELOOP` | 40 | 62 | `errno.h:182` |

The other signals `std.rt` names (4, 5, 6, 8, 11, 13) and the other `errno` values it tests for (2, 4, 20, 22) are the same on macOS (`signal.h:87-101`, `errno.h`). `MAP_FIXED` is 16 on both.

Linux's `struct sigaction` (32 bytes) is the kernel's `rt_sigaction` layout. macOS's `sigaction()` takes the 16-byte user `struct sigaction` and builds the kernel's `__sigaction`, with its trampoline, itself (`signal.h:375-389`).

Three defects in `std.rt` would surface on macOS (found 2026-11-17, at `c4a5f89`):

- `stdout_buffer` gives `TCGETS` a 56-byte buffer (`alloc(64)` minus the count word, `rt.osk:2060-2061`). That is enough for Linux's kernel termios (36 bytes: four `unsigned int` flags, `c_line` and `c_cc[19]`, `include/uapi/asm-generic/termbits.h` at Linux `20eef29`; neither arm64 nor x86 overrides it), but macOS's `TIOCGETA` writes 72 bytes. That would corrupt the heap.
- `lone_file` reads `st_nlink` with `rt_mem_load32` (`rt.osk:3493`). On macOS that is a u16 followed by the low bytes of `st_ino`, so every file would look hard-linked (the #171 finding).
- `sigbus()` returns 7 (`rt.osk:1893`), and the fault report hard-codes `signal 7 (SIGBUS)` (`rt.osk:1987`).

### Mach-O imports (loader.h, fixup-chains.h)

- `LC_LOAD_DYLINKER` (`0xe`) names `/usr/lib/dyld`. `LC_LOAD_DYLIB` (`0xc`) names `/usr/lib/libSystem.B.dylib`, which becomes library ordinal 1. `LC_MAIN` (`0x28|LC_REQ_DYLD`) is an `entry_point_command { entryoff, stacksize }` (`loader.h:1530-1535`).
- `LC_DYLD_CHAINED_FIXUPS` (`0x34|LC_REQ_DYLD`) points into `__LINKEDIT` at a `dyld_chained_fixups_header` (`fixup-chains.h:37-46`). Behind the header come the starts table, the imports table and the symbol strings.
- With `imports_format = DYLD_CHAINED_IMPORT` (1), each import is 4 bytes: `lib_ordinal:8, weak_import:1, name_offset:23` (`fixup-chains.h:301-312`).
- With `pointer_format = DYLD_CHAINED_PTR_64` (2), each pointer slot holds a `dyld_chained_ptr_64_bind { ordinal:24, addend:8, reserved:19, next:12, bind:1 }` (`fixup-chains.h:219-226`). `next` counts 4-byte strides, so consecutive 8-byte slots chain with `next = 2` and the last has `next = 0`.
- `SG_READ_ONLY` (`0x10`) marks a segment "made read-only after fixups" (`loader.h:404`). That is how `__DATA_CONST` keeps the bound pointers from being overwritten.
- The header flags are `MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE` (`0x00200085`), as ld64 writes them for an ordinary executable (`loader.h:126-190`). `MH_NOUNDEFS` refers to static linking: every symbol is bound to a dylib, so it is set even though the file has imports. dyld `fd8d0c4` does not check the bit, so this is about matching ld64, not about loading. (Corrected in review on 2026-11-17. The draft said to leave the bit out.)
- **What dyld `fd8d0c4` checks** (read in review, 2026-11-17, not run). Its enforcement epoch comes from the SDK version the file declares (`mach_o/Policy.cpp:40-44`):
  - It needs a **platform load command**. `validMachOForArchAndPlatform` refuses a file that was not built for the process's platform (`common/MachOAnalyzer.cpp:293-296`, `mach_o/UnsafeHeader.cpp:1209-1225`). So the file needs `LC_BUILD_VERSION` with platform macOS. The draft did not list it.
  - It needs **at least one `LC_LOAD_DYLIB`**, from the spring 2025 SDK epoch (`UnsafeHeader.cpp:749-757`, `Policy.cpp:440-443`). The design already has libSystem.
  - It needs **`LC_UUID` in a main executable** once the declared SDK is new enough: the fall 2025 epoch in `Policy::enforceHasUUID` (`Policy.cpp:450-461`), or macOS SDK 14 in `MachOFile::enforceFormat` (`common/MachOFile.cpp:2700-2703`). Emit one, computed from the file's other bytes so builds stay identical on every host.
  - **Chained fixups.** ld64 uses them by default from deployment target macOS 12 on arm64 and macOS 13 for x86_64 executables (`Policy.cpp:142-170`). `LC_BUILD_VERSION`'s `minos` should be at least that.
- **Still unverified:** whether dyld needs `LC_SYMTAB`, `LC_DYSYMTAB` or `LC_DYLD_EXPORTS_TRIE` in a main executable. ld64 emits all three. #169's first CI run settles it, and the answer doesn't change the import design.

## 3. The design

### 3.1 How a call site names a libSystem function

`std.rt` does not change how it calls: it still writes `rt_sys_fstat(fd, buf)`. The backend's `Platform` says what that call is on the target:

```rust
pub struct Platform {
    pub table: &'static str,
    /// How the operating system makes each intrinsic's call.
    pub call: fn(Sys) -> OsCall,
}

pub enum OsCall {
    /// A system call: its number, and constant arguments after the
    /// intrinsic's own (Linux; today's `syscall` and `then`).
    Trap { number: u16, then: &'static [i64] },
    /// A function of the one library the platform links (libSystem).
    Library(&'static Import),
    /// No call: the result is the intrinsic's first argument
    /// (Linux's `rt_sys_open_dir`, see 3.5).
    Same,
}

pub struct Import {
    pub symbol: &'static str,   // "_fstat", or "_fstat$INODE64" on x86_64
    pub fixed: u8,              // arguments before `...`; all, if not variadic
    pub result: Result32or64,   // `int` results are sign-extended from 32 bits
    pub fails: Fails,           // MinusOne | Null | NullKeepsErrno | Never
    pub then: &'static [i64],   // e.g. sigaction's old action: [0]
}
```

`LINUX_AARCH64` and `LINUX_X86_64` give `Trap` for every call except `Sys::OpenDir`, with the numbers and `then` arguments they give today. `MACOS_AARCH64` and `MACOS_X86_64` give `Library` rows. Both use one `std.rt` table, `macos`, and they differ only in the three `$INODE64` symbols.

The macOS rows (symbol for arm64 / `fixed` / result / failure):

| `Sys` | Function | Notes |
| --- | --- | --- |
| Read, Write | `_read`, `_write` / 3 / 64 / −1 | `ssize_t` |
| Openat | `_openat` / **3 of 4** / 32 / −1 | variadic `mode` |
| Close, Mkdirat, Ftruncate | `_close`, `_mkdirat`, `_ftruncate` / all / 32 / −1 | |
| Fstat | `_fstat` (`$INODE64` on x86_64) / 2 / 32 / −1 | |
| Readlinkat | `_readlinkat` / 4 / 64 / −1 | |
| Getcwd | `_getcwd` / 2 / 64 / `NULL` | contract change, 3.5 |
| Fcntl, Ioctl | `_fcntl`, `_ioctl` / **2 of 3** / 32 / −1 | variadic third argument |
| Mmap, Munmap | `_mmap` / 6 / 64 / −1 (`MAP_FAILED`); `_munmap` / 2 / 32 / −1 | |
| ClockGettime | `_clock_gettime` / 2 / 32 / −1 | `CLOCK_REALTIME` is 0 on macOS (Libc `71bbe35` `include/_time.h:211`, checked in review) |
| Random | `_getentropy` / 2 / 32 / −1 | at most 256 bytes; `std.rt` asks for 8 |
| Sigaction | `_sigaction` / 3 / 32 / −1, `then: [0]` | old action |
| Sigaltstack, Kill | `_sigaltstack`, `_kill` / 2 / 32 / −1 | |
| Getpid | `_getpid` / 0 / 32 / never | |
| Exit | `__exit` / 1 / — / never | `_exit`, not `exit`: no `atexit` handlers, like `exit_group` |
| OpenDir (new) | `_fdopendir` (`$INODE64`) / 1 / 64 / `NULL` | 3.5 |
| ReadDir | `_readdir` (`$INODE64`) / 1 / 64 / `NULL`, keeps `errno` | 3.5 |
| CloseDir (new) | `_closedir` / 1 / 32 / −1 | |

The backend also imports `___error`, which no intrinsic names. That makes 24 imports.

### 3.2 How the Mach-O writer emits imports

- **One GOT, no stubs.** The writer puts a `__DATA_CONST` segment (`SG_READ_ONLY`) between `__TEXT` and `__DATA`. It holds one section, `__got` (`S_NON_LAZY_SYMBOL_POINTERS`), with one 8-byte slot per import, in the order of the platform's import list.
  - Each slot's file bytes are a `dyld_chained_ptr_64_bind` with `ordinal` = its index, `next` = 2, or 0 for the last slot.
  - `__LINKEDIT` holds the fixups blob: header, starts (only `__DATA_CONST` has a chain; its `page_start[0]` is the GOT's offset in its page), imports (`lib_ordinal` 1) and the names.
- **Code reaches a slot as it reaches data.** `ir::Symbol` gains `Import(u16)`, and `target::Layout` gains `got`. `Layout::address(Symbol::Import(i))` is `got + 8 * i`.
  - aarch64 emits `adrp x16, slot; add x16, x16, :lo12:slot; ldr x16, [x16]; blr x16`. The first two instructions are exactly today's `Fixup` (`aarch64.rs:89-102`), so `Text::link` does not change.
  - x86_64 emits `lea r11, [rip + slot]` with today's displacement fixup, then `call [r11]`.
  - `x16` and `r11` are already scratch registers in each convention.
- **The same list every time.** Every macOS executable imports all 24 functions, whether or not the program reaches them. That makes the bytes a function of the target alone, which is easy to golden-test on the Pi, and keeps the list reviewable. The cost is about 24 × (8 + 4 + ~12) bytes. See decision Q4.
- **No other library.** `lib_ordinal` is always 1. A format test can assert that the file has exactly one `LC_LOAD_DYLIB` and that every import's ordinal is 1 (spec story 11).

ELF never sees `Symbol::Import`, because no Linux `Platform` has a `Library` call. `elf.rs` does not change.

### 3.3 The calling convention, variadics included

There is one new IR instruction:

```rust
/// `dst =` the library function in GOT slot `import`, called with `args`
/// by the platform's C convention: the first `fixed` in registers, the
/// rest as variadic arguments; an `int` result sign-extended.
LibCall { dst: Temp, import: u16, args: Vec<Temp>, fixed: u8, wide: bool },
```

`regalloc.rs` counts it with `Call`, `CallIndirect` and `Syscall` as a call that live ranges cross. That is a one-word change, and it is correct, because the C convention preserves exactly the registers Oska already keeps temporaries in across a `Syscall` (§1).

**aarch64.** The code generator:

1. Moves `args[..fixed]` into `x0`… with the existing parallel move.
2. If there is a variadic argument, reads it into `x9` and pushes it with `str x9, [sp, #-16]!`. That is one 16-byte-aligned slot; every variadic call in the table has exactly one variadic argument. The push comes after the parallel move, so stack-slot reads made by the move still use the frame's offsets.
3. Emits the four-instruction GOT call.
4. Pops with `add sp, sp, #16`.
5. Emits `sxtw x0, w0` if `!wide`, and writes `dst` from `x0`.

Placing the variadic argument in a register as well would be harmless, but the ABI does not read it there, so the design doesn't.

**x86_64.** The code generator does the parallel move into `rdi, rsi, rdx, rcx, r8, r9`. Fixed and variadic arguments go in the same registers. Then it emits `xor eax, eax` before every library call: the vector-register count for variadics, and harmless otherwise. Then the GOT call, then `movsxd rax, eax` if `!wide`.

Stack alignment holds without new work: frames are 16-byte multiples, so `sp` is 16-aligned at every call on both processors (§1). On x86_64 that depends on how `_start` is entered. `LC_MAIN` enters it by a C call, so `rsp` is 8 mod 16, as at any function's entry, and `_start`'s calls are aligned. Linux's kernel entry is 0 mod 16, so `_start`'s calls on Linux are 8 off, which no system call notices. (Derived from `x86_64.rs`'s frame rule, not run on macOS.)

**Entry.** dyld calls `LC_MAIN`'s entry as `main(argc, argv, envp, apple)`, by the C convention, on the main thread's stack. `_start` never returns, so clobbering callee-saved registers there is fine. It calls `_exit` through `std.rt`'s `exit`. What changes is how `_start` finds `argc` and `argv` (3.5).

**Signal handler.** The handler macOS installs, through `sigaction()`'s trampoline, is called with the signal number as its first C argument. That is `x0` or `rdi`, the register where Oska's convention passes `on_fault`'s one argument (`aarch64.rs:10-14`). **Unverified** beyond the `__sigaction` declaration (`signal.h:375-380`); #170's `E0392` tests settle it.

**Stack depth.** libSystem calls run on `std.rt`'s mapped stack, within the existing 8 MiB `STACK_RESERVE`. No libSystem function `std.rt` calls is recursive. A fault inside one is a `SIGSEGV`/`SIGBUS`, reported as `E0392`, as on Linux.

### 3.4 errno

`lower.rs` lowers `Runtime::Syscall(call)` as follows:

- `Trap`: as today, `Inst::Syscall`.
- `Library` with `fails: MinusOne`:
  ```
  r = LibCall(import, args ++ then)
  if r != -1 goto done
  e = LibCall(___error)          // int *
  r = 0 - Load32(e)
  done: dst = r
  ```
- `Null` is the same with `r != 0`.
- `NullKeepsErrno` (`readdir`) first stores 0 through `___error()`. After the call, a `NULL` with `errno` still 0 is the end of the directory, and the result is 0.
- `Never` adds nothing.

Every failing call reaches `std.rt` as `-errno`, which is what `ir.rs:252-259` promises today. Nothing in `std.rt`'s error paths changes. `errno` is read before any other call, because the expansion has none in between. On-fault handlers never return, so they cannot clobber it either.

The errno *values* that differ, `ELOOP` and anything the #167 `IoError` kind table keys on, go in the `std.rt` table, as the other numbers did in #168.

### 3.5 How the `Platform` table changes

**In the backend (`intrinsics.rs`).** `syscall` and `then` merge into `call: fn(Sys) -> OsCall` (3.1). `Sys` gains `OpenDir` and `CloseDir`. For Linux:

- `OpenDir` is `Same`: the directory handle is the file descriptor.
- `CloseDir` is `close`.

**In `std.rt`'s `Platform` struct**, the fields #168 left out, which macOS needs:

- `sigbus` (7 / 10). The fault report builds its `SIGBUS` line from it.
- `e_loop` (40 / 62).
- `termios_bytes` (36 / 72; Linux's is the kernel's `struct termios` that `TCGETS` fills, §2), the size of the buffer `stdout_buffer` gives the terminal `ioctl`. Allocating the larger size everywhere is the alternative.
- `stat_mode_bits` and `stat_nlink_bits` (32/32 on Linux, 16/16 on macOS), read through a `load_bits(at, bits)` helper.
- `dirent_one_at_a_time` (false / true). On Linux, `rt_sys_read_dir`'s positive result is the number of record bytes it put at `buf`. On macOS it is the address of one record, whose length is at `dirent_length`. `list_dir` takes `(start, end)` from this flag and walks records as it does now.

**Contracts that change, each one Linux already meets:**

| Intrinsic | Today | New contract | Linux | macOS |
| --- | --- | --- | --- | --- |
| `rt_sys_getcwd` | length incl. NUL | ≥ 0: the path is NUL-terminated at `buf` | `getcwd` syscall, unchanged; `std.rt` measures the string | `getcwd` returns `buf` |
| `rt_sys_random` | bytes filled | ≥ 0: all `length` (≤ 256) bytes filled | `getrandom(buf, 8, 0)` returns 8 | `getentropy` returns 0 |
| `rt_sys_read_dir` | bytes at `buf` | > 0 as `dirent_one_at_a_time` says | `getdents64(fd, …)` | `readdir(dir)` |
| `rt_sys_open_dir(fd)` / `rt_sys_close_dir(dir)` | (new) | a directory handle that owns `fd` / release it | `Same` / `close` | `fdopendir` / `closedir` |
| `process_init(sp, handler)` | reads `argc`/`argv` at `sp` | `process_init(argc, argv, handler)` | `_start` loads them from `EntrySp` | `_start` takes `x0`/`x1` (`rdi`/`rsi`) |

- **`getcwd`.** `working_directory` (`rt.osk:3218-3236`) already checks the first byte. It would find the length by scanning to the NUL, which is ≤ 4096 bytes into a buffer it owns.
- **`random`.** `random_next_int` would test `got < 0` instead of `got != 8` (`rt.osk:2534`). Linux's `getrandom` fills requests of ≤ 256 bytes completely once the pool is ready. The man page says it "will return all of the bytes that have been requested" (`getrandom(2)`, man7.org, checked in review).
- **Why `readdir`.**
  - `getdirentries` is unavailable with 64-bit inodes, and `__getdirentries64` is a private symbol.
  - `readdir_r` is deprecated in Libc's `dirent.h:130`.
  - `getattrlistbulk` is public and fd-based, but its records hold the name through an `attrreference_t` offset. The record walk would then differ by OS, not just its offsets. See decision Q3.

Linux executables' **system calls** are unchanged, but their **bytes** change, because `std.rt`'s code changes (`list_dir`, `working_directory`, `random_next_int`, `process_init`, `stdout_buffer`). The differential suite and the cross-build identity job are the check. A golden test of every Linux `OsCall::Trap` row against today's numbers and `then` arguments shows that the reshape alone moves nothing.

### 3.6 What ADR 0013 needs to say

ADR 0013 says intrinsics are lowered "directly to machine code". It also rejects "system-call wrappers [that] live inside the code generator", because they hide the surface in Rust. libSystem forces a little code around each call: the GOT load, variadic placement, sign extension and the `errno` read. Without a rule, that could grow into the wrappers 0013 rejected. [ADR 0015](../adr/0015-macos-intrinsics-are-libsystem-calls.md) (Accepted) states the rule:

- A system-call intrinsic is **one call of the operating system's interface**: a system call by number (Linux), or one function of the one library the platform requires (libSystem on macOS; kernel32/ntdll on Windows, later).
- Around that call the backend may add only:
  - the platform's calling convention;
  - the conversion of failure to `-errno`, the one result convention `std.rt` sees;
  - constant trailing arguments;
  - "no call" for an intrinsic the OS doesn't need.

  It never adds loops, buffers or decisions. Anything more is Oska code in `std.rt`.
- A macOS executable imports from `/usr/lib/libSystem.B.dylib` only, and only the functions in its `Platform`'s import list. The list is part of the reviewed surface, like `RUNTIME`.
- ADR 0013's checker and guard rules are unchanged.

## 4. Decisions for the user

Each question has options and a recommendation, most important first.

**Q1. Who turns a libSystem failure into `-errno`?**
- (a) The backend, in `lower.rs`, as in 3.4. **Recommended.** `std.rt` is unchanged, the IR keeps its contract, and Linux is untouched.
- (b) A new `rt_sys_errno()` intrinsic, with each call site in `std.rt` testing `-1`. This makes every error path OS-dependent.
- (c) Raw results plus a per-OS "error shape" field in the table. This is (b) with indirection.

**Q2. When does the `Platform` reshape (3.1, 3.5) land?**
- (a) As its own Linux-only PR before #169: `OsCall`, `LibCall` and `Symbol::Import` unused, the `std.rt` contract changes, and the golden test that Linux's system calls are unchanged. **Recommended.** #169 then is only Mach-O, signing and the macOS rows, and a regression in the reshape shows up on Linux, where it can run.
- (b) Inside #169. One fewer PR, but #169 grows by a third, and its only runtime check is the macOS CI runner.
- (c) Folded into #358 before it merges. This reopens a reviewed PR.

**Q3. How does macOS list a directory?**
- (a) `fdopendir`/`readdir`/`closedir`, with `rt_sys_open_dir`/`rt_sys_close_dir` and the `dirent_one_at_a_time` table field. **Recommended.** These are public, stable and what Rust's std uses (**unverified**: from memory of Rust's `std::sys::fs::unix`, not re-read).
- (b) `getattrlistbulk`. One call, fd-based and batched like `getdents64`, but the record walk differs by OS.
- (c) The private `__getdirentries64`, which matches Linux's shape but is not API.

**Q4. Which functions does a macOS executable import?**
- (a) The platform's whole list, always. **Recommended:** the bytes depend only on the target, and the list is fixed and reviewable.
- (b) Only those the program's code reaches. Smaller and still deterministic, but the import list varies per program, and the format tests need a program per case.

**Q5. Does `std.rt` change Linux code paths to get one contract (`getcwd`, `random`, `read_dir`, `process_init`)?**
- (a) Yes, as 3.5. **Recommended:** the code stays one path per operation. Linux's system calls are unchanged, but its executables' bytes change once.
- (b) No: the backend adapts macOS results to Linux's (`strlen` after `getcwd`, a copy loop after `readdir`). This keeps Linux's bytes, but it is the "wrappers in the code generator" ADR 0013 rejected.

**Q6. Record the rule as a new ADR or as an edit to ADR 0013?**
- (a) A new ADR 0015 that amends 0013 (drafted here, now Accepted), with a one-line pointer added to 0013 when it is accepted. **Recommended:** it keeps 0013's history and matches how 0014 amended 0011.
- (b) Rewrite 0013's Decision section in place.

## 5. Risks

- **dyld's minimum load commands are only partly settled** (§2). dyld's source requires `LC_BUILD_VERSION`, `LC_LOAD_DYLIB`, and `LC_UUID` once the declared SDK is recent enough. `LC_SYMTAB`, `LC_DYSYMTAB` and `LC_DYLD_EXPORTS_TRIE` are still unverified; if dyld refuses a file without them, #169 adds empty ones. That costs bytes, not design.
- **The x86_64 psABI claims were not re-read** at the source (§2): `al` bounds the vector registers, and an `int` result leaves the top of `rax` unspecified. Both are long-standing and matter only from #173.
- **`$INODE64` is easy to get wrong silently.** On x86_64, a call to `_fstat` without the suffix links and runs, but fills the old 32-bit-inode `struct stat`, so every offset in the table is wrong. A format test should assert the three suffixed names on `macos-x86_64`, and #173's differential suite on `macos-15-intel` covers the behaviour.
- **Sign extension is easy to miss.** An `int` result used unextended can look right on the CI runner and break under another libSystem build. The `wide` flag is per import, so a test can check that every `int`-returning row says `false`.
- **Linux executables' bytes change once** (Q5a), without behaviour changes. Any test that pins a Linux executable's hash will need updating in the same PR.
- **The `std.rt` table grows by six fields.** Each one is data, but `dirent_one_at_a_time` is a shape flag that `list_dir` branches on. That is the first table field that is a choice, not a number. ADR 0015 allows it because the branch is on data the table owns, not on which platform is running. Reviewers may prefer Q3(b) or a per-OS function to avoid it.
- **The environment's address is unverified on macOS.** `process_init(argc, argv, handler)` assumes, as on Linux, that `envp` starts after `argv`'s `NULL`. dyld also passes `envp` as `main`'s third argument (`x2`/`rdx`). Passing that explicitly is safer if #170 finds the layout differs.
- **Nothing here has run on a Mac.** The first `macos-latest` CI run in #169 is the first real test of the import path.
