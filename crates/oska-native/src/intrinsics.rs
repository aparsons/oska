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

use crate::ir::{BinOp, Width};

/// How a prelude intrinsic is lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prelude {
    /// A call of the `std.rt` function of this name.
    Runtime(&'static str),
    /// Its one argument is its result, as `String.to_string`'s is.
    Identity,
    /// Its result is field `n` of its struct receiver.
    Field(usize),
    /// A call of the `std.rt` function of this name, which gives the word
    /// 0 (or, for [`Check::Repeat`], also -1) in place of its result when
    /// the call is a bug, for the generated code to panic as `check` says.
    Checked(&'static str, Check),
}

/// The panics a [`Prelude::Checked`] call may end in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// `String.slice`: `P0035` on the word 0.
    Slice,
    /// `String.repeat`: `P0036` on the word 0, `P0037` on -1.
    Repeat,
    /// `Env.secret`: `P0020` on the word 0.
    Secret,
}

/// The prelude intrinsics the native backend implements, by the name the
/// checked program calls them by. A call of any other is refused (`E0390`).
pub const PRELUDE: &[(&str, Prelude)] = &[
    ("Stdout.println", Prelude::Runtime("stdout_println")),
    ("Stderr.println", Prelude::Runtime("stderr_println")),
    ("Stdin.read_line", Prelude::Runtime("stdin_read_line")),
    ("Args.get", Prelude::Runtime("args_get")),
    ("Env.get", Prelude::Runtime("env_get")),
    ("Env.secret", Prelude::Checked("env_secret", Check::Secret)),
    ("Secret.expose", Prelude::Field(0)),
    (
        "Secret.constant_time_eq",
        Prelude::Runtime("secret_constant_time_eq"),
    ),
    (
        "Secret.constant_time_eq_secret",
        Prelude::Runtime("secret_constant_time_eq_secret"),
    ),
    ("Clock.now_millis", Prelude::Runtime("clock_now_millis")),
    ("Random.next_int", Prelude::Runtime("random_next_int")),
    ("String.to_string", Prelude::Identity),
    ("String.eq", Prelude::Runtime("string_eq")),
    ("Int.to_string", Prelude::Runtime("int_to_string")),
    ("IoError.to_string", Prelude::Field(0)),
    ("String.len", Prelude::Runtime("string_len")),
    ("String.byte_len", Prelude::Runtime("string_byte_len")),
    ("String.contains", Prelude::Runtime("string_contains")),
    ("String.starts_with", Prelude::Runtime("string_starts_with")),
    ("String.ends_with", Prelude::Runtime("string_ends_with")),
    ("String.trim", Prelude::Runtime("string_trim")),
    (
        "String.to_lowercase",
        Prelude::Runtime("string_to_lowercase"),
    ),
    ("String.replace", Prelude::Runtime("string_replace")),
    (
        "String.slice",
        Prelude::Checked("string_slice", Check::Slice),
    ),
    (
        "String.repeat",
        Prelude::Checked("string_repeat", Check::Repeat),
    ),
    ("String.assume_trusted", Prelude::Identity),
    ("String.parse_int", Prelude::Runtime("string_parse_int")),
    ("String.split", Prelude::Runtime("string_split")),
    ("String.lines", Prelude::Runtime("string_lines")),
    ("String.chars", Prelude::Runtime("string_chars")),
    ("String.join", Prelude::Runtime("string_join")),
    ("Path.new", Prelude::Runtime("path_new")),
    ("Path.from", Prelude::Runtime("path_new")),
    ("Path.eq", Prelude::Runtime("path_eq")),
    // `std.html`'s `text`, `TrustedHtml.new` and `Html.concat` build a
    // new object: `lower/html.rs`.
    ("Html.raw", Prelude::Identity),
    ("Html.render", Prelude::Field(0)),
    // Attenuation that only narrows the access: the type says which
    // operations a capability has, so the value stays the same.
    ("Fs.read_only", Prelude::Identity),
    ("Fs.write_only", Prelude::Identity),
    ("Dir.read_only", Prelude::Identity),
    ("Dir.write_only", Prelude::Identity),
    ("Fs.restrict_to", Prelude::Runtime("fs_restrict_to")),
    ("FsRead.restrict_to", Prelude::Runtime("fs_restrict_to")),
    ("FsWrite.restrict_to", Prelude::Runtime("fs_restrict_to")),
    (
        "FsRead.restrict_to_file",
        Prelude::Runtime("fs_restrict_to_file"),
    ),
    (
        "FsWrite.restrict_to_file",
        Prelude::Runtime("fs_restrict_to_file"),
    ),
    ("Dir.restrict_to", Prelude::Runtime("dir_restrict_to")),
    ("DirRead.restrict_to", Prelude::Runtime("dir_restrict_to")),
    ("DirWrite.restrict_to", Prelude::Runtime("dir_restrict_to")),
    (
        "DirRead.restrict_to_file",
        Prelude::Runtime("dir_restrict_to_file"),
    ),
    (
        "DirWrite.restrict_to_file",
        Prelude::Runtime("dir_restrict_to_file"),
    ),
    (
        "DirRead.read_to_string",
        Prelude::Runtime("dir_read_to_string"),
    ),
    ("DirRead.list_dir", Prelude::Runtime("dir_list_dir")),
    ("DirWrite.write", Prelude::Runtime("dir_write")),
    ("DirWrite.create_dir", Prelude::Runtime("dir_create_dir")),
    (
        "FileRead.read_to_string",
        Prelude::Runtime("file_read_to_string"),
    ),
    ("FileWrite.write", Prelude::Runtime("file_write")),
    ("TestContext.printed", Prelude::Runtime("test_printed")),
];

