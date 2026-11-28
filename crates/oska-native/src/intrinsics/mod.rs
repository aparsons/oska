// SPDX-License-Identifier: Apache-2.0
//! How the native backend implements body-less functions: the prelude's
//! intrinsics, which user code calls, and `std.rt`'s system-call and
//! memory intrinsics, which only the runtime calls.
//!
//! A prelude intrinsic is implemented in Oska, by a function of `std.rt`
//! that takes the same arguments (the receiver first), or, when that would
//! be a single instruction, inline. A `std.rt` intrinsic is one call of
//! the operating system (on Linux, a system call, or none where Linux
//! needs none), one word operation, or a constant. What differs between target platforms is in one
//! [`Platform`] per target: how its operating system is called here, and
//! its numbers, flags and structure layouts in its table in `std.rt`.
//!
//! To add a prelude intrinsic: write its runtime function in
//! `oska-check/src/std/rt.osk` and add a [`Prelude::Runtime`] row to
//! [`PRELUDE`]. To add a `std.rt` intrinsic: declare it body-less in
//! `rt.osk` and add its row to [`RUNTIME`]; a test requires one for every
//! declaration.

mod linux;
mod macos;
mod prelude;
mod runtime;
#[cfg(test)]
mod tests;
mod windows;

use crate::ir::{BinOp, Width};

pub use linux::{LINUX_AARCH64, LINUX_X86_64};
pub use macos::{MACOS_AARCH64, MACOS_X86_64};
pub use prelude::{prelude, unqualified, Check, Prelude, SECRET_BYTES};
pub use runtime::runtime;
pub use windows::{NT_STATUS_TO_ERROR, WINDOWS_X86_64};

/// How a `std.rt` intrinsic is lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// The operating-system call `Sys`, as the target platform makes it
    /// ([`Platform`]).
    Syscall(Sys),
    /// `load(addr)`.
    Load(Width),
    /// `store(addr, value)`, whose result is `()`.
    Store(Width),
    /// `op(a, b)`.
    Binary(BinOp),
    /// Its one argument, whose word is already what it asks for, such as
    /// a text value's object address.
    Identity,
    /// The address of the runtime's writable data.
    Globals,
    /// How many more calls may nest (`Inst::Depth`).
    Depth,
    /// Which host the process runs on: always 0, the real OS, in a
    /// program's executable; in a test executable, what its entry set
    /// (`lower/tests.rs`).
    TestHost,
    /// `std.rt`'s table of what differs between target platforms, the
    /// target platform's ([`Platform::table`]).
    PlatformTable,
    /// 1 when the executable was built with the DNS test hooks (a debug
    /// build of Oska built it), else 0: a constant of the build.
    DnsTestHooks,
    /// Its one argument, marked secret (`Inst::Secret`): a byte read from
    /// secret bytes, which no branch or address may then depend on
    /// (`constant_time.rs`).
    Conceal,
    /// Its one argument, made public on purpose (`Inst::Expose`): the
    /// result of a constant-time comparison.
    Expose,
}

/// An operating-system call, which the `std.rt` intrinsic of its name
/// makes. Most are the POSIX calls of their names; the directory calls
/// ([`Sys::OpenDir`], [`Sys::ReadDir`], [`Sys::CloseDir`]),
/// [`Sys::Random`], [`Sys::Sigaction`] and [`Sys::Exit`] are named for
/// what they do, since Linux's calls for them (`getdents64`, `getrandom`,
/// `rt_sigaction`, `exit_group`) are Linux's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sys {
    Read,
    Write,
    Openat,
    Close,
    Mkdirat,
    OpenDir,
    ReadDir,
    CloseDir,
    Fstat,
    Ftruncate,
    Readlinkat,
    Unlinkat,
    Renameat,
    Getcwd,
    Fcntl,
    Mmap,
    Munmap,
    ClockGettime,
    Ioctl,
    Random,
    Sigaction,
    Sigaltstack,
    Getpid,
    Kill,
    Exit,
    // The sockets of spec #228: the BSD calls of their names, and `poll`,
    // which is Linux's `ppoll` there. Linux's `Accept` is `accept4`,
    // `Send` is `sendto` and `Recv` is `recvfrom`, with the arguments
    // those take beyond the neutral ones.
    Socket,
    Connect,
    Bind,
    Listen,
    Accept,
    Getsockname,
    Getsockopt,
    Setsockopt,
    Send,
    Recv,
    Poll,
    // Windows' own calls, one per kernel32, ntdll, bcryptprimitives or
    // iphlpapi function (ADR 0020, ADR 0026): no other platform has them.
    GetStdHandle,
    WriteFile,
    ReadFile,
    GetConsoleMode,
    WriteConsole,
    ReadConsole,
    CreateFile,
    CloseHandle,
    FinalPathName,
    SetEndOfFile,
    NtCreateFile,
    NtQueryInformationFile,
    NtSetInformationFile,
    NtQueryDirectoryFile,
    VirtualAlloc,
    VirtualFree,
    ExitProcess,
    CommandLine,
    Environment,
    AddExceptionHandler,
    StackGuarantee,
    SystemTime,
    ProcessPrng,
    // `ws2_32.dll`'s calls that no other platform has.
    WsaStartup,
    CloseSocket,
    IoctlSocket,
    GetAdaptersAddresses,
}

