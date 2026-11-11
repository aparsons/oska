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
}

/// The prelude intrinsics the native backend implements, by the name the
/// checked program calls them by. A call of any other is refused (`E0390`).
pub const PRELUDE: &[(&str, Prelude)] = &[
    ("Stdout.println", Prelude::Runtime("stdout_println")),
    ("Stderr.println", Prelude::Runtime("stderr_println")),
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
    ("Path.new", Prelude::Runtime("path_new")),
    ("Path.from", Prelude::Runtime("path_new")),
    ("Path.eq", Prelude::Runtime("path_eq")),
];

/// The implementation of the prelude intrinsic `name`, if there is one.
pub fn prelude(name: &str) -> Option<Prelude> {
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
    ("rt_sys_mmap", Runtime::Syscall(222)),
    ("rt_sys_munmap", Runtime::Syscall(215)),
    ("rt_sys_clock_gettime", Runtime::Syscall(113)),
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
    ("rt_mem_text", Runtime::Identity),
    ("rt_mem_as_text", Runtime::Identity),
    ("rt_mem_path", Runtime::Identity),
    ("rt_mem_as_path", Runtime::Identity),
    ("rt_mem_globals", Runtime::Globals),
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
