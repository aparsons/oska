//! What `std.rt` reads that differs between target platforms is in one
//! table per platform, which the constant intrinsic `rt_mem_platform()`
//! gives (#158). Between Linux processors that is the open flags and the
//! layout of `struct stat`, checked here; the values every Linux processor
//! shares are checked in `std_rt_platform_linux.rs`. The values below are
//! the Linux UAPI headers' for each platform, so a wrong entry fails here
//! and not only on the platform's runner.

mod common;

use common::platform::{fields_of, owned, LINUX_PROCESSOR_FIELDS};

#[test]
fn linux_aarch64s_table_holds_arm64s_flags_and_stat_layout() {
    let expected = owned(&[
        // asm-generic/fcntl.h, as arm64's asm/fcntl.h overrides it.
        ("o_directory", 0o40000),
        ("o_nofollow", 0o100000),
        // asm-generic/stat.h: st_dev, st_ino (8 bytes each), st_mode,
        // st_nlink (4 each), st_uid, st_gid, st_rdev, __pad1, st_size.
        ("stat_bytes", 128),
        ("stat_mode", 16),
        ("stat_nlink", 20),
        ("stat_size", 48),
        // st_mode and st_nlink are `unsigned int`s.
        ("stat_mode_bits", 32),
        ("stat_nlink_bits", 32),
    ]);
    assert_eq!(
        fields_of("linux_aarch64", &LINUX_PROCESSOR_FIELDS),
        expected
    );
}

#[test]
fn linux_x86_64s_table_holds_x86_64s_flags_and_stat_layout() {
    let expected = owned(&[
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
        // st_mode is an `unsigned int`, st_nlink an `unsigned long`.
        ("stat_mode_bits", 32),
        ("stat_nlink_bits", 64),
    ]);
    assert_eq!(fields_of("linux_x86_64", &LINUX_PROCESSOR_FIELDS), expected);
}
