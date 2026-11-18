//! What `std.rt` reads that differs between operating systems or
//! processors (open flags, `mmap` flags, `struct stat`, the directory
//! records, `struct sigaction`, `stack_t`, `struct termios`, `SIGBUS` and
//! `ELOOP`) is in one table per target platform, which the constant
//! intrinsic `rt_mem_platform()` gives (#158, #168, #366). The values
//! below are the Linux UAPI headers' for each platform, so a wrong entry
//! fails here and not only on the platform's runner.

use oska_check::program::{ExprKind, Program, TypeDeclKind};

/// What every Linux table holds after the processor's open flags,
/// whatever the processor: the generic headers' values, which arm64 and
/// x86 both take as they are.
fn linux_common() -> Vec<(&'static str, i128)> {
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
    ]
}

/// What every Linux table holds after `struct stat`.
fn linux_common_records() -> Vec<(&'static str, i128)> {
    vec![
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
        ("stack_size", 16),
        // asm-generic/signal.h: SIGBUS; asm-generic/errno.h: ELOOP.
        ("sigbus", 7),
        ("e_loop", 40),
    ]
}

/// A Linux table: the processor's open flags and `struct stat` among the
/// values every Linux processor shares.
fn linux(flags: [(&'static str, i128); 2], stat: [(&'static str, i128); 6]) -> Vec<(String, i128)> {
    flags
        .into_iter()
        .chain(linux_common())
        .chain(stat)
        .chain(linux_common_records())
        .map(|(field, value)| (field.to_string(), value))
        .collect()
}

/// The table the `std.rt` function `name` gives, by field name.
fn table(program: &Program, name: &str) -> Vec<(String, i128)> {
    let function = program
        .functions
        .get(name)
        .unwrap_or_else(|| panic!("std.rt has no table `{name}`"));
    assert_eq!(function.ret.to_string(), "Platform");
    let mut body = &function.body;
    while let ExprKind::Block {
        stmts,
        value: Some(value),
    } = &body.kind
    {
        assert!(stmts.is_empty(), "`{name}` does more than give its table");
        body = value;
    }
    let ExprKind::Construct { fields, .. } = &body.kind else {
        panic!("`{name}` gives no `Platform(...)`");
    };
    let Some(TypeDeclKind::Struct(names)) = program.types.get("Platform").map(|t| &t.kind) else {
        panic!("std.rt has no struct `Platform`");
    };
    names
        .iter()
        .zip(fields)
        .map(|(field, value)| match &value.kind {
            ExprKind::Int(n, _) => (field.name.clone().unwrap_or_default(), *n),
            ExprKind::Bool(b) => (field.name.clone().unwrap_or_default(), i128::from(*b)),
            _ => panic!("`{name}`'s `{:?}` is not a constant", field.name),
        })
        .collect()
}

#[test]
fn linux_aarch64s_table_holds_linuxs_values_with_arm64s_flags_and_stat() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    let expected = linux(
        [
            // asm-generic/fcntl.h, as arm64's asm/fcntl.h overrides it.
            ("o_directory", 0o40000),
            ("o_nofollow", 0o100000),
        ],
        [
            // asm-generic/stat.h: st_dev, st_ino (8 bytes each), st_mode,
            // st_nlink (4 each), st_uid, st_gid, st_rdev, __pad1, st_size.
            ("stat_bytes", 128),
            ("stat_mode", 16),
            ("stat_nlink", 20),
            ("stat_size", 48),
            // st_mode and st_nlink are `unsigned int`s.
            ("stat_mode_bits", 32),
            ("stat_nlink_bits", 32),
        ],
    );
    assert_eq!(table(&program, "linux_aarch64"), expected);
}

#[test]
fn linux_x86_64s_table_holds_linuxs_values_with_x86_64s_flags_and_stat() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    let expected = linux(
        [
            // asm-generic/fcntl.h, which x86's asm/fcntl.h takes as it is.
            ("o_directory", 0o200000),
            ("o_nofollow", 0o400000),
        ],
        [
            // x86's asm/stat.h, `struct stat` for __x86_64__: st_dev, st_ino,
            // st_nlink (8 bytes each), st_mode, st_uid, st_gid, __pad0 (4
            // each), st_rdev, st_size; 144 bytes with the times and
            // __unused[3].
            ("stat_bytes", 144),
            ("stat_mode", 24),
            ("stat_nlink", 16),
            ("stat_size", 48),
            // st_mode is an `unsigned int`, st_nlink an `unsigned long`.
            ("stat_mode_bits", 32),
            ("stat_nlink_bits", 64),
        ],
    );
    assert_eq!(table(&program, "linux_x86_64"), expected);
}
