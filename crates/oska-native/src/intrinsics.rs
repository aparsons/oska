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
    /// A call of the `std.rt` function of this name, which writes its
    /// argument 1 and a newline to stdout or stderr: under `--max-output`
    /// the bytes are counted first (`Body::spend_output`). Every prelude
    /// intrinsic whose function can write the program's text must be one,
    /// which a test checks.
    WriteLine(&'static str),
    /// Its one argument is its result, as `String.to_string`'s is.
    Identity,
    /// Its result is field `n` of its struct receiver.
    Field(usize),
    /// A call of the `std.rt` function of this name, which gives the word
    /// 0 (or, for [`Check::Repeat`], also -1) in place of its result when
    /// the call is a bug, for the generated code to panic as `check` says.
    Checked(&'static str, Check),
    /// `Bytes.at` (`width` 1, panicking with `P0002`) or an endian read
    /// of `width` bytes: `std.rt`'s `bytes_readable` first, which gives
    /// 0 when the bytes do not all lie inside the buffer, for the
    /// generated code to panic, then `bytes_read`.
    ///
    /// A `secret` one, `Secret<Bytes>`'s, calls `bytes_read_secret`
    /// instead, which marks each byte it reads secret
    /// (`constant_time.rs`); `Bytes`' own gives public bytes.
    ByteRead { width: u8, big: bool, secret: bool },
    /// A mutating method of `Bytes`: a call of the `std.rt` function of
    /// this name with the receiver's object (and the place's reference to
    /// it) and the other arguments, which gives the object the place
    /// holds afterwards, or the word 0, changing nothing, for the
    /// generated code to panic as `check` says.
    Mutating(&'static str, Check),
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
    /// `Bytes.zeroed`: `P0042` on the word 0, `P0043` on -1.
    Zeroed,
    /// `Bytes.slice`: `P0042` on the word 0.
    BytesSlice,
    /// `Bytes.set`: `P0002` on the word 0.
    BytesIndex,
    /// A `Bytes` push of this many bytes: `P0043` on the word 0.
    BytesGrow(u8),
    /// `Bytes.append`: `P0043` on the word 0.
    BytesAppend,
    /// `ct_select` between two `Bytes`: `P0042` on the word 0.
    BytesSelect,
}

/// The prelude intrinsics the native backend implements, by the name the
/// checked program calls them by. A call of any other is refused (`E0390`).
pub const PRELUDE: &[(&str, Prelude)] = &[
    ("Stdout.println", Prelude::WriteLine("stdout_println")),
    ("Stderr.println", Prelude::WriteLine("stderr_println")),
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
    // Not the prelude's: what the checked program lowers `Secret<Bytes>`'s
    // comparisons and `ct_select` to (`oska-check`'s `lower/secrets.rs`,
    // and `ExprKind::Select` here).
    (
        "Bytes.constant_time_eq",
        Prelude::Runtime("bytes_constant_time_eq"),
    ),
    (
        "Bytes.ct_select",
        Prelude::Checked("bytes_select", Check::BytesSelect),
    ),
    ("Clock.now_millis", Prelude::Runtime("clock_now_millis")),
    ("Random.next_int", Prelude::Runtime("random_next_int")),
    ("Budget.limit", Prelude::Runtime("budget_limit")),
    ("Budget.remaining", Prelude::Runtime("budget_remaining")),
    ("Budget.spent", Prelude::Runtime("budget_spent")),
    ("Budget.reserve", Prelude::Runtime("budget_reserve")),
    ("Reservation.debit", Prelude::Runtime("reservation_debit")),
    (
        "Reservation.release",
        Prelude::Runtime("reservation_release"),
    ),
    (
        "OverBudget.to_string",
        Prelude::Runtime("over_budget_to_string"),
    ),
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
    ("DirRead.read_bytes", Prelude::Runtime("dir_read_bytes")),
    ("DirRead.list_dir", Prelude::Runtime("dir_list_dir")),
    ("DirWrite.write", Prelude::Runtime("dir_write")),
    ("DirWrite.write_bytes", Prelude::Runtime("dir_write_bytes")),
    ("DirWrite.create_dir", Prelude::Runtime("dir_create_dir")),
    (
        "FileRead.read_to_string",
        Prelude::Runtime("file_read_to_string"),
    ),
    ("FileRead.read_bytes", Prelude::Runtime("file_read_bytes")),
    ("FileWrite.write", Prelude::Runtime("file_write")),
    (
        "FileWrite.write_bytes",
        Prelude::Runtime("file_write_bytes"),
    ),
    ("TestContext.printed", Prelude::Runtime("test_printed")),
    ("Bytes.new", Prelude::Runtime("bytes_new")),
    (
        "Bytes.zeroed",
        Prelude::Checked("bytes_zeroed", Check::Zeroed),
    ),
    ("Bytes.from_hex", Prelude::Runtime("bytes_from_hex")),
    ("Bytes.len", Prelude::Runtime("bytes_len")),
    ("Bytes.get", Prelude::Runtime("bytes_get")),
    (
        "Bytes.at",
        Prelude::ByteRead {
            width: 1,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.slice",
        Prelude::Checked("bytes_slice", Check::BytesSlice),
    ),
    (
        "Bytes.read_u16_be",
        Prelude::ByteRead {
            width: 2,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u16_le",
        Prelude::ByteRead {
            width: 2,
            big: false,
            secret: false,
        },
    ),
    (
        "Bytes.read_u32_be",
        Prelude::ByteRead {
            width: 4,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u32_le",
        Prelude::ByteRead {
            width: 4,
            big: false,
            secret: false,
        },
    ),
    (
        "Bytes.read_u64_be",
        Prelude::ByteRead {
            width: 8,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u64_le",
        Prelude::ByteRead {
            width: 8,
            big: false,
            secret: false,
        },
    ),
    ("Bytes.to_hex", Prelude::Runtime("bytes_to_hex")),
    ("Bytes.to_text", Prelude::Runtime("bytes_to_text")),
    ("Bytes.eq", Prelude::Runtime("bytes_eq")),
    ("Bytes.hash", Prelude::Runtime("bytes_hash")),
    ("String.to_bytes", Prelude::Runtime("string_to_bytes")),
    ("Bytes.assume_trusted", Prelude::Identity),
    (
        "Bytes.push",
        Prelude::Mutating("bytes_push", Check::BytesGrow(1)),
    ),
    (
        "Bytes.set",
        Prelude::Mutating("bytes_set", Check::BytesIndex),
    ),
    (
        "Bytes.append",
        Prelude::Mutating("bytes_append", Check::BytesAppend),
    ),
    (
        "Bytes.push_u16_be",
        Prelude::Mutating("bytes_push_u16_be", Check::BytesGrow(2)),
    ),
    (
        "Bytes.push_u16_le",
        Prelude::Mutating("bytes_push_u16_le", Check::BytesGrow(2)),
    ),
    (
        "Bytes.push_u32_be",
        Prelude::Mutating("bytes_push_u32_be", Check::BytesGrow(4)),
    ),
    (
        "Bytes.push_u32_le",
        Prelude::Mutating("bytes_push_u32_le", Check::BytesGrow(4)),
    ),
    (
        "Bytes.push_u64_be",
        Prelude::Mutating("bytes_push_u64_be", Check::BytesGrow(8)),
    ),
    (
        "Bytes.push_u64_le",
        Prelude::Mutating("bytes_push_u64_le", Check::BytesGrow(8)),
    ),
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
    if let Some(method) = name.strip_prefix(SECRET_BYTES) {
        return match prelude(&format!("Bytes.{method}")) {
            Some(Prelude::ByteRead { width, big, .. }) => Some(Prelude::ByteRead {
                width,
                big,
                secret: true,
            }),
            _ => None,
        };
    }
    PRELUDE.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

/// What the lowering prefixes a read of `Secret<Bytes>` with, in place of
/// `Bytes.`, once the checked program's call of it (such as `Secret.at`)
/// is the public read of the bytes inside (`SecretBytes.at` for
/// `Bytes.at`): a [`Prelude::ByteRead`] that is `secret`.
pub const SECRET_BYTES: &str = "SecretBytes.";

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
    // Windows' own calls, one per kernel32 or bcryptprimitives function
    // (ADR 0020): no other platform has them.
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

/// `linux-aarch64`.
pub const LINUX_AARCH64: Platform = Platform {
    table: "linux_aarch64",
    call: linux_aarch64_call,
    imports: &[],
    errno: 0,
    error: ErrorRead::Address,
    absent: -38,
    start: Start::Stack,
};

/// The Linux system call `number` gives for `call` (none for `None`),
/// with what it takes after a neutral intrinsic's arguments, on every
/// processor: `getrandom`'s flags (none), and `rt_sigaction`'s old
/// action (none kept) and the size of its signal mask in bytes.
fn linux_call(call: Sys, number: fn(Sys) -> Option<u16>) -> OsCall {
    if windows_only(call) {
        return OsCall::Absent;
    }
    let Some(number) = number(call) else {
        return OsCall::Same;
    };
    let extra_args: &'static [i64] = match call {
        Sys::Random => &[0],
        Sys::Sigaction => &[0, 8],
        _ => &[],
    };
    OsCall::Trap { number, extra_args }
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
        // Windows' own: absent (`linux_call`).
        _ => return None,
    })
}

/// `linux-x86_64`.
pub const LINUX_X86_64: Platform = Platform {
    table: "linux_x86_64",
    call: linux_x86_64_call,
    imports: &[],
    errno: 0,
    error: ErrorRead::Address,
    absent: -38,
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
        _ => return None,
    })
}

/// `macos-aarch64`: every call is a function of `libSystem`, through the
/// dynamic loader (ADR 0011, ADR 0015).
pub const MACOS_AARCH64: Platform = Platform {
    table: "macos",
    call: macos_call,
    imports: &MACOS_IMPORTS,
    errno: 0,
    error: ErrorRead::Address,
    // `ENOSYS`.
    absent: -78,
    start: Start::Main,
};

/// `macos-x86_64`: the same calls and the same `std.rt` table as
/// `macos-aarch64`, but `libSystem` on x86_64 still exports the
/// functions that take or give a `struct stat` or a `struct dirent` in
/// their old layouts under their plain names. The 64-bit-inode layouts,
/// the only ones arm64 has, are their `$INODE64` variants, which C's
/// headers select (`__DARWIN_INODE64`, `__DARWIN_ALIAS_I`).
pub const MACOS_X86_64: Platform = Platform {
    table: "macos",
    call: macos_x86_64_call,
    imports: &MACOS_X86_64_IMPORTS,
    errno: 0,
    error: ErrorRead::Address,
    // `ENOSYS`.
    absent: -78,
    start: Start::Main,
};

/// A function of `libSystem` that is not variadic and fails with -1, as
/// most do: `args` arguments and an `int` result.
const fn plain(symbol: &'static str, args: u8) -> Import {
    Import {
        library: "/usr/lib/libSystem.B.dylib",
        symbol,
        args,
        fixed: args,
        wide: false,
        fails: Fails::MinusOne,
        extra_args: &[],
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
const FDOPENDIR_INODE64: Import = Import {
    symbol: "_fdopendir$INODE64",
    ..FDOPENDIR
};
/// `readdir(dir)`: the next record, or `NULL` at the end and on failure.
const READDIR: Import = Import {
    wide: true,
    fails: Fails::NullKeepsErrno,
    ..plain("_readdir", 1)
};
const READDIR_INODE64: Import = Import {
    symbol: "_readdir$INODE64",
    ..READDIR
};
const CLOSEDIR: Import = plain("_closedir", 1);
const FSTAT: Import = plain("_fstat", 2);
const FSTAT_INODE64: Import = plain("_fstat$INODE64", 2);
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
    extra_args: &[0],
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

/// [`MACOS_IMPORTS`] with the `$INODE64` functions of x86_64's
/// `libSystem` in place of the plain ones, in the same slots.
const MACOS_X86_64_IMPORTS: [&Import; 24] = [
    &ERROR,
    &READ,
    &WRITE,
    &OPENAT,
    &CLOSE,
    &MKDIRAT,
    &FDOPENDIR_INODE64,
    &READDIR_INODE64,
    &CLOSEDIR,
    &FSTAT_INODE64,
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
    if windows_only(call) {
        return OsCall::Absent;
    }
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
        _ => return OsCall::Absent,
    })
}

/// Whether `call` is one of Windows' own, which no other platform has.
fn windows_only(call: Sys) -> bool {
    matches!(
        call,
        Sys::GetStdHandle
            | Sys::WriteFile
            | Sys::ReadFile
            | Sys::GetConsoleMode
            | Sys::WriteConsole
            | Sys::ReadConsole
            | Sys::CreateFile
            | Sys::CloseHandle
            | Sys::FinalPathName
            | Sys::SetEndOfFile
            | Sys::NtCreateFile
            | Sys::NtQueryInformationFile
            | Sys::NtQueryDirectoryFile
            | Sys::VirtualAlloc
            | Sys::VirtualFree
            | Sys::ExitProcess
            | Sys::CommandLine
            | Sys::Environment
            | Sys::AddExceptionHandler
            | Sys::StackGuarantee
            | Sys::SystemTime
            | Sys::ProcessPrng
    )
}

/// `windows-x86_64`: every call is a function of `kernel32.dll` or
/// `bcryptprimitives.dll`, through the import table, by the Windows x64
/// convention, and a failure is `-GetLastError()` (ADR 0020). The POSIX
/// calls are absent.
pub const WINDOWS_X86_64: Platform = Platform {
    table: "windows_x86_64",
    call: windows_call,
    imports: &WINDOWS_IMPORTS,
    errno: 0,
    error: ErrorRead::Value,
    // `ERROR_CALL_NOT_IMPLEMENTED`.
    absent: -120,
    start: Start::Windows,
};

const KERNEL32: &str = "KERNEL32.dll";
const NTDLL: &str = "ntdll.dll";
const BCRYPTPRIMITIVES: &str = "bcryptprimitives.dll";

/// A function of `kernel32.dll` that takes `args` of the intrinsic's
/// arguments, gives a `BOOL` and fails with `FALSE`.
const fn win(symbol: &'static str, args: u8) -> Import {
    Import {
        library: KERNEL32,
        symbol,
        args,
        fixed: args,
        wide: false,
        fails: Fails::Zero,
        extra_args: &[],
    }
}

/// `GetLastError()`: the thread's last error, no intrinsic's.
const GET_LAST_ERROR: Import = Import {
    fails: Fails::Never,
    ..win("GetLastError", 0)
};
/// `GetStdHandle(n)`: a handle, `INVALID_HANDLE_VALUE` (-1) on failure,
/// or `NULL` when the process has none.
const GET_STD_HANDLE: Import = Import {
    wide: true,
    fails: Fails::MinusOne,
    ..win("GetStdHandle", 1)
};
/// `WriteFile(h, buf, count, &done, NULL)`, synchronous.
const WRITE_FILE: Import = Import {
    fixed: 5,
    extra_args: &[0],
    ..win("WriteFile", 4)
};
/// `ReadFile(h, buf, count, &done, NULL)`, synchronous.
const READ_FILE: Import = Import {
    fixed: 5,
    extra_args: &[0],
    ..win("ReadFile", 4)
};
const GET_CONSOLE_MODE: Import = win("GetConsoleMode", 2);
/// `WriteConsoleW(h, text, count, &done, NULL)`: UTF-16 units.
const WRITE_CONSOLE: Import = Import {
    fixed: 5,
    extra_args: &[0],
    ..win("WriteConsoleW", 4)
};
/// `ReadConsoleW(h, units, count, &read, NULL)`: UTF-16 units.
const READ_CONSOLE: Import = Import {
    fixed: 5,
    extra_args: &[0],
    ..win("ReadConsoleW", 4)
};
/// `CreateFileW(name, access, share, NULL, disposition, flags, NULL)`: a
/// handle, or `INVALID_HANDLE_VALUE` (-1).
const CREATE_FILE: Import = Import {
    wide: true,
    fails: Fails::MinusOne,
    ..win("CreateFileW", 7)
};
/// `CloseHandle(h)`.
const CLOSE_HANDLE: Import = win("CloseHandle", 1);
/// `GetFinalPathNameByHandleW(h, buf, count, flags)`: the units written,
/// or 0 on failure.
const FINAL_PATH_NAME: Import = win("GetFinalPathNameByHandleW", 4);
/// `SetEndOfFile(h)`: the file ends at the handle's position.
const SET_END_OF_FILE: Import = win("SetEndOfFile", 1);
/// An `ntdll` function: an `NTSTATUS`, negative for an error, which the
/// backend turns into a Win32 error ([`Fails::Status`]).
const fn nt(symbol: &'static str, args: u8) -> Import {
    Import {
        library: NTDLL,
        fails: Fails::Status,
        ..win(symbol, args)
    }
}
/// `NtCreateFile(&h, access, &attributes, &status, NULL, attributes,
/// share, disposition, options, NULL, 0)`.
const NT_CREATE_FILE: Import = Import {
    fixed: 11,
    extra_args: &[0, 0],
    ..nt("NtCreateFile", 9)
};
/// `NtQueryInformationFile(h, &status, buf, length, class)`.
const NT_QUERY_INFORMATION_FILE: Import = nt("NtQueryInformationFile", 5);
/// `NtQueryDirectoryFile(h, NULL, NULL, NULL, &status, buf, length,
/// class, single, NULL, restart)`.
const NT_QUERY_DIRECTORY_FILE: Import = nt("NtQueryDirectoryFile", 11);
/// `RtlNtStatusToDosError(status)`: the Win32 error for a status, which
/// the backend calls after an `ntdll` function fails. No intrinsic.
pub const NT_STATUS_TO_ERROR: Import = Import {
    library: NTDLL,
    fails: Fails::Never,
    ..win("RtlNtStatusToDosError", 1)
};
/// `VirtualAlloc(addr, size, type, protect)`: an address, or `NULL`.
const VIRTUAL_ALLOC: Import = Import {
    wide: true,
    ..win("VirtualAlloc", 4)
};
/// `VirtualFree(addr, 0, MEM_RELEASE)`: the whole mapping.
const VIRTUAL_FREE: Import = Import {
    fixed: 3,
    extra_args: &[0, 0x8000],
    ..win("VirtualFree", 1)
};
const EXIT_PROCESS: Import = Import {
    fails: Fails::Never,
    ..win("ExitProcess", 1)
};
/// `GetCommandLineW()`: the process's command line, which cannot fail.
const GET_COMMAND_LINE: Import = Import {
    wide: true,
    fails: Fails::Never,
    ..win("GetCommandLineW", 0)
};
/// `GetEnvironmentStringsW()`: the environment block, or `NULL`.
const GET_ENVIRONMENT: Import = Import {
    wide: true,
    ..win("GetEnvironmentStringsW", 0)
};
/// `AddVectoredExceptionHandler(1, handler)`: the first handler called.
const ADD_EXCEPTION_HANDLER: Import = Import {
    wide: true,
    fixed: 2,
    extra_args: &[],
    ..win("AddVectoredExceptionHandler", 2)
};
/// `SetThreadStackGuarantee(&size)`.
const STACK_GUARANTEE: Import = win("SetThreadStackGuarantee", 1);
/// `GetSystemTimePreciseAsFileTime(&time)`: no result, cannot fail.
const SYSTEM_TIME: Import = Import {
    fails: Fails::Never,
    ..win("GetSystemTimePreciseAsFileTime", 1)
};
/// `ProcessPrng(buf, count)`: always `TRUE`.
const PROCESS_PRNG: Import = Import {
    library: BCRYPTPRIMITIVES,
    fails: Fails::Never,
    ..win("ProcessPrng", 2)
};

/// Every function a Windows executable imports, `kernel32.dll`'s first:
/// always all of them, so the import table depends on the target alone.
/// [`GET_LAST_ERROR`] is first ([`Platform::errno`]).
const WINDOWS_IMPORTS: [&Import; 24] = [
    &GET_LAST_ERROR,
    &GET_STD_HANDLE,
    &WRITE_FILE,
    &READ_FILE,
    &GET_CONSOLE_MODE,
    &WRITE_CONSOLE,
    &READ_CONSOLE,
    &VIRTUAL_ALLOC,
    &VIRTUAL_FREE,
    &EXIT_PROCESS,
    &GET_COMMAND_LINE,
    &GET_ENVIRONMENT,
    &ADD_EXCEPTION_HANDLER,
    &STACK_GUARANTEE,
    &SYSTEM_TIME,
    &CREATE_FILE,
    &CLOSE_HANDLE,
    &FINAL_PATH_NAME,
    &SET_END_OF_FILE,
    &NT_CREATE_FILE,
    &NT_QUERY_INFORMATION_FILE,
    &NT_QUERY_DIRECTORY_FILE,
    &NT_STATUS_TO_ERROR,
    &PROCESS_PRNG,
];

/// The function behind each of Windows' own calls; the POSIX calls are
/// absent (ADR 0020).
fn windows_call(call: Sys) -> OsCall {
    OsCall::Library(match call {
        Sys::GetStdHandle => &GET_STD_HANDLE,
        Sys::WriteFile => &WRITE_FILE,
        Sys::ReadFile => &READ_FILE,
        Sys::GetConsoleMode => &GET_CONSOLE_MODE,
        Sys::WriteConsole => &WRITE_CONSOLE,
        Sys::ReadConsole => &READ_CONSOLE,
        Sys::VirtualAlloc => &VIRTUAL_ALLOC,
        Sys::VirtualFree => &VIRTUAL_FREE,
        Sys::ExitProcess => &EXIT_PROCESS,
        Sys::CommandLine => &GET_COMMAND_LINE,
        Sys::Environment => &GET_ENVIRONMENT,
        Sys::AddExceptionHandler => &ADD_EXCEPTION_HANDLER,
        Sys::StackGuarantee => &STACK_GUARANTEE,
        Sys::SystemTime => &SYSTEM_TIME,
        Sys::CreateFile => &CREATE_FILE,
        Sys::CloseHandle => &CLOSE_HANDLE,
        Sys::FinalPathName => &FINAL_PATH_NAME,
        Sys::SetEndOfFile => &SET_END_OF_FILE,
        Sys::NtCreateFile => &NT_CREATE_FILE,
        Sys::NtQueryInformationFile => &NT_QUERY_INFORMATION_FILE,
        Sys::NtQueryDirectoryFile => &NT_QUERY_DIRECTORY_FILE,
        Sys::ProcessPrng => &PROCESS_PRNG,
        _ => return OsCall::Absent,
    })
}

/// The `libSystem` function behind each call on x86_64 macOS: arm64's,
/// but the `$INODE64` variant where there is one.
fn macos_x86_64_call(call: Sys) -> OsCall {
    match call {
        Sys::OpenDir => OsCall::Library(&FDOPENDIR_INODE64),
        Sys::ReadDir => OsCall::Library(&READDIR_INODE64),
        Sys::Fstat => OsCall::Library(&FSTAT_INODE64),
        _ => macos_call(call),
    }
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
    ("rt_sys_get_std_handle", Runtime::Syscall(Sys::GetStdHandle)),
    ("rt_sys_write_file", Runtime::Syscall(Sys::WriteFile)),
    ("rt_sys_read_file", Runtime::Syscall(Sys::ReadFile)),
    (
        "rt_sys_get_console_mode",
        Runtime::Syscall(Sys::GetConsoleMode),
    ),
    ("rt_sys_write_console", Runtime::Syscall(Sys::WriteConsole)),
    ("rt_sys_read_console", Runtime::Syscall(Sys::ReadConsole)),
    ("rt_sys_create_file", Runtime::Syscall(Sys::CreateFile)),
    ("rt_sys_close_handle", Runtime::Syscall(Sys::CloseHandle)),
    (
        "rt_sys_final_path_name",
        Runtime::Syscall(Sys::FinalPathName),
    ),
    (
        "rt_sys_set_end_of_file",
        Runtime::Syscall(Sys::SetEndOfFile),
    ),
    ("rt_sys_nt_create_file", Runtime::Syscall(Sys::NtCreateFile)),
    (
        "rt_sys_nt_query_information_file",
        Runtime::Syscall(Sys::NtQueryInformationFile),
    ),
    (
        "rt_sys_nt_query_directory_file",
        Runtime::Syscall(Sys::NtQueryDirectoryFile),
    ),
    ("rt_sys_virtual_alloc", Runtime::Syscall(Sys::VirtualAlloc)),
    ("rt_sys_virtual_free", Runtime::Syscall(Sys::VirtualFree)),
    ("rt_sys_exit_process", Runtime::Syscall(Sys::ExitProcess)),
    ("rt_sys_command_line", Runtime::Syscall(Sys::CommandLine)),
    ("rt_sys_environment", Runtime::Syscall(Sys::Environment)),
    (
        "rt_sys_add_exception_handler",
        Runtime::Syscall(Sys::AddExceptionHandler),
    ),
    (
        "rt_sys_stack_guarantee",
        Runtime::Syscall(Sys::StackGuarantee),
    ),
    ("rt_sys_system_time", Runtime::Syscall(Sys::SystemTime)),
    ("rt_sys_process_prng", Runtime::Syscall(Sys::ProcessPrng)),
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
    ("rt_mem_conceal", Runtime::Conceal),
    ("rt_mem_expose", Runtime::Expose),
    ("rt_mem_text", Runtime::Identity),
    ("rt_mem_as_text", Runtime::Identity),
    ("rt_mem_secret", Runtime::Identity),
    ("rt_mem_as_secret", Runtime::Identity),
    ("rt_mem_as_io_error", Runtime::Identity),
    ("rt_mem_path", Runtime::Identity),
    ("rt_mem_as_path", Runtime::Identity),
    ("rt_mem_list", Runtime::Identity),
    ("rt_mem_as_list", Runtime::Identity),
    ("rt_mem_bytes", Runtime::Identity),
    ("rt_mem_as_bytes", Runtime::Identity),
    ("rt_mem_as_root", Runtime::Identity),
    ("rt_mem_globals", Runtime::Globals),
    ("rt_mem_depth", Runtime::Depth),
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
                let expected = if windows_only(*call) {
                    OsCall::Absent
                } else if *name == "rt_sys_open_dir" {
                    OsCall::Same
                } else {
                    let (_, aarch64, x86_64, extra_args) = rows
                        .iter()
                        .find(|row| row.0 == *name)
                        .unwrap_or_else(|| panic!("`{name}` has no row"));
                    let number = if platform.table == "linux_aarch64" {
                        *aarch64
                    } else {
                        *x86_64
                    };
                    OsCall::Trap { number, extra_args }
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
        let module = crate::lower::program(
            &program,
            &rt,
            &crate::Unplaced,
            crate::lower::Entry::Main,
            target,
            oska_check::limits::Limits::NONE,
        )
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

    /// How macOS makes an intrinsic's call: (intrinsic, symbol, arguments
    /// it takes, fixed arguments, whole-word result, how it fails,
    /// trailing constants).
    type Row = (
        &'static str,
        &'static str,
        u8,
        u8,
        bool,
        Fails,
        &'static [i64],
    );

    /// Each macOS call is one `libSystem` function, made as Apple's
    /// headers declare it.
    #[test]
    fn macos_makes_each_call_as_the_libsystem_function_declares_it() {
        use Fails::*;
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
        // x86_64's `libSystem` gives the 64-bit-inode `struct stat` and
        // `struct dirent`, arm64's only ones, under `$INODE64` names.
        for (platform, inode64) in [(MACOS_AARCH64, ""), (MACOS_X86_64, "$INODE64")] {
            macos_calls(&platform, inode64, rows);
        }
    }

    /// Each row of `rows` is how `platform` makes its intrinsic's call,
    /// with `inode64` after the symbol of a function that takes or gives
    /// a `struct stat` or `struct dirent`.
    fn macos_calls(platform: &Platform, inode64: &str, rows: &[Row]) {
        for (name, implementation) in RUNTIME {
            let Runtime::Syscall(call) = implementation else {
                continue;
            };
            if windows_only(*call) {
                assert_eq!((platform.call)(*call), OsCall::Absent, "`{name}`");
                continue;
            }
            let row = rows
                .iter()
                .find(|row| row.0 == *name)
                .unwrap_or_else(|| panic!("`{name}` has no row"));
            let symbol = match *name {
                "rt_sys_open_dir" | "rt_sys_read_dir" | "rt_sys_fstat" => {
                    format!("{}{inode64}", row.1)
                }
                _ => row.1.to_string(),
            };
            let OsCall::Library(import) = (platform.call)(*call) else {
                panic!("`{name}` is not a libSystem call on macOS");
            };
            assert_eq!(
                (
                    import.symbol,
                    import.args,
                    import.fixed,
                    import.wide,
                    import.fails,
                    import.extra_args
                ),
                (symbol.as_str(), row.2, row.3, row.4, row.5, row.6),
                "`{name}`"
            );
            assert!(
                usize::from(import.args) + import.extra_args.len() >= usize::from(import.fixed),
                "`{name}`"
            );
            assert!(platform.slot(import).is_some(), "`{name}` is imported");
        }
        let errno = platform.imports[usize::from(platform.errno)];
        assert_eq!(errno.symbol, "___error");
        // Every import is some call's, or `__error`'s, and none twice.
        assert_eq!(platform.imports.len(), rows.len() + 1);
        let mut symbols: Vec<&str> = platform.imports.iter().map(|i| i.symbol).collect();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), platform.imports.len());
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
        for (target, platform) in [
            (crate::Target::MacosAarch64, MACOS_AARCH64),
            (crate::Target::MacosX86_64, MACOS_X86_64),
        ] {
            let module = crate::lower::program(
                &program,
                &rt,
                &crate::Unplaced,
                crate::lower::Entry::Main,
                target,
                oska_check::limits::Limits::NONE,
            )
            .expect("the program should lower");
            let mut imports = std::collections::BTreeSet::new();
            for inst in module.functions.iter().flat_map(|f| &f.insts) {
                match inst {
                    crate::ir::Inst::Syscall { .. } | crate::ir::Inst::EntrySp { .. } => {
                        panic!("a {target:?} executable has {inst:?}")
                    }
                    crate::ir::Inst::LibCall { import, .. } => {
                        imports.insert(*import);
                    }
                    _ => {}
                }
            }
            assert_eq!(imports.len(), platform.imports.len(), "{target:?}");
        }
    }

    /// How Windows makes an intrinsic's call: (intrinsic, library,
    /// symbol, arguments it takes, whole-word result, how it fails,
    /// trailing constants).
    type WindowsRow = (
        &'static str,
        &'static str,
        &'static str,
        u8,
        bool,
        Fails,
        &'static [i64],
    );

    /// Each of Windows' own calls is one function of `kernel32.dll` (or
    /// `bcryptprimitives.dll`), made as Microsoft's documentation
    /// declares it, and every POSIX call is absent (ADR 0020):
    /// (intrinsic, library, symbol, arguments it takes, whole-word
    /// result, how it fails, trailing constants).
    #[test]
    fn windows_makes_each_call_as_kernel32_declares_it() {
        use Fails::*;
        const K: &str = "KERNEL32.dll";
        let rows: &[WindowsRow] = &[
            // GetStdHandle: INVALID_HANDLE_VALUE on failure.
            (
                "rt_sys_get_std_handle",
                K,
                "GetStdHandle",
                1,
                true,
                MinusOne,
                &[],
            ),
            // WriteFile and ReadFile: no OVERLAPPED.
            ("rt_sys_write_file", K, "WriteFile", 4, false, Zero, &[0]),
            ("rt_sys_read_file", K, "ReadFile", 4, false, Zero, &[0]),
            (
                "rt_sys_get_console_mode",
                K,
                "GetConsoleMode",
                2,
                false,
                Zero,
                &[],
            ),
            (
                "rt_sys_write_console",
                K,
                "WriteConsoleW",
                4,
                false,
                Zero,
                &[0],
            ),
            (
                "rt_sys_read_console",
                K,
                "ReadConsoleW",
                4,
                false,
                Zero,
                &[0],
            ),
            (
                "rt_sys_virtual_alloc",
                K,
                "VirtualAlloc",
                4,
                true,
                Zero,
                &[],
            ),
            // VirtualFree(addr, 0, MEM_RELEASE).
            (
                "rt_sys_virtual_free",
                K,
                "VirtualFree",
                1,
                false,
                Zero,
                &[0, 0x8000],
            ),
            (
                "rt_sys_exit_process",
                K,
                "ExitProcess",
                1,
                false,
                Never,
                &[],
            ),
            (
                "rt_sys_command_line",
                K,
                "GetCommandLineW",
                0,
                true,
                Never,
                &[],
            ),
            (
                "rt_sys_environment",
                K,
                "GetEnvironmentStringsW",
                0,
                true,
                Zero,
                &[],
            ),
            (
                "rt_sys_add_exception_handler",
                K,
                "AddVectoredExceptionHandler",
                2,
                true,
                Zero,
                &[],
            ),
            (
                "rt_sys_stack_guarantee",
                K,
                "SetThreadStackGuarantee",
                1,
                false,
                Zero,
                &[],
            ),
            (
                "rt_sys_system_time",
                K,
                "GetSystemTimePreciseAsFileTime",
                1,
                false,
                Never,
                &[],
            ),
            (
                "rt_sys_create_file",
                K,
                "CreateFileW",
                7,
                true,
                MinusOne,
                &[],
            ),
            ("rt_sys_close_handle", K, "CloseHandle", 1, false, Zero, &[]),
            (
                "rt_sys_final_path_name",
                K,
                "GetFinalPathNameByHandleW",
                4,
                false,
                Zero,
                &[],
            ),
            (
                "rt_sys_set_end_of_file",
                K,
                "SetEndOfFile",
                1,
                false,
                Zero,
                &[],
            ),
            // NtCreateFile: no extended attributes.
            (
                "rt_sys_nt_create_file",
                "ntdll.dll",
                "NtCreateFile",
                9,
                false,
                Status,
                &[0, 0],
            ),
            (
                "rt_sys_nt_query_information_file",
                "ntdll.dll",
                "NtQueryInformationFile",
                5,
                false,
                Status,
                &[],
            ),
            (
                "rt_sys_nt_query_directory_file",
                "ntdll.dll",
                "NtQueryDirectoryFile",
                11,
                false,
                Status,
                &[],
            ),
            // ProcessPrng: "Always returns TRUE".
            (
                "rt_sys_process_prng",
                "bcryptprimitives.dll",
                "ProcessPrng",
                2,
                false,
                Never,
                &[],
            ),
        ];
        let platform = WINDOWS_X86_64;
        for (name, implementation) in RUNTIME {
            let Runtime::Syscall(call) = implementation else {
                continue;
            };
            if !windows_only(*call) {
                assert_eq!((platform.call)(*call), OsCall::Absent, "`{name}`");
                continue;
            }
            let row = rows
                .iter()
                .find(|row| row.0 == *name)
                .unwrap_or_else(|| panic!("`{name}` has no row"));
            let OsCall::Library(import) = (platform.call)(*call) else {
                panic!("`{name}` is not a library call on Windows");
            };
            assert_eq!(
                (
                    import.library,
                    import.symbol,
                    import.args,
                    import.wide,
                    import.fails,
                    import.extra_args
                ),
                (row.1, row.2, row.3, row.4, row.5, row.6),
                "`{name}`"
            );
            // No Windows function is variadic.
            assert_eq!(
                usize::from(import.fixed),
                usize::from(import.args) + import.extra_args.len(),
                "`{name}`"
            );
            assert!(platform.slot(import).is_some(), "`{name}` is imported");
        }
        let error = platform.imports[usize::from(platform.errno)];
        assert_eq!(
            (error.symbol, platform.error),
            ("GetLastError", ErrorRead::Value)
        );
        // `RtlNtStatusToDosError`, which the backend calls after an
        // `ntdll` function fails, is the one other import no intrinsic
        // names.
        assert_eq!(
            (NT_STATUS_TO_ERROR.library, NT_STATUS_TO_ERROR.symbol),
            ("ntdll.dll", "RtlNtStatusToDosError")
        );
        assert!(platform.slot(&NT_STATUS_TO_ERROR).is_some());
        assert_eq!(platform.imports.len(), rows.len() + 2);
        // Each library's imports are together, so each has one run of
        // slots in the import address table, ended by a null slot.
        let libraries: Vec<&str> = platform.imports.iter().map(|i| i.library).collect();
        let mut runs = libraries.clone();
        runs.dedup();
        let mut distinct = runs.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(runs.len(), distinct.len());
        let last = platform.imports.len() - 1;
        assert_eq!(
            platform.slot_of(last),
            u16::try_from(last + runs.len() - 1).ok()
        );
    }

    /// A Windows executable makes no system call and reaches every
    /// import, the program's start through the fault handler's entry.
    #[test]
    fn windows_executables_call_only_their_imports() {
        let report = oska_check::check_module(EVERY_CALL);
        let program = report.program.expect("the program should check");
        let rt = oska_check::check_toolchain_rt()
            .program
            .expect("std.rt should check");
        let module = crate::lower::program(
            &program,
            &rt,
            &crate::Unplaced,
            crate::lower::Entry::Main,
            crate::Target::WindowsX86_64,
            oska_check::limits::Limits::NONE,
        )
        .expect("the program should lower");
        assert!(module.callback.is_some());
        for inst in module.functions.iter().flat_map(|f| &f.insts) {
            assert!(
                !matches!(
                    inst,
                    crate::ir::Inst::Syscall { .. } | crate::ir::Inst::EntrySp { .. }
                ),
                "a Windows executable has {inst:?}"
            );
        }
    }

    /// The `std.rt` function a prelude intrinsic calls, if it calls one.
    fn runtime_function(implementation: Prelude) -> Option<&'static str> {
        match implementation {
            Prelude::Runtime(f)
            | Prelude::WriteLine(f)
            | Prelude::Checked(f, _)
            | Prelude::Mutating(f, _) => Some(f),
            Prelude::ByteRead { secret: false, .. } => Some("bytes_read"),
            Prelude::ByteRead { secret: true, .. } => Some("bytes_read_secret"),
            Prelude::Identity | Prelude::Field(_) => None,
        }
    }

    /// The `std.rt` functions that write the program's own text to
    /// stdout or stderr: every other write is of a panic's report, or of
    /// bytes already counted (a flush).
    const OUTPUT_SINKS: [&str; 2] = ["stdout_put_line", "print_error_line"];

    /// The `std.rt` functions that write only the runtime's own words:
    /// `exit` reports a failed flush of stdout in its own wording.
    const OWN_REPORTS: [&str; 1] = ["exit"];

    /// Each `std.rt` function by name, with the names of the `std.rt`
    /// functions its body calls, read from the source.
    fn rt_calls() -> std::collections::BTreeMap<String, Vec<String>> {
        let mut bodies: Vec<(String, String)> = Vec::new();
        for line in oska_check::STD_RT_SOURCE.lines() {
            let head = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("fn "));
            match head.and_then(|h| h.split_once('(')) {
                Some((name, _)) => bodies.push((name.to_string(), String::new())),
                None => {
                    if let Some((_, body)) = bodies.last_mut() {
                        body.push_str(line);
                        body.push('\n');
                    }
                }
            }
        }
        let names: Vec<String> = bodies.iter().map(|(n, _)| n.clone()).collect();
        bodies
            .into_iter()
            .map(|(name, body)| {
                let called = names
                    .iter()
                    .filter(|callee| {
                        body.match_indices(&format!("{callee}(")).any(|(at, _)| {
                            !body[..at]
                                .chars()
                                .next_back()
                                .is_some_and(|c| c.is_alphanumeric() || c == '_')
                        })
                    })
                    .cloned()
                    .collect();
                (name, called)
            })
            .collect()
    }

    /// Whether the `std.rt` function `function` can reach an output sink.
    fn writes_output(
        calls: &std::collections::BTreeMap<String, Vec<String>>,
        function: &str,
    ) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![function.to_string()];
        while let Some(at) = stack.pop() {
            if OUTPUT_SINKS.contains(&at.as_str()) {
                return true;
            }
            if !OWN_REPORTS.contains(&at.as_str()) && seen.insert(at.clone()) {
                stack.extend(calls.get(&at).into_iter().flatten().cloned());
            }
        }
        false
    }

    /// `--max-output` fails closed: a prelude intrinsic whose `std.rt`
    /// function can write the program's text to stdout or stderr is a
    /// [`Prelude::WriteLine`], which counts the bytes first, and the
    /// lowering calls no other such function directly but `dbg_write`
    /// and `main_failed`, which it counts at the call (`spend_output`).
    #[test]
    fn every_write_of_the_programs_text_is_counted_against_max_output() {
        let calls = rt_calls();
        let mut write_lines = 0;
        for (name, implementation) in PRELUDE {
            let Some(function) = runtime_function(*implementation) else {
                continue;
            };
            let counted = matches!(implementation, Prelude::WriteLine(_));
            write_lines += usize::from(counted);
            assert_eq!(
                writes_output(&calls, function),
                counted,
                "`{name}` (`{function}`) writes output exactly when it is a `Prelude::WriteLine`"
            );
        }
        assert_eq!(write_lines, 2, "`Stdout.println` and `Stderr.println`");
        let lower = [
            include_str!("lower.rs"),
            include_str!("lower/closures.rs"),
            include_str!("lower/data.rs"),
            include_str!("lower/debug.rs"),
            include_str!("lower/html.rs"),
            include_str!("lower/lists.rs"),
            include_str!("lower/patterns.rs"),
            include_str!("lower/tasks.rs"),
            include_str!("lower/tests.rs"),
            include_str!("lower/traits.rs"),
        ];
        let mut direct = std::collections::BTreeSet::new();
        for source in lower {
            for (at, _) in source.match_indices("call_runtime(\"") {
                let rest = &source[at + "call_runtime(\"".len()..];
                let function = &rest[..rest.find('"').expect("a closing quote")];
                if writes_output(&calls, function) {
                    direct.insert(function);
                }
            }
        }
        assert_eq!(
            direct,
            std::collections::BTreeSet::from(["dbg_write", "main_failed"]),
            "the lowering writes output only through these, which it counts"
        );
    }

    /// A `Bytes` read fails closed on its own: the generated code panics
    /// when `bytes_readable` says the bytes do not all lie inside the
    /// buffer, and `bytes_read` checks again rather than trust that it
    /// did, so no call of it can read past the buffer.
    #[test]
    fn the_std_rt_function_of_every_bytes_read_checks_its_own_bounds() {
        let calls = rt_calls();
        for (name, implementation) in PRELUDE {
            if let Prelude::ByteRead { .. } = implementation {
                let function = runtime_function(*implementation).expect("a function");
                assert!(
                    calls
                        .get(function)
                        .is_some_and(|called| called.iter().any(|c| c == "bytes_readable")),
                    "`{name}` reads through `{function}`, which does not check its bounds"
                );
            }
        }
    }

    #[test]
    fn every_runtime_function_a_prelude_intrinsic_names_is_in_std_rt() {
        let report = oska_check::check_toolchain_rt();
        let program = report.program.expect("std.rt should check");
        for (name, implementation) in PRELUDE {
            if let Some(function) = runtime_function(*implementation) {
                assert!(
                    program.functions.contains_key(function),
                    "`{name}` names `{function}`, which std.rt lacks"
                );
            }
        }
    }
}