/// `name` without the module a `std` function is called by when the
/// program declares the same name itself (`html.text` for `text`).
pub fn unqualified(name: &str) -> &str {
    oska_check::builtins::std_modules()
        .filter_map(|path| path.rsplit('.').next())
        .find_map(|module| name.strip_prefix(module)?.strip_prefix('.'))
        .unwrap_or(name)
}

/// The implementation of the prelude intrinsic `name`, if there is one.
pub fn prelude(name: &str) -> Option<Prelude> {
    let name = unqualified(name);
    PRELUDE.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

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
    /// Which host the process runs on: always 0, the real OS, in a
    /// program's executable; in a test executable, what its entry set
    /// (`lower/tests.rs`).
    TestHost,
    /// `std.rt`'s table of what differs between target platforms, the
    /// target platform's ([`Platform::table`]).
    PlatformTable,
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
    /// The function that gives the address of the thread's `errno`, by
    /// its index in [`Platform::imports`], for a library call that fails.
    pub errno: u16,
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
}

impl Platform {
    /// Where `import` is in [`Platform::imports`]: its slot in the
    /// executable's import table.
    pub fn slot(&self, import: &Import) -> Option<u16> {
        let at = self
            .imports
            .iter()
            .position(|i| i.symbol == import.symbol)?;
        u16::try_from(at).ok()
    }
}

/// How an operating system makes one [`Sys`] call: one call of its
/// interface, or none (ADR 0013, ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsCall {
    /// The system call `number`, passed the intrinsic's arguments and then
    /// `then`, the constant arguments the system call takes beyond the
    /// intrinsic's.
    Trap { number: u16, then: &'static [i64] },
    /// A function of the one library the platform links, called by the
    /// platform's C calling convention, with a failure turned into
    /// `-errno`.
    Library(&'static Import),
    /// No call: the result is the intrinsic's first argument.
    Same,
}

/// A function the executable imports from the platform's library, and
/// how a call of it is made and read (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import {
    /// Its symbol, as the library exports it: C's name with a leading
    /// `_`.
    pub symbol: &'static str,
    /// How many of the intrinsic's arguments it takes: the rest are
    /// dropped (`readdir` takes only the directory).
    pub args: u8,
    /// How many arguments it declares before `...`, `then` included: the
    /// rest are variadic. All of them when it is not variadic.
    pub fixed: u8,
    /// Whether its result is a whole word (a pointer, `ssize_t`); if not,
    /// it is a C `int`, whose 32 bits are sign-extended.
    pub wide: bool,
    /// How it says it failed.
    pub fails: Fails,
    /// The constant arguments it takes after the intrinsic's.
    pub then: &'static [i64],
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
    /// It cannot fail, or does not return.
    Never,
}

/// `linux-aarch64`.
pub const LINUX_AARCH64: Platform = Platform {
    table: "linux_aarch64",
    call: linux_aarch64_call,
    imports: &[],
    errno: 0,
    start: Start::Stack,
};

/// The Linux system call `number` gives for `call` (none for `None`),
/// with what it takes after a neutral intrinsic's arguments, on every
/// processor: `getrandom`'s flags (none), and `rt_sigaction`'s old
/// action (none kept) and the size of its signal mask in bytes.
fn linux_call(call: Sys, number: fn(Sys) -> Option<u16>) -> OsCall {
    let Some(number) = number(call) else {
        return OsCall::Same;
    };
    let then: &'static [i64] = match call {
        Sys::Random => &[0],
        Sys::Sigaction => &[0, 8],
        _ => &[],
    };
    OsCall::Trap { number, then }
}

