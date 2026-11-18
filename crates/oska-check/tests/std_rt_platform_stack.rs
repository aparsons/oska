//! `stack_t` in `std.rt`'s platform tables (#361, #403): macOS's layout
//! (Linux's is checked in `std_rt_platform_linux.rs`), and the rule every
//! table keeps: `handle_faults` writes `ss_sp` at 0 and the two words the
//! table names within a 24-byte `stack_t`, so a wrong entry fails here
//! and not only on the platform's runner.

mod common;

use common::platform::{fields_of, owned, tables};

#[test]
fn the_macos_table_holds_xnus_stack_t_layout() {
    // xnu's sys/_types/_sigaltstack.h, `struct __darwin_sigaltstack`:
    // ss_sp (8 bytes), ss_size (8), ss_flags (an `int`, then padding).
    let expected = owned(&[("ss_size_at", 8), ("ss_flags_at", 16)]);
    assert_eq!(fields_of("macos", &["ss_size_at", "ss_flags_at"]), expected);
}

#[test]
fn every_table_puts_ss_size_and_ss_flags_in_separate_words_after_ss_sp() {
    let tables = tables();
    assert!(tables.len() >= 3, "std.rt gives too few tables: {tables:?}");
    for name in &tables {
        let fields = fields_of(name, &["ss_size_at", "ss_flags_at"]);
        let offsets: Vec<i128> = fields.iter().map(|(_, at)| *at).collect();
        assert_eq!(offsets.len(), 2, "{name} lacks a `stack_t` field");
        for at in &offsets {
            assert!(
                *at == 8 || *at == 16,
                "{name}'s `stack_t` field at {at} is not a word after `ss_sp` within 24 bytes"
            );
        }
        assert_ne!(offsets[0], offsets[1], "{name}'s `stack_t` fields overlap");
    }
}
