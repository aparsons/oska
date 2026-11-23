// SPDX-License-Identifier: Apache-2.0
//! The values every Linux table of `std.rt` holds whatever the processor
//! (#168, #366): the open flags but `O_DIRECTORY` and `O_NOFOLLOW`,
//! `AT_FDCWD`, `F_DUPFD_CLOEXEC`, `TCGETS` and `struct termios`, `mmap`'s
//! flags, the directory records, `struct sigaction`, `stack_t`, `SIGBUS`
//! and `ELOOP`. They are the generic UAPI headers' values, which arm64 and
//! x86 both take as they are, so a wrong entry fails here and not only on
//! the platform's runner. The processor's own values are checked in
//! `std_rt_platform.rs`. Each table also says where Linux's errors are
//! worded in `system_errors`, which `oskac`'s `std_rt_errors.rs` checks.

mod common;

use common::platform::{fields_of, owned, table, LINUX_PROCESSOR_FIELDS};

const LINUX_TABLES: [&str; 2] = ["linux_aarch64", "linux_x86_64"];

/// What every Linux table holds beyond the processor's open flags and
/// `struct stat`, in the table's order.
fn linux_shared() -> Vec<(&'static str, i128)> {
    vec![
        // asm-generic/fcntl.h.
        ("o_wronly", 0o1),
        ("o_creat", 0o100),
        ("o_nonblock", 0o4000),
        ("o_cloexec", 0o2000000),
        // linux/fcntl.h: AT_FDCWD, and F_DUPFD_CLOEXEC
        // (F_LINUX_SPECIFIC_BASE + 6).
        ("at_fdcwd", -100),
        ("f_dupfd_cloexec", 1030),
        // asm-generic/ioctls.h: TCGETS.
        ("tty_request", 0x5401),
        // asm-generic/termbits.h, `struct termios`, which TCGETS fills:
        // c_iflag, c_oflag, c_cflag, c_lflag (4 bytes each), c_line, and
        // c_cc[NCCS], NCCS being 19.
        ("termios_bytes", 4 * 4 + 1 + 19),
        // asm-generic/mman-common.h and mman.h: MAP_PRIVATE |
        // MAP_ANONYMOUS, and MAP_NORESERVE.
        ("map_private_anonymous", 0x02 | 0x20),
        ("map_noreserve", 0x4000),
        // linux/dirent.h, `struct linux_dirent64`: d_ino, d_off (8 bytes
        // each), d_reclen (2), d_type (1), d_name.
        ("dirent_length", 16),
        ("dirent_name", 19),
        // getdents64 fills the buffer with records: false.
        ("dirent_one_at_a_time", 0),
        // The kernel's `struct sigaction`: sa_handler, sa_flags,
        // sa_restorer, sa_mask (8 bytes each); a fault's flags are
        // SA_ONSTACK | SA_RESETHAND | SA_NODEFER | SA_RESTORER
        // (asm-generic/signal-defs.h, and SA_RESTORER from arm64's and
        // x86's asm/signal.h).
        ("sigaction_bytes", 32),
        ("sigaction_flags", 8),
        (
            "sa_fault_flags",
            0x0800_0000 | 0x8000_0000 | 0x4000_0000 | 0x0400_0000,
        ),
        // asm-generic/signal.h, `stack_t`: ss_sp, ss_flags (4 bytes and
        // padding), ss_size.
        ("ss_size_at", 16),
        ("ss_flags_at", 8),
        // asm-generic/signal.h: SIGBUS; asm-generic/errno.h: ELOOP.
        ("sigbus", 7),
        ("e_loop", 40),
        // Linux's errors 1 to 133 are worded first in `system_errors`,
        // which `oskac`'s `std_rt_errors.rs` checks against `oska-sys`.
        ("errors_at", 0),
        ("errors_count", 133),
        // Not Windows (ADR 0020); asm-generic/errno-base.h: EINTR and
        // EPIPE, with no second broken-pipe error.
        ("windows", 0),
        ("e_intr", 4),
        ("e_pipe", 32),
        ("e_pipe_alt", 0),
    ]
}

#[test]
fn every_linux_table_holds_the_values_linux_processors_share() {
    let shared = linux_shared();
    let names: Vec<&str> = shared.iter().map(|(field, _)| *field).collect();
    for name in LINUX_TABLES {
        assert_eq!(fields_of(name, &names), owned(&shared), "{name}");
    }
}

#[test]
fn every_field_of_a_linux_table_is_checked() {
    let shared = linux_shared();
    for name in LINUX_TABLES {
        for (field, _) in table(name) {
            assert!(
                LINUX_PROCESSOR_FIELDS.contains(&field.as_str())
                    || shared.iter().any(|(known, _)| *known == field),
                "{name}'s `{field}` is checked by no test"
            );
        }
    }
}