fn linux_aarch64_call(call: Sys) -> OsCall {
    linux_call(call, linux_aarch64_syscall)
}

/// `linux-aarch64`'s system-call numbers: the generic Linux table's
/// (`asm-generic/unistd.h`). A directory is listed through its file
/// descriptor, so opening one for listing makes no call, and closing it
/// is `close`.
fn linux_aarch64_syscall(call: Sys) -> Option<u16> {
    Some(match call {
        Sys::OpenDir => return None,
        Sys::Read => 63,
        Sys::Write => 64,
        Sys::Openat => 56,
        Sys::Close | Sys::CloseDir => 57,
        Sys::Mkdirat => 34,
        Sys::ReadDir => 61,
        Sys::Fstat => 80,
        Sys::Ftruncate => 46,
        Sys::Readlinkat => 78,
        Sys::Getcwd => 17,
        Sys::Fcntl => 25,
        Sys::Mmap => 222,
        Sys::Munmap => 215,
        Sys::ClockGettime => 113,
        Sys::Ioctl => 29,
        Sys::Random => 278,
        Sys::Sigaction => 134,
        Sys::Sigaltstack => 132,
        Sys::Getpid => 172,
        Sys::Kill => 129,
        Sys::Exit => 94,
    })
}

/// `linux-x86_64`.
pub const LINUX_X86_64: Platform = Platform {
    table: "linux_x86_64",
    call: linux_x86_64_call,
    imports: &[],
    errno: 0,
    start: Start::Stack,
};

fn linux_x86_64_call(call: Sys) -> OsCall {
    linux_call(call, linux_x86_64_syscall)
}

/// `linux-x86_64`'s system-call numbers (`arch/x86/entry/syscalls/
/// syscall_64.tbl`), and the same directory calls as aarch64's.
fn linux_x86_64_syscall(call: Sys) -> Option<u16> {
    Some(match call {
        Sys::OpenDir => return None,
        Sys::Read => 0,
        Sys::Write => 1,
        Sys::Openat => 257,
        Sys::Close | Sys::CloseDir => 3,
        Sys::Mkdirat => 258,
        Sys::ReadDir => 217,
        Sys::Fstat => 5,
        Sys::Ftruncate => 77,
        Sys::Readlinkat => 267,
        Sys::Getcwd => 79,
        Sys::Fcntl => 72,
        Sys::Mmap => 9,
        Sys::Munmap => 11,
        Sys::ClockGettime => 228,
        Sys::Ioctl => 16,
        Sys::Random => 318,
        Sys::Sigaction => 13,
        Sys::Sigaltstack => 131,
        Sys::Getpid => 39,
        Sys::Kill => 62,
        Sys::Exit => 231,
    })
}

/// `macos-aarch64`: every call is a function of `libSystem`, through the
/// dynamic loader (ADR 0011, ADR 0015).
pub const MACOS_AARCH64: Platform = Platform {
    table: "macos",
    call: macos_call,
    imports: &MACOS_IMPORTS,
    errno: 0,
    start: Start::Main,
};

/// A function of `libSystem` that is not variadic and fails with -1, as
/// most do: `args` arguments and an `int` result.
const fn plain(symbol: &'static str, args: u8) -> Import {
    Import {
        symbol,
        args,
        fixed: args,
        wide: false,
        fails: Fails::MinusOne,
        then: &[],
    }
}

