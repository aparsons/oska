// SPDX-License-Identifier: Apache-2.0
//! The Windows platform: `kernel32` and `ntdll` calls (ADR 0020).

use super::*;

/// Whether `call` is one of Windows' own, which no other platform has.
pub(super) fn windows_only(call: Sys) -> bool {
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
            | Sys::NtSetInformationFile
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
            | Sys::WsaStartup
            | Sys::CloseSocket
            | Sys::IoctlSocket
            | Sys::GetAdaptersAddresses
    )
}

/// `windows-x86_64`: every call is a function of `kernel32.dll`,
/// `ntdll.dll`, `bcryptprimitives.dll` or `iphlpapi.dll`, through the import table, by the Windows x64
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
const WS2_32: &str = "WS2_32.dll";
const IPHLPAPI: &str = "IPHLPAPI.DLL";

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
/// `NtSetInformationFile(h, &status, buf, length, class)`.
const NT_SET_INFORMATION_FILE: Import = nt("NtSetInformationFile", 5);
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

/// A function of `ws2_32.dll` that takes `args` of the intrinsic's
/// arguments and gives an `int`, `SOCKET_ERROR` (-1) on failure. Winsock's
/// error is `GetLastError()`'s.
const fn ws(symbol: &'static str, args: u8) -> Import {
    Import {
        library: WS2_32,
        fails: Fails::MinusOne,
        ..win(symbol, args)
    }
}
/// `WSAStartup(version, &data)`: the error itself, 0 for none.
const WSA_STARTUP: Import = Import {
    fails: Fails::Never,
    ..ws("WSAStartup", 2)
};
/// `socket(family, type, protocol)`: a `SOCKET`, `INVALID_SOCKET` (-1) on
/// failure.
const WS_SOCKET: Import = Import {
    wide: true,
    ..ws("socket", 3)
};
const WS_CONNECT: Import = ws("connect", 3);
const WS_BIND: Import = ws("bind", 3);
const WS_LISTEN: Import = ws("listen", 2);
/// `accept(socket, NULL, NULL)`: a `SOCKET`.
const WS_ACCEPT: Import = Import {
    wide: true,
    ..ws("accept", 3)
};
const WS_GETSOCKNAME: Import = ws("getsockname", 3);
const WS_GETSOCKOPT: Import = ws("getsockopt", 5);
const WS_SETSOCKOPT: Import = ws("setsockopt", 5);
/// `send(socket, buf, count, 0)`.
const WS_SEND: Import = Import {
    fixed: 4,
    extra_args: &[0],
    ..ws("send", 3)
};
/// `recv(socket, buf, count, 0)`.
const WS_RECV: Import = Import {
    fixed: 4,
    extra_args: &[0],
    ..ws("recv", 3)
};
/// `WSAPoll(entries, count, milliseconds)`: the number of ready entries.
const WS_POLL: Import = ws("WSAPoll", 3);
const WS_CLOSE_SOCKET: Import = ws("closesocket", 1);
/// `ioctlsocket(socket, command, &argument)`.
const WS_IOCTL_SOCKET: Import = ws("ioctlsocket", 3);
/// `GetAdaptersAddresses(family, flags, reserved, addresses, size)`: the
/// adapters, with their DNS servers, as an `IP_ADAPTER_ADDRESSES` list. It
/// returns the Win32 error code itself (0 on success, 111 when `size` is
/// too small, 232 when there is no adapter), so `std.rt` reads the result
/// as it is (ADR 0026).
const GET_ADAPTERS_ADDRESSES: Import = Import {
    library: IPHLPAPI,
    fails: Fails::Never,
    ..win("GetAdaptersAddresses", 5)
};

/// Every function a Windows executable imports, `kernel32.dll`'s first:
/// always all of them, so the import table depends on the target alone.
/// [`GET_LAST_ERROR`] is first ([`Platform::errno`]).
const WINDOWS_IMPORTS: [&Import; 40] = [
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
    &NT_SET_INFORMATION_FILE,
    &NT_QUERY_DIRECTORY_FILE,
    &NT_STATUS_TO_ERROR,
    &PROCESS_PRNG,
    &GET_ADAPTERS_ADDRESSES,
    &WSA_STARTUP,
    &WS_SOCKET,
    &WS_CONNECT,
    &WS_BIND,
    &WS_LISTEN,
    &WS_ACCEPT,
    &WS_GETSOCKNAME,
    &WS_GETSOCKOPT,
    &WS_SETSOCKOPT,
    &WS_SEND,
    &WS_RECV,
    &WS_POLL,
    &WS_CLOSE_SOCKET,
    &WS_IOCTL_SOCKET,
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
        Sys::NtSetInformationFile => &NT_SET_INFORMATION_FILE,
        Sys::NtQueryDirectoryFile => &NT_QUERY_DIRECTORY_FILE,
        Sys::ProcessPrng => &PROCESS_PRNG,
        Sys::WsaStartup => &WSA_STARTUP,
        Sys::Socket => &WS_SOCKET,
        Sys::Connect => &WS_CONNECT,
        Sys::Bind => &WS_BIND,
        Sys::Listen => &WS_LISTEN,
        Sys::Accept => &WS_ACCEPT,
        Sys::Getsockname => &WS_GETSOCKNAME,
        Sys::Getsockopt => &WS_GETSOCKOPT,
        Sys::Setsockopt => &WS_SETSOCKOPT,
        Sys::Send => &WS_SEND,
        Sys::Recv => &WS_RECV,
        Sys::Poll => &WS_POLL,
        Sys::CloseSocket => &WS_CLOSE_SOCKET,
        Sys::IoctlSocket => &WS_IOCTL_SOCKET,
        Sys::GetAdaptersAddresses => &GET_ADAPTERS_ADDRESSES,
        _ => return OsCall::Absent,
    })
}