/// What the backend gives `std.rt` for a target platform: how its
/// operating system is called, which `std.rt` table holds its numbers,
/// flags and structure layouts, and how its process starts.
#[derive(Debug, Clone, Copy)]
pub struct Platform {
    /// The `std.rt` function giving the platform's table, which
    /// `rt_mem_platform` gives.
    pub table: &'static str,
    /// How the operating system makes each call.
    pub call: fn(Sys) -> OsCall,
    /// Every function the executable imports from the platform's one
    /// library, in the order of its import table: none on Linux. Every
    /// [`OsCall::Library`] the platform gives is one of them, and so is
    /// [`Platform::errno`] (ADR 0015: the list is part of the reviewed
    /// surface of the operating-system interface, like [`RUNTIME`]).
    pub imports: &'static [&'static Import],
    /// The function that gives the thread's error, by its index in
    /// [`Platform::imports`], for a library call that fails, and how it
    /// gives it.
    pub errno: u16,
    pub error: ErrorRead,
    /// The result of a call the platform does not have
    /// ([`OsCall::Absent`]): its "not supported" error, negated.
    pub absent: i64,
    /// How the entry point finds the program's arguments.
    pub start: Start,
}

/// How the operating system starts the process at the entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// The kernel jumps to it, with `argc`, then the `argv` pointers, on
    /// the stack (Linux).
    Stack,
    /// The dynamic loader calls it as C's `main(argc, argv)`, by the
    /// platform's C calling convention (macOS's `LC_MAIN`).
    Main,
    /// The loader calls it with nothing the program uses: `std.rt` reads
    /// the command line and the environment itself, and the program runs
    /// on the thread's own stack (Windows, ADR 0020).
    Windows,
}

/// How a platform's error function gives a failed call's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorRead {
    /// The address of a 32-bit `errno` (macOS's `__error()`).
    Address,
    /// The error itself (Windows' `GetLastError()`).
    Value,
}

impl Platform {
    /// Where `import` is in [`Platform::imports`]: its slot in the
    /// executable's import table.
    pub fn slot(&self, import: &Import) -> Option<u16> {
        let at = self
            .imports
            .iter()
            .position(|i| i.symbol == import.symbol)?;
        self.slot_of(at)
    }

    /// The import table's slot of the `index`th import: its index, past
    /// the null slot that ends each library's run before it (PE's import
    /// address table; a Mach-O has one library and no such slot).
    pub fn slot_of(&self, index: usize) -> Option<u16> {
        let breaks = self.imports[..=index.min(self.imports.len().checked_sub(1)?)]
            .windows(2)
            .filter(|pair| pair[0].library != pair[1].library)
            .count();
        u16::try_from(index + breaks).ok()
    }
}

/// How an operating system makes one [`Sys`] call: one call of its
/// interface, or none (ADR 0013, ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsCall {
    /// The system call `number`, passed the intrinsic's arguments and then
    /// `extra_args`, the constant arguments the system call takes beyond
    /// the intrinsic's.
    Trap {
        number: u16,
        extra_args: &'static [i64],
    },
    /// A function of the one library the platform links, called by the
    /// platform's C calling convention, with a failure turned into
    /// `-errno`.
    Library(&'static Import),
    /// No call: the result is the intrinsic's first argument.
    Same,
    /// No call: the platform has no such call, and `std.rt` never makes
    /// it there (ADR 0020). The result is [`Platform::absent`].
    Absent,
}

/// A function the executable imports from the platform's library, and
/// how a call of it is made and read (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import {
    /// The library it comes from, as the executable names it.
    pub library: &'static str,
    /// Its symbol, as the library exports it: on macOS, C's name with a
    /// leading `_`.
    pub symbol: &'static str,
    /// How many of the intrinsic's arguments it takes: the rest are
    /// dropped (`readdir` takes only the directory).
    pub args: u8,
    /// How many arguments it declares before `...`, `extra_args` included:
    /// the rest are variadic. All of them when it is not variadic.
    pub fixed: u8,
    /// Whether its result is a whole word (a pointer, `ssize_t`); if not,
    /// it is a C `int`, whose 32 bits are sign-extended.
    pub wide: bool,
    /// How it says it failed.
    pub fails: Fails,
    /// The constant arguments it takes after the intrinsic's.
    pub extra_args: &'static [i64],
}

/// How a library function says it failed, so the backend can give
/// `std.rt` `-errno` in its place, as a Linux system call gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fails {
    /// It returns -1 and sets `errno`.
    MinusOne,
    /// It returns `NULL` and sets `errno`.
    Null,
    /// It returns `NULL` both at the end and on failure, and sets `errno`
    /// only on failure (`readdir`): `errno` is cleared before the call,
    /// and a `NULL` with `errno` still 0 is the result 0.
    NullKeepsErrno,
    /// It returns 0 (`FALSE` or `NULL`) and the error function gives
    /// the error (Windows).
    Zero,
    /// It returns an `NTSTATUS`, negative for an error, which
    /// [`NT_STATUS_TO_ERROR`] turns into a Win32 error (Windows'
    /// `ntdll`, ADR 0020).
    Status,
    /// It cannot fail, or does not return.
    Never,
}
