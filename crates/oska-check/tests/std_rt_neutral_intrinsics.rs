// SPDX-License-Identifier: Apache-2.0
//! Reading a directory, random bytes, exit and signal setup are
//! operating-system-neutral intrinsics of `std.rt` (#168), so each
//! operating system's table can give its own call behind them.

#[test]
fn std_rt_names_no_linux_only_call_where_a_neutral_form_exists() {
    let names = oska_check::std_rt_intrinsics();
    for linux_only in [
        "rt_sys_getdents64",
        "rt_sys_getrandom",
        "rt_sys_exit_group",
        "rt_sys_rt_sigaction",
    ] {
        assert!(!names.contains(&linux_only), "{linux_only} in {names:?}");
    }
}
