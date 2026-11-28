// SPDX-License-Identifier: Apache-2.0
//! The Linux platforms: system calls (ADR 0013).

use super::windows::windows_only;
use super::*;

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
        // renameat2(olddirfd, old, newdirfd, new, flags): no flags.
        Sys::Renameat => &[0],
        Sys::Sigaction => &[0, 8],
        // accept4(fd, 0, 0, flags): no flags.
        Sys::Accept => &[0],
        // sendto(fd, buf, count, flags, 0, 0): `MSG_NOSIGNAL`, so a send
        // to a closed connection fails with `EPIPE` and raises no signal.
        Sys::Send => &[16384, 0, 0],
        // recvfrom(fd, buf, count, 0, 0, 0): no flags, no sender.
        Sys::Recv => &[0, 0, 0],
        // ppoll(entries, count, timeout, 0, 8): no signal mask, of 8 bytes.
        Sys::Poll => &[0, 8],
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
        Sys::Unlinkat => 35,
        Sys::Renameat => 276,
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
        Sys::Socket => 198,
        Sys::Connect => 203,
        Sys::Bind => 200,
        Sys::Listen => 201,
        Sys::Accept => 242,
        Sys::Getsockname => 204,
        Sys::Getsockopt => 209,
        Sys::Setsockopt => 208,
        Sys::Send => 206,
        Sys::Recv => 207,
        Sys::Poll => 73,
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
        Sys::Unlinkat => 263,
        Sys::Renameat => 316,
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
        Sys::Socket => 41,
        Sys::Connect => 42,
        Sys::Bind => 49,
        Sys::Listen => 50,
        Sys::Accept => 288,
        Sys::Getsockname => 51,
        Sys::Getsockopt => 55,
        Sys::Setsockopt => 54,
        Sys::Send => 44,
        Sys::Recv => 45,
        Sys::Poll => 271,
        _ => return None,
    })
}
