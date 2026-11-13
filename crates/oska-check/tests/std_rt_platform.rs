//! What `std.rt` reads that differs between target platforms (open flags,
//! the layout of `struct stat`) is in one table per platform, which the
//! constant intrinsic `rt_mem_platform()` gives (#158). The values below
//! are the Linux UAPI headers' for each platform, so a wrong entry fails
//! here and not only on the platform's runner.

use oska_check::program::{ExprKind, Program, TypeDeclKind};

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
            _ => panic!("`{name}`'s `{:?}` is not a constant", field.name),
        })
        .collect()
}

#[test]
fn linux_aarch64s_table_holds_arm64s_flags_and_stat_layout() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    let expected: Vec<(String, i128)> = [
        // asm-generic/fcntl.h, as arm64's asm/fcntl.h overrides it.
        ("o_directory", 0o40000),
        ("o_nofollow", 0o100000),
        // asm-generic/stat.h: st_dev, st_ino (8 bytes each), st_mode,
        // st_nlink (4 each), st_uid, st_gid, st_rdev, __pad1, st_size.
        ("stat_bytes", 128),
        ("stat_mode", 16),
        ("stat_nlink", 20),
        ("stat_size", 48),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    assert_eq!(table(&program, "linux_aarch64"), expected);
}

#[test]
fn linux_x86_64s_table_holds_x86_64s_flags_and_stat_layout() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    let expected: Vec<(String, i128)> = [
        // asm-generic/fcntl.h, which x86's asm/fcntl.h takes as it is.
        ("o_directory", 0o200000),
        ("o_nofollow", 0o400000),
        // x86's asm/stat.h, `struct stat` for __x86_64__: st_dev, st_ino,
        // st_nlink (8 bytes each), st_mode, st_uid, st_gid, __pad0 (4
        // each), st_rdev, st_size; 144 bytes with the times and
        // __unused[3].
        ("stat_bytes", 144),
        ("stat_mode", 24),
        ("stat_nlink", 16),
        ("stat_size", 48),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    assert_eq!(table(&program, "linux_x86_64"), expected);
}
