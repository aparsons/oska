// SPDX-License-Identifier: Apache-2.0
//! The macOS platforms: `libSystem` calls (ADR 0015).

use super::windows::windows_only;
use super::*;

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
const UNLINKAT: Import = plain("_unlinkat", 3);
const RENAMEAT: Import = plain("_renameat", 4);
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

const SOCKET: Import = plain("_socket", 3);
const CONNECT: Import = plain("_connect", 3);
const BIND: Import = plain("_bind", 3);
const LISTEN: Import = plain("_listen", 2);
const ACCEPT: Import = plain("_accept", 3);
const GETSOCKNAME: Import = plain("_getsockname", 3);
const GETSOCKOPT: Import = plain("_getsockopt", 5);
const SETSOCKOPT: Import = plain("_setsockopt", 5);
/// `send(fd, buf, count, flags)`: no flags. A write to a closed connection
/// fails with `EPIPE` because the runtime ignores `SIGPIPE`.
const SEND: Import = Import {
    wide: true,
    fixed: 4,
    extra_args: &[0],
    ..plain("_send", 3)
};
/// `recv(fd, buf, count, flags)`: no flags.
const RECV: Import = Import {
    wide: true,
    fixed: 4,
    extra_args: &[0],
    ..plain("_recv", 3)
};
/// `poll(entries, count, milliseconds)`.
const POLL: Import = plain("_poll", 3);

/// Every function a macOS executable imports from `libSystem`: always
/// all of them, whatever the program calls, so the import table depends
/// on the target alone. [`ERROR`] is first ([`Platform::errno`]).
const MACOS_IMPORTS: [&Import; 37] = [
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
    &UNLINKAT,
    &RENAMEAT,
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
    &SOCKET,
    &CONNECT,
    &BIND,
    &LISTEN,
    &ACCEPT,
    &GETSOCKNAME,
    &GETSOCKOPT,
    &SETSOCKOPT,
    &SEND,
    &RECV,
    &POLL,
];

/// [`MACOS_IMPORTS`] with the `$INODE64` functions of x86_64's
/// `libSystem` in place of the plain ones, in the same slots.
const MACOS_X86_64_IMPORTS: [&Import; 37] = [
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
    &UNLINKAT,
    &RENAMEAT,
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
    &SOCKET,
    &CONNECT,
    &BIND,
    &LISTEN,
    &ACCEPT,
    &GETSOCKNAME,
    &GETSOCKOPT,
    &SETSOCKOPT,
    &SEND,
    &RECV,
    &POLL,
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
        Sys::Unlinkat => &UNLINKAT,
        Sys::Renameat => &RENAMEAT,
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
        Sys::Socket => &SOCKET,
        Sys::Connect => &CONNECT,
        Sys::Bind => &BIND,
        Sys::Listen => &LISTEN,
        Sys::Accept => &ACCEPT,
        Sys::Getsockname => &GETSOCKNAME,
        Sys::Getsockopt => &GETSOCKOPT,
        Sys::Setsockopt => &SETSOCKOPT,
        Sys::Send => &SEND,
        Sys::Recv => &RECV,
        Sys::Poll => &POLL,
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
