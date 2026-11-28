// SPDX-License-Identifier: Apache-2.0
//! `List.set`, `pop` and `truncate` (spec #798, ticket #800, ADR 0031).
//!
//! Every `assert_runs` and `assert_panics` here is also a differential
//! test: on aarch64 and x86_64 Linux the program is compiled and run
//! natively too, and must print, exit and panic as the interpreter does
//! (`common/mod.rs`). The step counts and the golden of the `P0002` report
//! are in `crates/oskac/tests/list_primitives.rs`.

mod common;

use common::{assert_panics, assert_runs, run_under_limit};
use oska_run::codes;

/// A program whose `main` runs `body` with `io: Stdout` in scope, after
/// `items`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// A helper that prints a list of `Int`s on one line.
const SHOW: &str = r#"fn show(io: Stdout, xs: List<Int>) -> Result<(), IoError> {
    var line = ""
    for x in xs {
        line = "{line}{x} "
    }
    io.println("[{line}]")
}"#;

#[test]
fn set_changes_one_item_and_keeps_the_length() {
    assert_runs(
        &program(
            SHOW,
            "    var xs = [1, 2, 3]
    xs.set(1, 20)
    xs.set(0, 10)
    xs.set(2, 30)
    show(io, xs)?
    let n = xs.len()
    io.println(\"{n}\")",
        ),
        &[],
        "[10 20 30 ]\n3\n",
    );
}

#[test]
fn a_copy_made_before_set_pop_or_truncate_does_not_see_the_change() {
    assert_runs(
        &program(
            SHOW,
            "    var xs = [1, 2, 3]
    let before = xs
    xs.set(0, 99)
    show(io, before)?
    show(io, xs)?
    var ys = [1, 2, 3]
    let kept = ys
    let _ = ys.pop()
    show(io, kept)?
    show(io, ys)?
    var zs = [1, 2, 3]
    let whole = zs
    zs.truncate(1)
    show(io, whole)?
    show(io, zs)",
        ),
        &[],
        "[1 2 3 ]\n[99 2 3 ]\n[1 2 3 ]\n[1 2 ]\n[1 2 3 ]\n[1 ]\n",
    );
}

#[test]
fn a_list_in_a_struct_field_and_in_a_list_is_copied_on_write() {
    assert_runs(
        &program(
            &format!("{SHOW}\nstruct Box {{ items: List<Int> }}"),
            "    var a = Box(items: [1, 2, 3])
    let b = a
    a.items.set(1, 7)
    let _ = a.items.pop()
    show(io, a.items)?
    show(io, b.items)?
    var rows = [[1, 2], [3, 4]]
    let first = rows.at(0)
    var row = rows.at(0)
    row.set(0, 5)
    rows.set(0, row)
    show(io, first)?
    show(io, rows.at(0))?
    show(io, rows.at(1))",
        ),
        &[],
        "[1 7 ]\n[1 2 3 ]\n[1 2 ]\n[5 2 ]\n[3 4 ]\n",
    );
}

#[test]
fn set_replaces_text_and_struct_items() {
    assert_runs(
        &program(
            "struct P { x: Int }",
            "    var names = [\"a\", \"b\", \"c\"]
    let old = names
    let a = old.at(0)
    let c = old.at(2)
    names.set(1, \"{a}{c}\")
    io.println(names.at(1))?
    io.println(old.at(1))?
    var ps = [P(x: 1), P(x: 2)]
    ps.set(0, P(x: 9))
    let first = ps.at(0)
    let second = ps.at(1)
    io.println(\"{first.x} {second.x}\")",
        ),
        &[],
        "ac\nb\n9 2\n",
    );
}

#[test]
fn pop_gives_the_last_item_and_none_for_an_empty_list() {
    assert_runs(
        &program(
            SHOW,
            "    var xs = [1, 2]
    var i = 0
    while i < 4 {
        let last = xs.pop().unwrap_or(-1)
        io.println(\"{last}\")?
        i = i + 1
    }
    show(io, xs)?
    var names = [\"x\", \"y\"]
    let last = names.pop()
    match last {
        Some(s) => { io.println(s)? }
        None => { io.println(\"none\")? }
    }
    let n = names.len()
    io.println(\"{n}\")",
        ),
        &[],
        "2\n1\n-1\n-1\n[]\ny\n1\n",
    );
}

#[test]
fn a_list_can_be_pushed_to_again_after_pop_and_truncate() {
    assert_runs(
        &program(
            SHOW,
            "    var xs = [1, 2, 3, 4]
    xs.truncate(2)
    xs.push(8)
    let _ = xs.pop()
    let _ = xs.pop()
    xs.push(5)
    xs.push(6)
    show(io, xs)",
        ),
        &[],
        "[1 5 6 ]\n",
    );
}

#[test]
fn truncate_keeps_the_first_n_items_and_never_panics() {
    assert_runs(
        &program(
            SHOW,
            "    var xs = [1, 2, 3, 4, 5]
    xs.truncate(9)
    show(io, xs)?
    xs.truncate(5)
    show(io, xs)?
    xs.truncate(3)
    show(io, xs)?
    xs.truncate(3)
    show(io, xs)?
    xs.truncate(-4)
    show(io, xs)?
    xs.truncate(2)
    show(io, xs)?
    var empty: List<Int> = []
    empty.truncate(-1)
    empty.truncate(0)
    empty.truncate(3)
    show(io, empty)",
        ),
        &[],
        "[1 2 3 4 5 ]\n[1 2 3 4 5 ]\n[1 2 3 ]\n[1 2 3 ]\n[]\n[]\n[]\n",
    );
}

#[test]
fn items_released_by_pop_and_truncate_and_replaced_by_set_are_freed() {
    // Texts held by a list are counted by native code, which must release
    // an item that leaves the list: the program ends cleanly either way,
    // and the interpreter and native code print the same.
    assert_runs(
        &program(
            "",
            "    var total = 0
    var round = 0
    while round < 200 {
        var xs = [\"a{round}\", \"b{round}\", \"c{round}\"]
        let shared = xs
        xs.set(0, \"z{round}\")
        let _ = xs.pop()
        xs.truncate(1)
        total = total + xs.at(0).len() + shared.len()
        round = round + 1
    }
    io.println(\"{total}\")",
        ),
        &[],
        // "z" and the digits of the round, plus 3 for each shared list.
        &format!(
            "{}\n",
            (0..200)
                .map(|r: i32| 1 + r.to_string().len() + 3)
                .sum::<usize>()
        ),
    );
}

#[test]
fn set_with_a_bad_index_panics_with_p0002() {
    for index in ["3", "-1", "100"] {
        let src = program(
            "",
            &format!(
                "    var xs = [1, 2, 3]\n    xs.set({index}, 0)\n    io.println(\"unreached\")"
            ),
        );
        assert_panics(&src, "P0002", &format!("xs.set({index}, 0)"));
    }
    assert_panics(
        &program(
            "",
            "    var xs: List<Int> = []\n    xs.set(0, 1)\n    io.println(\"unreached\")",
        ),
        "P0002",
        "xs.set(0, 1)",
    );
}

#[test]
fn the_p0002_report_of_set_names_the_index_and_length() {
    let outcome = common::run_fake(
        &program(
            "",
            "    var xs = [1, 2, 3]\n    xs.set(5, 0)\n    io.println(\"unreached\")",
        ),
        &[],
    );
    let panic = outcome.panic.expect("a bad index panics");
    assert_eq!(panic.code, codes::INDEX_OUT_OF_BOUNDS);
    assert_eq!(
        panic.message,
        "index 5 is out of bounds for a list of length 3"
    );
}

#[test]
fn set_pop_and_truncate_need_a_var_list() {
    for call in ["xs.set(0, 1)", "let _ = xs.pop()", "xs.truncate(1)"] {
        let src = program("", &format!("    let xs = [1, 2]\n    {call}\n    Ok(())"));
        let outcome = common::run_fake(&src, &[]);
        assert!(
            outcome.diagnostics.iter().any(|d| d.code.starts_with('E')),
            "{call}: {outcome:#?}"
        );
        assert_eq!(outcome.exit_code, 1, "{call}");
    }
}

#[test]
fn under_max_memory_a_program_that_grows_through_set_on_shared_lists_stops_with_p0040() {
    // Every `set` on a list that a kept copy shares must copy it, and the
    // copy is charged as `push` charges one: 2,000 lists of 1,000 items
    // are far past 1 MiB.
    let src = "fn main(io: Stdout) {
    var xs: List<Int> = []
    var i = 0
    while i < 1000 {
        xs.push(i)
        i = i + 1
    }
    var kept: List<List<Int>> = []
    var n = 0
    while n < 2000 {
        kept.push(xs)
        xs.set(0, n)
        n = n + 1
    }
    let _ = io.println(\"unreached\")
}
";
    let outcome = run_under_limit(src, 1 << 20);
    let Some(panic) = &outcome.panic else {
        panic!("2,000 copies of 1,000 items pass 1 MiB: {outcome:#?}");
    };
    assert_eq!(panic.code, codes::MEMORY_LIMIT, "{panic:#?}");
    assert_eq!(outcome.stdout, "");
}

#[test]
fn under_max_memory_pop_and_truncate_on_an_unshared_list_charge_nothing_more() {
    // One list held alone, changed 200,000 times: nothing is allocated, so
    // a 1 MiB limit is never reached.
    let src = "fn main(io: Stdout) {
    var xs: List<Int> = [1, 2, 3, 4]
    var i = 0
    while i < 200000 {
        xs.set(1, i)
        let _ = xs.pop()
        xs.push(i)
        xs.truncate(3)
        xs.push(i)
        i = i + 1
    }
    let n = xs.len()
    let _ = io.println(\"{n}\")
}
";
    let outcome = run_under_limit(src, 1 << 20);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "4\n");
}

#[test]
fn under_max_memory_pop_and_truncate_of_a_shared_list_charge_a_copy() {
    for change in ["copy.truncate(999)", "let _ = copy.pop()"] {
        let src = format!(
            "fn main(io: Stdout) {{
    var base: List<Int> = []
    var i = 0
    while i < 1000 {{
        base.push(i)
        i = i + 1
    }}
    var kept: List<List<Int>> = []
    var n = 0
    while n < 2000 {{
        var copy = base
        {change}
        kept.push(copy)
        n = n + 1
    }}
    let _ = io.println(\"unreached\")
}}
"
        );
        let outcome = run_under_limit(&src, 1 << 20);
        let Some(panic) = &outcome.panic else {
            panic!("{change}: copies of 1,000 items pass 1 MiB: {outcome:#?}");
        };
        assert_eq!(panic.code, codes::MEMORY_LIMIT, "{change}: {panic:#?}");
    }
}