/// `__error()`, the address of the thread's `errno`: no intrinsic's.
const ERROR: Import = Import {
    fails: Fails::Never,
    wide: true,
    ..plain("___error", 0)
};
const READ: Import = Import {
    wide: true,
    ..plain("_read", 3)
};
const WRITE: Import = Import {
    wide: true,
    ..plain("_write", 3)
};
/// `openat(fd, path, flags, ...)`: the mode is variadic.
const OPENAT: Import = Import {
    fixed: 3,
    ..plain("_openat", 4)
};
const CLOSE: Import = plain("_close", 1);
const MKDIRAT: Import = plain("_mkdirat", 3);
/// `fdopendir(fd)`: a `DIR *`, or `NULL`.
const FDOPENDIR: Import = Import {
    wide: true,
    fails: Fails::Null,
    ..plain("_fdopendir", 1)
};
/// `readdir(dir)`: the next record, or `NULL` at the end and on failure.
const READDIR: Import = Import {
    wide: true,
    fails: Fails::NullKeepsErrno,
    ..plain("_readdir", 1)
};
const CLOSEDIR: Import = plain("_closedir", 1);
const FSTAT: Import = plain("_fstat", 2);
const FTRUNCATE: Import = plain("_ftruncate", 2);
const READLINKAT: Import = Import {
    wide: true,
    ..plain("_readlinkat", 4)
};
/// `getcwd(buf, size)`: `buf`, or `NULL`.
const GETCWD: Import = Import {
    wide: true,
    fails: Fails::Null,
    ..plain("_getcwd", 2)
};
/// `fcntl(fd, command, ...)`: the argument is variadic.
const FCNTL: Import = Import {
    fixed: 2,
    ..plain("_fcntl", 3)
};
/// `mmap`: an address, or `MAP_FAILED` (-1).
const MMAP: Import = Import {
    wide: true,
    ..plain("_mmap", 6)
};
const MUNMAP: Import = plain("_munmap", 2);
const CLOCK_GETTIME: Import = plain("_clock_gettime", 2);
/// `ioctl(fd, request, ...)`: the argument is variadic.
const IOCTL: Import = Import {
    fixed: 2,
    ..plain("_ioctl", 3)
};
/// `getentropy(buf, length)`: at most 256 bytes, all of them, or -1.
const GETENTROPY: Import = plain("_getentropy", 2);
/// `sigaction(signal, action, old)`: no old action kept.
const SIGACTION: Import = Import {
    fixed: 3,
    then: &[0],
    ..plain("_sigaction", 2)
};
const SIGALTSTACK: Import = plain("_sigaltstack", 2);
const GETPID: Import = Import {
    fails: Fails::Never,
    ..plain("_getpid", 0)
};
const KILL: Import = plain("_kill", 2);
/// `_exit(status)`, not `exit`: no `atexit` handlers run, as with Linux's
/// `exit_group`.
const EXIT: Import = Import {
    fails: Fails::Never,
    ..plain("__exit", 1)
};

/// Every function a macOS executable imports from `libSystem`: always
/// all of them, whatever the program calls, so the import table depends
/// on the target alone. [`ERROR`] is first ([`Platform::errno`]).
const MACOS_IMPORTS: [&Import; 24] = [
    &ERROR,
    &READ,
    &WRITE,
    &OPENAT,
    &CLOSE,
    &MKDIRAT,
    &FDOPENDIR,
    &READDIR,
    &CLOSEDIR,
    &FSTAT,
    &FTRUNCATE,
    &READLINKAT,
    &GETCWD,
    &FCNTL,
    &MMAP,
    &MUNMAP,
    &CLOCK_GETTIME,
    &IOCTL,
    &GETENTROPY,
    &SIGACTION,
    &SIGALTSTACK,
    &GETPID,
    &KILL,
    &EXIT,
];

/// The `libSystem` function behind each call on macOS. A directory is
/// listed through a `DIR *` that owns its descriptor (`fdopendir`,
/// `readdir`, `closedir`), one record at a time.
fn macos_call(call: Sys) -> OsCall {
    OsCall::Library(match call {
        Sys::Read => &READ,
        Sys::Write => &WRITE,
        Sys::Openat => &OPENAT,
        Sys::Close => &CLOSE,
        Sys::Mkdirat => &MKDIRAT,
        Sys::OpenDir => &FDOPENDIR,
        Sys::ReadDir => &READDIR,
        Sys::CloseDir => &CLOSEDIR,
        Sys::Fstat => &FSTAT,
        Sys::Ftruncate => &FTRUNCATE,
        Sys::Readlinkat => &READLINKAT,
        Sys::Getcwd => &GETCWD,
        Sys::Fcntl => &FCNTL,
        Sys::Mmap => &MMAP,
        Sys::Munmap => &MUNMAP,
        Sys::ClockGettime => &CLOCK_GETTIME,
        Sys::Ioctl => &IOCTL,
        Sys::Random => &GETENTROPY,
        Sys::Sigaction => &SIGACTION,
        Sys::Sigaltstack => &SIGALTSTACK,
        Sys::Getpid => &GETPID,
        Sys::Kill => &KILL,
        Sys::Exit => &EXIT,
    })
}

