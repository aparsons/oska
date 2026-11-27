// SPDX-License-Identifier: Apache-2.0
//! The core library's `List` methods (spec #660, ticket #667, ADR 0034):
//! `filter`, `fold`, `any`, `all`, `find`, `contains`, `first`, `last`,
//! `is_empty`, `reverse` and `concat`. Each runs on empty, one-item and
//! many-item lists, and every program also runs natively on Linux
//! (`common/mod.rs`), so both backends give the same result.

mod common;

use common::assert_runs;

fn main_with(body: &str) -> String {
    format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n{body}?\n    Ok(())\n}}\n\
         fn show(xs: List<Int>) -> String {{\n    \
         xs.fold(\"\", |acc, x| \"{{acc}}{{x}},\")\n}}\n\
         fn opt(o: Option<Int>) -> String {{\n    \
         match o {{\n        Some(n) => \"some {{n}}\"\n        None => \"none\"\n    }}\n}}\n"
    )
}

#[test]
fn filter_keeps_the_items_that_pass_in_order() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         io.println(show(none.filter(|x| x > 0)))?\n    \
         io.println(show([5].filter(|x| x > 0)))?\n    \
         io.println(show([5].filter(|x| x > 9)))?\n    \
         io.println(show([1, 2, 3, 4, 5, 6].filter(|x| x % 2 == 0)))",
    );
    assert_runs(&src, &[], "\n5,\n\n2,4,6,\n");
}

#[test]
fn fold_combines_from_the_initial_value() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         let a = none.fold(7, |acc, x| acc + x)\n    \
         let b = [4].fold(7, |acc, x| acc + x)\n    \
         let c = [1, 2, 3, 4].fold(0, |acc, x| acc * 10 + x)\n    \
         io.println(\"{a} {b} {c}\")",
    );
    assert_runs(&src, &[], "7 11 1234\n");
}

#[test]
fn any_and_all_on_empty_one_and_many() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         let a = none.any(|x| x > 1)\n    \
         let b = none.all(|x| x > 1)\n    \
         let c = [2].any(|x| x > 1)\n    \
         let d = [1].all(|x| x > 1)\n    \
         let e = [1, 2, 3].any(|x| x == 3)\n    \
         let f = [1, 2, 3].all(|x| x < 3)\n    \
         io.println(\"{a} {b} {c} {d} {e} {f}\")",
    );
    // `Bool` has no `Display`, so print through `if`.
    let src = src.replace(
        "io.println(\"{a} {b} {c} {d} {e} {f}\")",
        "let s = [a, b, c, d, e, f].fold(\"\", |acc, v| if v { \"{acc}T\" } else { \"{acc}F\" })\n    io.println(s)",
    );
    assert_runs(&src, &[], "FTTFTF\n");
}

#[test]
fn find_gives_the_first_match_or_none() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         io.println(opt(none.find(|x| x > 1)))?\n    \
         io.println(opt([3].find(|x| x > 1)))?\n    \
         io.println(opt([1].find(|x| x > 1)))?\n    \
         io.println(opt([1, 5, 7].find(|x| x > 1)))",
    );
    assert_runs(&src, &[], "none\nsome 3\nnone\nsome 5\n");
}

#[test]
fn contains_needs_eq_and_works_on_text() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         let a = none.contains(1)\n    \
         let b = [1].contains(1)\n    \
         let c = [1, 2, 3].contains(4)\n    \
         let d = [\"x\", \"y\"].contains(\"y\")\n    \
         let s = [a, b, c, d].fold(\"\", |acc, v| if v { \"{acc}T\" } else { \"{acc}F\" })\n    \
         io.println(s)",
    );
    assert_runs(&src, &[], "FTFT\n");
}

#[test]
fn first_last_and_is_empty() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         io.println(opt(none.first()))?\n    \
         io.println(opt(none.last()))?\n    \
         io.println(opt([4].first()))?\n    \
         io.println(opt([4].last()))?\n    \
         io.println(opt([1, 2, 3].first()))?\n    \
         io.println(opt([1, 2, 3].last()))?\n    \
         let a = none.is_empty()\n    \
         let b = [1].is_empty()\n    \
         let s = [a, b].fold(\"\", |acc, v| if v { \"{acc}T\" } else { \"{acc}F\" })\n    \
         io.println(s)",
    );
    assert_runs(
        &src,
        &[],
        "none\nnone\nsome 4\nsome 4\nsome 1\nsome 3\nTF\n",
    );
}

#[test]
fn reverse_and_concat_build_new_lists() {
    let src = main_with(
        "    let none: List<Int> = []\n    \
         let many = [1, 2, 3]\n    \
         io.println(show(none.reverse()))?\n    \
         io.println(show([9].reverse()))?\n    \
         io.println(show(many.reverse()))?\n    \
         io.println(show(many))?\n    \
         io.println(show(none.concat(none)))?\n    \
         io.println(show(none.concat([1])))?\n    \
         io.println(show([1].concat(none)))?\n    \
         io.println(show(many.concat([4, 5])))",
    );
    assert_runs(&src, &[], "\n9,\n3,2,1,\n1,2,3,\n\n1,\n1,\n1,2,3,4,5,\n");
}

#[test]
fn the_methods_work_in_a_chain_and_on_lists_of_options() {
    let src = main_with(
        "    let xs = [1, 2, 3, 4, 5, 6]\n    \
         io.println(show(xs.filter(|x| x > 1).reverse().concat([0])))?\n    \
         let o = [Some(1), None]\n    \
         let n = o.filter(|v| v.is_some()).len()\n    \
         io.println(\"{n}\")",
    );
    assert_runs(&src, &[], "6,5,4,3,2,0,\n1\n");
}

#[test]
fn a_user_function_named_like_a_method_is_untouched() {
    let src = "fn filter(n: Int) -> Int {\n    n + 1\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let n = filter(1)\n    io.println(\"{n}\")\n}\n";
    assert_runs(src, &[], "2\n");
}

#[test]
fn closure_calls_and_loop_back_edges_are_steps() {
    // `filter` over 100 items: the call of `list_filter`, 100 closure
    // calls and 100 loop back-edges, more than a limit of 20.
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               var xs: List<Int> = []\n    \
               for i in 0..100 {\n        xs.push(i)\n    }\n    \
               let n = xs.filter(|x| x > 50).len()\n    \
               io.println(\"{n}\")\n}\n";
    let outcome = run_under_limit_steps(src, 20);
    assert!(outcome.panic.is_some(), "{outcome:#?}");
    assert_runs(src, &[], "49\n");
}

fn run_under_limit_steps(src: &str, steps: u64) -> oska_run::RunOutcome {
    let limits = oska_run::Limits {
        max_steps: Some(steps),
        ..oska_run::Limits::NONE
    };
    oska_run::run_with_limits(
        src,
        &mut oska_run::FakeHost::new(),
        &[common::PROGRAM_PATH.to_string()],
        limits,
    )
}
