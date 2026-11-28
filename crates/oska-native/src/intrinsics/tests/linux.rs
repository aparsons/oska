// SPDX-License-Identifier: Apache-2.0
//! Linux.

use super::*;

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
        ("rt_sys_unlinkat", 35, 263, &[]),
        // renameat2(old dir, old, new dir, new, flags): no flags.
        ("rt_sys_renameat", 276, 316, &[0]),
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
        ("rt_sys_socket", 198, 41, &[]),
        ("rt_sys_connect", 203, 42, &[]),
        ("rt_sys_bind", 200, 49, &[]),
        ("rt_sys_listen", 201, 50, &[]),
        // accept4(fd, 0, 0, flags): no flags.
        ("rt_sys_accept", 242, 288, &[0]),
        ("rt_sys_getsockname", 204, 51, &[]),
        ("rt_sys_getsockopt", 209, 55, &[]),
        ("rt_sys_setsockopt", 208, 54, &[]),
        // sendto(fd, buf, count, MSG_NOSIGNAL, 0, 0).
        ("rt_sys_send", 206, 44, &[0x4000, 0, 0]),
        // recvfrom(fd, buf, count, 0, 0, 0).
        ("rt_sys_recv", 207, 45, &[0, 0, 0]),
        // ppoll(entries, count, timeout, 0, 8): no signal mask.
        ("rt_sys_poll", 73, 271, &[0, 8]),
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
        false,
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
        (35, 3),
        (46, 2),
        (56, 4),
        (57, 1),
        (61, 3),
        (63, 3),
        (64, 3),
        (73, 5),
        (78, 4),
        (80, 2),
        (94, 1),
        (113, 2),
        (129, 2),
        (132, 2),
        (134, 4),
        (172, 0),
        (198, 3),
        (200, 3),
        (201, 2),
        (203, 3),
        (204, 3),
        (206, 6),
        (207, 6),
        (208, 5),
        (209, 5),
        (215, 2),
        (222, 6),
        (242, 4),
        (276, 5),
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
        (41, 3),
        (42, 3),
        (44, 6),
        (45, 6),
        (49, 3),
        (50, 2),
        (51, 3),
        (54, 5),
        (55, 5),
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
        (263, 3),
        (267, 4),
        (271, 5),
        (288, 4),
        (316, 5),
        (318, 3),
    ];
    assert_eq!(system_calls(crate::Target::LinuxAarch64), aarch64);
    assert_eq!(system_calls(crate::Target::LinuxX86_64), x86_64);
}