/// Every `std.rt` intrinsic, by name.
pub const RUNTIME: &[(&str, Runtime)] = &[
    ("rt_sys_read", Runtime::Syscall(Sys::Read)),
    ("rt_sys_write", Runtime::Syscall(Sys::Write)),
    ("rt_sys_openat", Runtime::Syscall(Sys::Openat)),
    ("rt_sys_close", Runtime::Syscall(Sys::Close)),
    ("rt_sys_mkdirat", Runtime::Syscall(Sys::Mkdirat)),
    ("rt_sys_open_dir", Runtime::Syscall(Sys::OpenDir)),
    ("rt_sys_read_dir", Runtime::Syscall(Sys::ReadDir)),
    ("rt_sys_close_dir", Runtime::Syscall(Sys::CloseDir)),
    ("rt_sys_fstat", Runtime::Syscall(Sys::Fstat)),
    ("rt_sys_ftruncate", Runtime::Syscall(Sys::Ftruncate)),
    ("rt_sys_readlinkat", Runtime::Syscall(Sys::Readlinkat)),
    ("rt_sys_getcwd", Runtime::Syscall(Sys::Getcwd)),
    ("rt_sys_fcntl", Runtime::Syscall(Sys::Fcntl)),
    ("rt_sys_mmap", Runtime::Syscall(Sys::Mmap)),
    ("rt_sys_munmap", Runtime::Syscall(Sys::Munmap)),
    ("rt_sys_clock_gettime", Runtime::Syscall(Sys::ClockGettime)),
    ("rt_sys_ioctl", Runtime::Syscall(Sys::Ioctl)),
    ("rt_sys_random", Runtime::Syscall(Sys::Random)),
    ("rt_sys_sigaction", Runtime::Syscall(Sys::Sigaction)),
    ("rt_sys_sigaltstack", Runtime::Syscall(Sys::Sigaltstack)),
    ("rt_sys_getpid", Runtime::Syscall(Sys::Getpid)),
    ("rt_sys_kill", Runtime::Syscall(Sys::Kill)),
    ("rt_sys_exit", Runtime::Syscall(Sys::Exit)),
    ("rt_mem_load8", Runtime::Load(Width::W8)),
    ("rt_mem_load16", Runtime::Load(Width::W16)),
    ("rt_mem_load32", Runtime::Load(Width::W32)),
    ("rt_mem_load64", Runtime::Load(Width::W64)),
    ("rt_mem_store8", Runtime::Store(Width::W8)),
    ("rt_mem_store16", Runtime::Store(Width::W16)),
    ("rt_mem_store32", Runtime::Store(Width::W32)),
    ("rt_mem_store64", Runtime::Store(Width::W64)),
    ("rt_mem_and", Runtime::Binary(BinOp::And)),
    ("rt_mem_or", Runtime::Binary(BinOp::Or)),
    ("rt_mem_xor", Runtime::Binary(BinOp::Xor)),
    ("rt_mem_shl", Runtime::Binary(BinOp::Shl)),
    ("rt_mem_shr", Runtime::Binary(BinOp::Shr)),
    ("rt_mem_sar", Runtime::Binary(BinOp::Sar)),
    ("rt_mem_add", Runtime::Binary(BinOp::Add)),
    ("rt_mem_sub", Runtime::Binary(BinOp::Sub)),
    ("rt_mem_mul", Runtime::Binary(BinOp::Mul)),
    ("rt_mem_text", Runtime::Identity),
    ("rt_mem_as_text", Runtime::Identity),
    ("rt_mem_secret", Runtime::Identity),
    ("rt_mem_as_secret", Runtime::Identity),
    ("rt_mem_as_io_error", Runtime::Identity),
    ("rt_mem_path", Runtime::Identity),
    ("rt_mem_as_path", Runtime::Identity),
    ("rt_mem_list", Runtime::Identity),
    ("rt_mem_as_list", Runtime::Identity),
    ("rt_mem_as_root", Runtime::Identity),
    ("rt_mem_globals", Runtime::Globals),
    ("rt_mem_test_host", Runtime::TestHost),
    ("rt_mem_platform", Runtime::PlatformTable),
];

