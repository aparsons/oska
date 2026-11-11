//! `std.rt` is the toolchain's own (#127, ADR 0013): the system-call and
//! memory intrinsics (`rt_sys_*`, `rt_mem_*`) exist only in its embedded
//! source. Any `use std.rt`, or any name that is an intrinsic, anywhere
//! else is `E0209`.

mod common;

use common::{assert_errors, assert_modules_errors, assert_ok};

#[test]
fn use_std_rt_in_a_single_file_is_refused() {
    assert_errors("use std.rt\nfn main() {}", &[("E0209", "std.rt")]);
}

#[test]
fn a_brace_import_from_std_rt_is_refused() {
    assert_errors(
        "use std.rt.{rt_sys_write}\nfn main() {}",
        &[
            ("E0209", "std.rt.{rt_sys_write}"),
            ("E0209", "rt_sys_write"),
        ],
    );
}

#[test]
fn use_std_rt_in_a_package_module_is_refused() {
    assert_modules_errors(
        &[
            ("main", "use pkg.util\n\nfn main() {}\n"),
            ("util", "use std.rt\n\npub(pkg) fn f() {}\n"),
        ],
        &[("src/util.osk", "E0209", "std.rt")],
    );
}

#[test]
fn naming_an_intrinsic_is_refused() {
    assert_errors(
        "fn main() {\n    let n = rt_sys_getrandom(0, 0, 0)\n}",
        &[("E0209", "rt_sys_getrandom")],
    );
    assert_errors(
        "fn rt_mem_load64(a: Int) -> Int {\n    a\n}",
        &[("E0209", "rt_mem_load64")],
    );
}

#[test]
fn a_user_module_named_rt_is_not_the_toolchains() {
    assert_modules_errors(
        &[
            ("main", "use pkg.rt\n\nfn main() {}\n"),
            (
                "rt",
                "pub(pkg) fn f() -> Int {\n    rt_sys_exit_group(0)\n}\n",
            ),
        ],
        &[("src/rt.osk", "E0209", "rt_sys_exit_group")],
    );
}

#[test]
fn a_pkg_module_named_rt_imports_cleanly() {
    common::assert_modules_ok(&[
        ("main", "use pkg.rt\n\nfn main() {}\n"),
        ("rt", "pub(pkg) fn f() -> Int {\n    1\n}\n"),
    ]);
}

#[test]
fn a_comment_or_text_mentioning_an_intrinsic_is_fine() {
    assert_ok("// rt_sys_write is not for you\nfn f() -> String {\n    \"rt_sys_read\"\n}");
}

#[test]
fn the_toolchains_own_std_rt_checks_cleanly() {
    let report = oska_check::check_toolchain_rt();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn std_rt_declares_every_intrinsic_the_runtime_needs() {
    let names = oska_check::std_rt_intrinsics();
    for expected in [
        "rt_sys_read",
        "rt_sys_write",
        "rt_sys_openat",
        "rt_sys_close",
        "rt_sys_mkdirat",
        "rt_sys_getdents64",
        "rt_sys_fstat",
        "rt_sys_mmap",
        "rt_sys_munmap",
        "rt_sys_clock_gettime",
        "rt_sys_getrandom",
        "rt_sys_exit_group",
        "rt_mem_load64",
        "rt_mem_store64",
        "rt_mem_and",
        "rt_mem_or",
        "rt_mem_xor",
        "rt_mem_shl",
        "rt_mem_shr",
        "rt_mem_sar",
    ] {
        assert!(names.contains(&expected), "{expected} in {names:?}");
    }
    for name in &names {
        assert!(name.starts_with("rt_sys_") || name.starts_with("rt_mem_"));
    }
}
