// SPDX-License-Identifier: Apache-2.0
//! `Map` and `Entry` are in the prelude (#591, ADR 0028): a program uses
//! them with no `use`. Each helper that runs a program also runs it
//! natively on Linux and compares (`common/mod.rs`).

mod common;

use common::assert_runs;

#[test]
fn map_and_entry_need_no_import() {
    let src = r#"fn main(io: Stdout) -> Result<(), IoError> {
    var m: Map<String, Int> = Map.new()
    m.put("pear", 1)
    m.put("apple", 2)
    let made: Map<String, Int> = Map.of([
        Entry(key: "pear", value: 1),
        Entry(key: "apple", value: 2),
    ])
    let limited: Map<Int, Int> = Map.with_max_entries(2)
    let n = m.len()
    let k = limited.len()
    let same = made == m
    io.println("{n} {k}")?
    io.println(if same { "same" } else { "differ" })?
    for e in m.entries() {
        io.println("{e.key}={e.value}")?
    }
    Ok(())
}
"#;
    assert_runs(src, &[], "2 0\nsame\npear=1\napple=2\n");
}

#[test]
fn a_program_that_still_writes_use_std_map_keeps_working() {
    let src = r#"use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var m: map.Map<String, Int> = map.Map.new()
    m.put("a", 1)
    var p: Map<String, Int> = Map.new()
    p.put("a", 1)
    let same = m == p
    io.println(if same { "same" } else { "differ" })?
    Ok(())
}
"#;
    assert_runs(src, &[], "same\n");
}

#[test]
fn a_brace_import_of_map_still_works() {
    let src = r#"use std.map.{Map, Entry}

fn main(io: Stdout) -> Result<(), IoError> {
    let m: Map<Int, Int> = Map.of([Entry(key: 1, value: 2)])
    let n = m.len()
    io.println("{n}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "1\n");
}

#[test]
fn a_helper_function_takes_a_map_without_an_import() {
    let src = r#"fn count(m: Map<Int, Int>) -> Int {
    m.len()
}

fn main(io: Stdout) -> Result<(), IoError> {
    var m: Map<Int, Int> = Map.new()
    m.put(7, 8)
    let n = count(m)
    io.println("{n}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "1\n");
}
