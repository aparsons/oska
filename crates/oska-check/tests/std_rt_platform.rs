//! What differs between target platforms (system-call numbers, open flags,
//! the layout of `struct stat`) reaches `std.rt` from one table per
//! platform, which the constant intrinsic `rt_mem_platform()` gives (#158).
//! No `std.rt` function holds such a value itself, so adding a platform is
//! adding a table.

/// The non-comment lines of `std.rt`, with their numbers.
fn code_lines() -> impl Iterator<Item = (usize, &'static str)> {
    oska_check::STD_RT_SOURCE
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
}

#[test]
fn every_system_call_takes_its_number_from_the_platform_table() {
    let mut calls = 0;
    for (number, line) in code_lines() {
        for (at, _) in line.match_indices("rt_sys_") {
            if line[..at].ends_with("fn ") {
                continue;
            }
            calls += 1;
            let call = &line[at..];
            let args = call.split_once('(').map_or("", |(_, args)| args);
            assert!(
                args.starts_with("rt_mem_platform().sys_"),
                "std.rt line {number} makes a system call whose number is not the platform table's: {}",
                line.trim()
            );
        }
    }
    assert!(calls > 0, "std.rt makes no system call");
}
