//! How the native backend implements body-less functions: the prelude's
//! intrinsics, which user code calls, and `std.rt`'s system-call and
//! memory intrinsics, which only the runtime calls.
//!
//! A prelude intrinsic is implemented in Oska, by a function of `std.rt`
//! that takes the same arguments (the receiver first), or, when that would
//! be a single instruction, inline. A `std.rt` intrinsic is one system
//! call (`svc #0`), one word operation, or a constant. What differs
//! between target platforms is in one [`Platform`] per target: its
//! system-call numbers here, and its table in `std.rt`.
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
    /// The Linux system call `Sys`, of the target platform's number.
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

/// A system call, which the `std.rt` intrinsic of its name makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sys {
    Read,
    Write,
    Openat,
    Close,
    Mkdirat,
    Getdents64,
    Fstat,
    Ftruncate,
    Readlinkat,
    Getcwd,
    Fcntl,
    Mmap,
    Munmap,
    ClockGettime,
    Ioctl,
    Getrandom,
    RtSigaction,
    ExitGroup,
}

/// What the backend gives `std.rt` for a target platform.
#[derive(Debug, Clone, Copy)]
pub struct Platform {
    /// The `std.rt` function giving the platform's table, which
    /// `rt_mem_platform` gives: its open flags and `struct stat` layout.
    pub table: &'static str,
    /// Each system call's number.
    pub syscall: fn(Sys) -> u16,
}

/// The target platforms the backend builds for.
pub const PLATFORMS: [Platform; 1] = [LINUX_AARCH64];

/// The target platform the backend builds for: `linux-aarch64`, the
/// only one so far.
pub const PLATFORM: Platform = PLATFORMS[0];

/// `linux-aarch64`.
pub const LINUX_AARCH64: Platform = Platform {
    table: "linux_aarch64",
    syscall: linux_aarch64_syscall,
};

/// `linux-aarch64`'s system-call numbers: the generic Linux table's
/// (`asm-generic/unistd.h`).
fn linux_aarch64_syscall(call: Sys) -> u16 {
    match call {
        Sys::Read => 63,
        Sys::Write => 64,
        Sys::Openat => 56,
        Sys::Close => 57,
        Sys::Mkdirat => 34,
        Sys::Getdents64 => 61,
        Sys::Fstat => 80,
        Sys::Ftruncate => 46,
        Sys::Readlinkat => 78,
        Sys::Getcwd => 17,
        Sys::Fcntl => 25,
        Sys::Mmap => 222,
        Sys::Munmap => 215,
        Sys::ClockGettime => 113,
        Sys::Ioctl => 29,
        Sys::Getrandom => 278,
        Sys::RtSigaction => 134,
        Sys::ExitGroup => 94,
    }
}

/// Every `std.rt` intrinsic, by name.
pub const RUNTIME: &[(&str, Runtime)] = &[
    ("rt_sys_read", Runtime::Syscall(Sys::Read)),
    ("rt_sys_write", Runtime::Syscall(Sys::Write)),
    ("rt_sys_openat", Runtime::Syscall(Sys::Openat)),
    ("rt_sys_close", Runtime::Syscall(Sys::Close)),
    ("rt_sys_mkdirat", Runtime::Syscall(Sys::Mkdirat)),
    ("rt_sys_getdents64", Runtime::Syscall(Sys::Getdents64)),
    ("rt_sys_fstat", Runtime::Syscall(Sys::Fstat)),
    ("rt_sys_ftruncate", Runtime::Syscall(Sys::Ftruncate)),
    ("rt_sys_readlinkat", Runtime::Syscall(Sys::Readlinkat)),
    ("rt_sys_getcwd", Runtime::Syscall(Sys::Getcwd)),
    ("rt_sys_fcntl", Runtime::Syscall(Sys::Fcntl)),
    ("rt_sys_mmap", Runtime::Syscall(Sys::Mmap)),
    ("rt_sys_munmap", Runtime::Syscall(Sys::Munmap)),
    ("rt_sys_clock_gettime", Runtime::Syscall(Sys::ClockGettime)),
    ("rt_sys_ioctl", Runtime::Syscall(Sys::Ioctl)),
    ("rt_sys_getrandom", Runtime::Syscall(Sys::Getrandom)),
    ("rt_sys_rt_sigaction", Runtime::Syscall(Sys::RtSigaction)),
    ("rt_sys_exit_group", Runtime::Syscall(Sys::ExitGroup)),
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
        for platform in PLATFORMS {
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

    #[test]
    fn every_platform_gives_each_system_call_a_number_of_its_own() {
        for platform in PLATFORMS {
            let mut numbers: Vec<u16> = RUNTIME
                .iter()
                .filter_map(|(_, implementation)| match implementation {
                    Runtime::Syscall(call) => Some((platform.syscall)(*call)),
                    _ => None,
                })
                .collect();
            let calls = numbers.len();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), calls, "`{}`", platform.table);
        }
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