/// The implementation of the `std.rt` intrinsic `name`, if there is one.
pub fn runtime(name: &str) -> Option<Runtime> {
    RUNTIME.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_std_rt_intrinsic_has_a_native_implementation_and_no_other_does() {
        let declared = oska_check::std_rt_intrinsics();
        let implemented: Vec<&str> = RUNTIME.iter().map(|(name, _)| *name).collect();
        let mut declared_sorted = declared.clone();
        declared_sorted.sort_unstable();
        let mut implemented_sorted = implemented.clone();
        implemented_sorted.sort_unstable();
        assert_eq!(declared_sorted, implemented_sorted);
    }

    #[test]
    fn every_platform_table_the_backend_gives_is_a_std_rt_table() {
        let report = oska_check::check_toolchain_rt();
        let program = report.program.expect("std.rt should check");
        for platform in crate::Target::ALL.map(crate::Target::platform) {
            let table = program
                .functions
                .get(platform.table)
                .unwrap_or_else(|| panic!("std.rt has no `{}`", platform.table));
            assert!(table.params.is_empty() && !table.receiver);
            assert_eq!(table.ret.to_string(), "Platform");
        }
    }

    #[test]
    fn the_rt_sys_intrinsics_are_the_system_calls() {
        for (name, implementation) in RUNTIME {
            assert_eq!(
                name.starts_with("rt_sys_"),
                matches!(implementation, Runtime::Syscall(_)),
                "`{name}`"
            );
        }
    }

    /// Each Linux call is the system call it was before the `Platform`
    /// reshape (#366): the kernel's number on each processor, and the
    /// same constant trailing arguments. Opening a directory for listing
    /// makes no call, and closing it is `close`.
    #[test]
    fn linux_makes_each_call_as_the_system_call_it_made_before() {
        // (intrinsic, aarch64's number, x86_64's, trailing arguments)
        let rows: &[(&str, u16, u16, &[i64])] = &[
            ("rt_sys_read", 63, 0, &[]),
            ("rt_sys_write", 64, 1, &[]),
            ("rt_sys_openat", 56, 257, &[]),
            ("rt_sys_close", 57, 3, &[]),
            ("rt_sys_mkdirat", 34, 258, &[]),
            ("rt_sys_read_dir", 61, 217, &[]),
            ("rt_sys_close_dir", 57, 3, &[]),
            ("rt_sys_fstat", 80, 5, &[]),
            ("rt_sys_ftruncate", 46, 77, &[]),
            ("rt_sys_readlinkat", 78, 267, &[]),
            ("rt_sys_getcwd", 17, 79, &[]),
            ("rt_sys_fcntl", 25, 72, &[]),
            ("rt_sys_mmap", 222, 9, &[]),
            ("rt_sys_munmap", 215, 11, &[]),
            ("rt_sys_clock_gettime", 113, 228, &[]),
            ("rt_sys_ioctl", 29, 16, &[]),
            // getrandom(buf, len, flags): no flags.
            ("rt_sys_random", 278, 318, &[0]),
            // rt_sigaction(sig, act, old, size): no old action, an 8-byte
            // signal mask.
            ("rt_sys_sigaction", 134, 13, &[0, 8]),
            ("rt_sys_sigaltstack", 132, 131, &[]),
            ("rt_sys_getpid", 172, 39, &[]),
            ("rt_sys_kill", 129, 62, &[]),
            ("rt_sys_exit", 94, 231, &[]),
        ];
        for (name, implementation) in RUNTIME {
            let Runtime::Syscall(call) = implementation else {
                continue;
            };
            for platform in [LINUX_AARCH64, LINUX_X86_64] {
                let expected = if *name == "rt_sys_open_dir" {
                    OsCall::Same
                } else {
                    let (_, aarch64, x86_64, then) = rows
                        .iter()
                        .find(|row| row.0 == *name)
                        .unwrap_or_else(|| panic!("`{name}` has no row"));
                    let number = if platform.table == "linux_aarch64" {
                        *aarch64
                    } else {
                        *x86_64
                    };
                    OsCall::Trap { number, then }
                };
                assert_eq!(
                    (platform.call)(*call),
                    expected,
                    "`{name}` on `{}`",
                    platform.table
                );
            }
        }
    }

    /// A program that reaches every system call `std.rt` makes: the
    /// standard streams, the clock and randomness, and confined reads,
    /// writes, listings and directories, whose paths are resolved against
    /// the working directory.
    const EVERY_CALL: &str = r#"fn main(io: Stdout, input: Stdin, fs: Fs, clock: Clock, random: Random) -> Result<(), IoError> {
    let read = fs.read_only().restrict_to("root")
    let write = fs.write_only().restrict_to("root")
    let names = read.list_dir(Path.new("sub"))?
    let text = read.read_to_string(Path.new("a.txt"))?
    write.write(Path.new("b.txt"), "b")?
    write.create_dir(Path.new("c"))?
    let file = fs.write_only().restrict_to_file("root/d.txt")
    file.write("d")?
    io.println(clock.now_millis().to_string())?
    io.println(random.next_int().to_string())?
    let line = input.read_line()?
    Ok(())
}
"#;

    /// Every system call in `EVERY_CALL`'s executable for `target`: its
    /// number and how many arguments it is passed.
    fn system_calls(target: crate::Target) -> Vec<(u16, usize)> {
        let report = oska_check::check_module(EVERY_CALL);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let program = report.program.expect("the program should check");
        let rt = oska_check::check_toolchain_rt()
            .program
            .expect("std.rt should check");
        let module =
            crate::lower::program(&program, &rt, &|d| d, crate::lower::Entry::Main, target)
                .expect("the program should lower");
        let calls: std::collections::BTreeSet<(u16, usize)> = module
            .functions
            .iter()
            .flat_map(|function| &function.insts)
            .filter_map(|inst| match inst {
                crate::ir::Inst::Syscall { number, args, .. } => Some((*number, args.len())),
                _ => None,
            })
            .collect();
        calls.into_iter().collect()
    }

    /// Linux executables make the system calls they made before the
    /// `Platform` reshape (#366), each with as many arguments. The
    /// numbers are the kernel's, from its tables, not the backend's.
    #[test]
    fn linux_executables_make_the_same_system_calls() {
        // getcwd, fcntl, ioctl, mkdirat, ftruncate, openat, close,
        // getdents64, read, write, readlinkat, fstat, exit_group,
        // clock_gettime, kill, sigaltstack, rt_sigaction, getpid, munmap,
        // mmap, getrandom: `asm-generic/unistd.h`.
        let aarch64: &[(u16, usize)] = &[
            (17, 2),
            (25, 3),
            (29, 3),
            (34, 3),
            (46, 2),
            (56, 4),
            (57, 1),
            (61, 3),
            (63, 3),
            (64, 3),
            (78, 4),
            (80, 2),
            (94, 1),
            (113, 2),
            (129, 2),
            (132, 2),
            (134, 4),
            (172, 0),
            (215, 2),
            (222, 6),
            (278, 3),
        ];
        // read, write, close, fstat, mmap, munmap, rt_sigaction, ioctl,
        // getpid, kill, fcntl, ftruncate, getcwd, sigaltstack, getdents64,
        // clock_gettime, exit_group, openat, mkdirat, readlinkat,
        // getrandom: `arch/x86/entry/syscalls/syscall_64.tbl`.
        let x86_64: &[(u16, usize)] = &[
            (0, 3),
            (1, 3),
            (3, 1),
            (5, 2),
            (9, 6),
            (11, 2),
            (13, 4),
            (16, 3),
            (39, 0),
            (62, 2),
            (72, 3),
            (77, 2),
            (79, 2),
            (131, 2),
            (217, 3),
            (228, 2),
            (231, 1),
            (257, 4),
            (258, 3),
            (267, 4),
            (318, 3),
        ];
        assert_eq!(system_calls(crate::Target::LinuxAarch64), aarch64);
        assert_eq!(system_calls(crate::Target::LinuxX86_64), x86_64);
    }

    /// Each macOS call is one `libSystem` function, made as Apple's
    /// headers declare it: (intrinsic, symbol, arguments it takes, fixed
    /// arguments, whole-word result, how it fails, trailing constants).
    #[test]
    fn macos_makes_each_call_as_the_libsystem_function_declares_it() {
        use Fails::*;
        /// (intrinsic, symbol, arguments, fixed, wide, fails, then).
        type Row = (
            &'static str,
            &'static str,
            u8,
            u8,
            bool,
            Fails,
            &'static [i64],
        );
        let rows: &[Row] = &[
            ("rt_sys_read", "_read", 3, 3, true, MinusOne, &[]),
            ("rt_sys_write", "_write", 3, 3, true, MinusOne, &[]),
            // openat(fd, path, flags, ...): the mode is variadic.
            ("rt_sys_openat", "_openat", 4, 3, false, MinusOne, &[]),
            ("rt_sys_close", "_close", 1, 1, false, MinusOne, &[]),
            ("rt_sys_mkdirat", "_mkdirat", 3, 3, false, MinusOne, &[]),
            ("rt_sys_open_dir", "_fdopendir", 1, 1, true, Null, &[]),
            // readdir(dir): one record, or NULL at the end or on failure.
            (
                "rt_sys_read_dir",
                "_readdir",
                1,
                1,
                true,
                NullKeepsErrno,
                &[],
            ),
            ("rt_sys_close_dir", "_closedir", 1, 1, false, MinusOne, &[]),
            ("rt_sys_fstat", "_fstat", 2, 2, false, MinusOne, &[]),
            ("rt_sys_ftruncate", "_ftruncate", 2, 2, false, MinusOne, &[]),
            (
                "rt_sys_readlinkat",
                "_readlinkat",
                4,
                4,
                true,
                MinusOne,
                &[],
            ),
            ("rt_sys_getcwd", "_getcwd", 2, 2, true, Null, &[]),
            ("rt_sys_fcntl", "_fcntl", 3, 2, false, MinusOne, &[]),
            ("rt_sys_mmap", "_mmap", 6, 6, true, MinusOne, &[]),
            ("rt_sys_munmap", "_munmap", 2, 2, false, MinusOne, &[]),
            (
                "rt_sys_clock_gettime",
                "_clock_gettime",
                2,
                2,
                false,
                MinusOne,
                &[],
            ),
            ("rt_sys_ioctl", "_ioctl", 3, 2, false, MinusOne, &[]),
            ("rt_sys_random", "_getentropy", 2, 2, false, MinusOne, &[]),
            // sigaction(signal, action, old): no old action.
            (
                "rt_sys_sigaction",
                "_sigaction",
                2,
                3,
                false,
                MinusOne,
                &[0],
            ),
            (
                "rt_sys_sigaltstack",
                "_sigaltstack",
                2,
                2,
                false,
                MinusOne,
                &[],
            ),
            ("rt_sys_getpid", "_getpid", 0, 0, false, Never, &[]),
            ("rt_sys_kill", "_kill", 2, 2, false, MinusOne, &[]),
            // _exit, not exit: no atexit handlers, as exit_group.
            ("rt_sys_exit", "__exit", 1, 1, false, Never, &[]),
        ];
        for (name, implementation) in RUNTIME {
            let Runtime::Syscall(call) = implementation else {
                continue;
            };
            let row = rows
                .iter()
                .find(|row| row.0 == *name)
                .unwrap_or_else(|| panic!("`{name}` has no row"));
            let OsCall::Library(import) = (MACOS_AARCH64.call)(*call) else {
                panic!("`{name}` is not a libSystem call on macOS");
            };
            assert_eq!(
                (
                    import.symbol,
                    import.args,
                    import.fixed,
                    import.wide,
                    import.fails,
                    import.then
                ),
                (row.1, row.2, row.3, row.4, row.5, row.6),
                "`{name}`"
            );
            assert!(
                usize::from(import.args) + import.then.len() >= usize::from(import.fixed),
                "`{name}`"
            );
            assert!(MACOS_AARCH64.slot(import).is_some(), "`{name}` is imported");
        }
        let errno = MACOS_AARCH64.imports[usize::from(MACOS_AARCH64.errno)];
        assert_eq!(errno.symbol, "___error");
        // Every import is some call's, or `__error`'s, and none twice.
        assert_eq!(MACOS_AARCH64.imports.len(), rows.len() + 1);
        let mut symbols: Vec<&str> = MACOS_AARCH64.imports.iter().map(|i| i.symbol).collect();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), MACOS_AARCH64.imports.len());
    }

    /// A macOS executable makes no system call of its own: every call is
    /// a `libSystem` function, through the import table.
    #[test]
    fn macos_executables_call_only_libsystem() {
        let report = oska_check::check_module(EVERY_CALL);
        let program = report.program.expect("the program should check");
        let rt = oska_check::check_toolchain_rt()
            .program
            .expect("std.rt should check");
        let module = crate::lower::program(
            &program,
            &rt,
            &|d| d,
            crate::lower::Entry::Main,
            crate::Target::MacosAarch64,
        )
        .expect("the program should lower");
        let mut imports = std::collections::BTreeSet::new();
        for inst in module.functions.iter().flat_map(|f| &f.insts) {
            match inst {
                crate::ir::Inst::Syscall { .. } | crate::ir::Inst::EntrySp { .. } => {
                    panic!("a macOS executable has {inst:?}")
                }
                crate::ir::Inst::LibCall { import, .. } => {
                    imports.insert(*import);
                }
                _ => {}
            }
        }
        assert_eq!(imports.len(), MACOS_AARCH64.imports.len());
    }

    #[test]
    fn every_runtime_function_a_prelude_intrinsic_names_is_in_std_rt() {
        let report = oska_check::check_toolchain_rt();
        let program = report.program.expect("std.rt should check");
        for (name, implementation) in PRELUDE {
            if let Prelude::Runtime(function) = implementation {
                assert!(
                    program.functions.contains_key(*function),
                    "`{name}` names `{function}`, which std.rt lacks"
                );
            }
        }
    }
}
