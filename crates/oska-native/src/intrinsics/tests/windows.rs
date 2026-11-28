// SPDX-License-Identifier: Apache-2.0
//! Windows.

use super::*;

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
    const W: &str = "WS2_32.dll";
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
            "rt_sys_nt_set_information_file",
            "ntdll.dll",
            "NtSetInformationFile",
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
        // GetAdaptersAddresses returns its error code, not a BOOL.
        (
            "rt_sys_get_adapters_addresses",
            "IPHLPAPI.DLL",
            "GetAdaptersAddresses",
            5,
            false,
            Never,
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
        // Winsock: `SOCKET_ERROR` (-1) says a call failed, and
        // `WSAGetLastError` is `GetLastError`; `WSAStartup` gives its
        // error itself.
        ("rt_sys_wsa_startup", W, "WSAStartup", 2, false, Never, &[]),
        ("rt_sys_socket", W, "socket", 3, true, MinusOne, &[]),
        ("rt_sys_connect", W, "connect", 3, false, MinusOne, &[]),
        ("rt_sys_bind", W, "bind", 3, false, MinusOne, &[]),
        ("rt_sys_listen", W, "listen", 2, false, MinusOne, &[]),
        ("rt_sys_accept", W, "accept", 3, true, MinusOne, &[]),
        (
            "rt_sys_getsockname",
            W,
            "getsockname",
            3,
            false,
            MinusOne,
            &[],
        ),
        (
            "rt_sys_getsockopt",
            W,
            "getsockopt",
            5,
            false,
            MinusOne,
            &[],
        ),
        (
            "rt_sys_setsockopt",
            W,
            "setsockopt",
            5,
            false,
            MinusOne,
            &[],
        ),
        // send(socket, buf, count, flags) and recv: no flags.
        ("rt_sys_send", W, "send", 3, false, MinusOne, &[0]),
        ("rt_sys_recv", W, "recv", 3, false, MinusOne, &[0]),
        ("rt_sys_poll", W, "WSAPoll", 3, false, MinusOne, &[]),
        (
            "rt_sys_close_socket",
            W,
            "closesocket",
            1,
            false,
            MinusOne,
            &[],
        ),
        (
            "rt_sys_ioctl_socket",
            W,
            "ioctlsocket",
            3,
            false,
            MinusOne,
            &[],
        ),
    ];
    let platform = WINDOWS_X86_64;
    for (name, implementation) in RUNTIME {
        let Runtime::Syscall(call) = implementation else {
            continue;
        };
        if !windows_only(*call) && !is_socket_call(*call) {
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

/// Whether `call` is one of the socket calls every platform makes.
fn is_socket_call(call: Sys) -> bool {
    matches!(
        call,
        Sys::Socket
            | Sys::Connect
            | Sys::Bind
            | Sys::Listen
            | Sys::Accept
            | Sys::Getsockname
            | Sys::Getsockopt
            | Sys::Setsockopt
            | Sys::Send
            | Sys::Recv
            | Sys::Poll
    )
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
        false,
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
