// SPDX-License-Identifier: Apache-2.0
//! macOS.

use super::*;

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
        ("rt_sys_unlinkat", "_unlinkat", 3, 3, false, MinusOne, &[]),
        ("rt_sys_renameat", "_renameat", 4, 4, false, MinusOne, &[]),
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
        ("rt_sys_socket", "_socket", 3, 3, false, MinusOne, &[]),
        ("rt_sys_connect", "_connect", 3, 3, false, MinusOne, &[]),
        ("rt_sys_bind", "_bind", 3, 3, false, MinusOne, &[]),
        ("rt_sys_listen", "_listen", 2, 2, false, MinusOne, &[]),
        ("rt_sys_accept", "_accept", 3, 3, false, MinusOne, &[]),
        (
            "rt_sys_getsockname",
            "_getsockname",
            3,
            3,
            false,
            MinusOne,
            &[],
        ),
        (
            "rt_sys_getsockopt",
            "_getsockopt",
            5,
            5,
            false,
            MinusOne,
            &[],
        ),
        (
            "rt_sys_setsockopt",
            "_setsockopt",
            5,
            5,
            false,
            MinusOne,
            &[],
        ),
        // send(fd, buf, count, flags) and recv: no flags.
        ("rt_sys_send", "_send", 3, 4, true, MinusOne, &[0]),
        ("rt_sys_recv", "_recv", 3, 4, true, MinusOne, &[0]),
        ("rt_sys_poll", "_poll", 3, 3, false, MinusOne, &[]),
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
            false,
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
