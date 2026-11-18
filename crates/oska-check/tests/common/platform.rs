//! Reading `std.rt`'s platform tables (#158, #168): each is a function
//! that gives one `Platform(...)` of constants.

use oska_check::program::{ExprKind, Program, TypeDeclKind};

/// The fields of a Linux table that differ between processors: the open
/// flags and `struct stat`. `std_rt_platform.rs` checks them; every other
/// field is one every Linux processor shares, which
/// `std_rt_platform_linux.rs` checks.
pub const LINUX_PROCESSOR_FIELDS: [&str; 8] = [
    "o_directory",
    "o_nofollow",
    "stat_bytes",
    "stat_mode",
    "stat_nlink",
    "stat_size",
    "stat_mode_bits",
    "stat_nlink_bits",
];

/// The names of every `std.rt` function that gives a `Platform`: one
/// table per target platform, sorted.
pub fn tables() -> Vec<String> {
    let report = oska_check::check_toolchain_rt();
    let program: Program = report.program.expect("std.rt should check");
    let mut names: Vec<String> = program
        .functions
        .iter()
        .filter(|(_, function)| function.ret.to_string() == "Platform")
        .map(|(name, _)| name.to_string())
        .collect();
    names.sort();
    names
}

/// The table the `std.rt` function `name` gives, by field name.
pub fn table(name: &str) -> Vec<(String, i128)> {
    let report = oska_check::check_toolchain_rt();
    let program: Program = report.program.expect("std.rt should check");
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

/// The fields of the table `name` named in `fields`, in the table's
/// order.
pub fn fields_of(name: &str, fields: &[&str]) -> Vec<(String, i128)> {
    table(name)
        .into_iter()
        .filter(|(field, _)| fields.contains(&field.as_str()))
        .collect()
}

/// `expected`, as `fields_of` gives it.
pub fn owned(expected: &[(&str, i128)]) -> Vec<(String, i128)> {
    expected
        .iter()
        .map(|(field, value)| (field.to_string(), *value))
        .collect()
}
