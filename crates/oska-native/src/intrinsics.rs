//! How the native backend implements body-less functions: the prelude's
//! intrinsics, which user code calls, and `std.rt`'s system-call and
//! memory intrinsics, which only the runtime calls.
//!
//! A prelude intrinsic is implemented in Oska, by a function of `std.rt`
//! that takes the same arguments (the receiver first), or, when that would
//! be a single instruction, inline. A `std.rt` intrinsic is one system
//! call (`svc #0`) or one word operation.
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
    /// The Linux aarch64 system call of this number.
    Syscall(u16),
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
}

/// Every `std.rt` intrinsic, by name. The system-call numbers are the
/// generic Linux table's (`asm-generic/unistd.h`), which aarch64 uses.
pub const RUNTIME: &[(&str, Runtime)] = &[
    ("rt_sys_read", Runtime::Syscall(63)),
    ("rt_sys_write", Runtime::Syscall(64)),
    ("rt_sys_openat", Runtime::Syscall(56)),
    ("rt_sys_close", Runtime::Syscall(57)),
    ("rt_sys_mkdirat", Runtime::Syscall(34)),
    ("rt_sys_getdents64", Runtime::Syscall(61)),
    ("rt_sys_fstat", Runtime::Syscall(80)),
    ("rt_sys_ftruncate", Runtime::Syscall(46)),
    ("rt_sys_readlinkat", Runtime::Syscall(78)),
    ("rt_sys_getcwd", Runtime::Syscall(17)),
    ("rt_sys_fcntl", Runtime::Syscall(25)),
    ("rt_sys_mmap", Runtime::Syscall(222)),
    ("rt_sys_munmap", Runtime::Syscall(215)),
    ("rt_sys_clock_gettime", Runtime::Syscall(113)),
    ("rt_sys_ioctl", Runtime::Syscall(29)),
    ("rt_sys_getrandom", Runtime::Syscall(278)),
    ("rt_sys_exit_group", Runtime::Syscall(94)),
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
