// SPDX-License-Identifier: Apache-2.0
//! `Map.of` (#605): a map built from a list of entries, put in
//! order. Each helper that runs a program also runs it natively on Linux
//! and compares (`common/mod.rs`).

mod common;

use common::assert_runs;

/// A program with a `show` that prints a
/// `Map<String, Int>` as `key=value` pairs on one line, and the given
/// `body` as `main`'s statements.
fn program(body: &str) -> String {
    format!(
        "\
         fn show(io: Stdout, label: String, m: Map<String, Int>) -> Result<(), IoError> {{\n    \
             var line = label\n    \
             for e in m.entries() {{\n        \
                 let n = e.value\n        \
                 line = \"{{line}} {{e.key}}={{n}}\"\n    \
             }}\n    \
             io.println(line)\n\
         }}\n\n\
         fn yes(b: Bool) -> String {{\n    \
             if b {{ \"true\" }} else {{ \"false\" }}\n\
         }}\n\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             var m: Map<String, Int> = Map.new()\n{body}\n    Ok(())\n}}\n"
    )
}

#[test]
fn of_builds_a_map_from_a_list_of_entries_in_order() {
    let src = program(
        r#"    let made: Map<String, Int> = Map.of([
        Entry(key: "pear", value: 1),
        Entry(key: "apple", value: 2),
        Entry(key: "pear", value: 3),
        Entry(key: "zebra", value: 4),
    ])
    show(io, "made:", made)?
    let count = made.len()
    io.println("len {count}")?
    m.put("pear", 1)
    m.put("apple", 2)
    m.put("pear", 3)
    m.put("zebra", 4)
    io.println(yes(made == m))?"#,
    );
    assert_runs(&src, &[], "made: pear=3 apple=2 zebra=4\nlen 3\ntrue\n");
}
