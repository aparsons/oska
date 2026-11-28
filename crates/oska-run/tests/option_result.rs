// SPDX-License-Identifier: Apache-2.0
//! The core library's `Option` and `Result` methods (spec #660, ticket
//! #668, ADR 0034): `Option.map`, `and_then`, `unwrap_or_else`, `is_none`,
//! `filter` and `Result.map`, `map_err`, `and_then`, `unwrap_or_else`,
//! `is_err`, `ok`, `err`. Every program also runs natively on Linux
//! (`common/mod.rs`), so both backends give the same result. `unwrap` and
//! `expect` are refused.

mod common;

use common::{assert_refused, assert_runs};

const HELPERS: &str = "fn opt(o: Option<Int>) -> String {
    match o {
        Some(n) => \"some {n}\"
        None => \"none\"
    }
}
fn res(r: Result<Int, String>) -> String {
    match r {
        Ok(n) => \"ok {n}\"
        Err(e) => \"err {e}\"
    }
}
fn yes(b: Bool) -> String {
    if b { \"T\" } else { \"F\" }
}
fn half(n: Int) -> Option<Int> {
    if n % 2 == 0 { Some(n / 2) } else { None }
}
fn small(n: Int) -> Result<Int, String> {
    if n < 10 { Ok(n) } else { Err(\"big\") }
}
";

fn program(body: &str) -> String {
    format!("{HELPERS}fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n")
}

#[test]
fn option_map_and_filter() {
    let src = program(
        "    let a: Option<Int> = Some(4)\n    let n: Option<Int> = None\n    \
         io.println(opt(a.map(|x| x + 1)))?\n    \
         io.println(opt(n.map(|x| x + 1)))?\n    \
         io.println(opt(a.filter(|x| x > 3)))?\n    \
         io.println(opt(a.filter(|x| x > 9)))?\n    \
         io.println(opt(n.filter(|x| x > 3)))?",
    );
    assert_runs(&src, &[], "some 5\nnone\nsome 4\nnone\nnone\n");
}

#[test]
fn option_map_changes_the_type() {
    let src = program(
        "    let a: Option<Int> = Some(4)\n    \
         let s = a.map(|x| \"n{x}\")\n    \
         io.println(match s {\n        Some(t) => t\n        None => \"none\"\n    })?",
    );
    assert_runs(&src, &[], "n4\n");
}

#[test]
fn option_and_then_chains() {
    let src = program(
        "    let a: Option<Int> = Some(12)\n    let n: Option<Int> = None\n    \
         io.println(opt(a.and_then(half)))?\n    \
         io.println(opt(a.and_then(half).and_then(half)))?\n    \
         io.println(opt(a.and_then(half).and_then(half).and_then(half)))?\n    \
         io.println(opt(n.and_then(half)))?",
    );
    assert_runs(&src, &[], "some 6\nsome 3\nnone\nnone\n");
}

#[test]
fn option_unwrap_or_else_runs_the_function_only_for_none() {
    let src = program(
        "    let a: Option<Int> = Some(4)\n    let n: Option<Int> = None\n    \
         let x = a.unwrap_or_else(|| 0 - 1)\n    \
         let y = n.unwrap_or_else(|| 0 - 1)\n    \
         io.println(\"{x}\")?\n    io.println(\"{y}\")?\n    \
         io.println(yes(a.is_none()))?\n    \
         io.println(yes(n.is_none()))?",
    );
    assert_runs(&src, &[], "4\n-1\nF\nT\n");
}

#[test]
fn result_map_map_err_and_and_then() {
    let src = program(
        "    let a: Result<Int, String> = Ok(4)\n    let e: Result<Int, String> = Err(\"bad\")\n    \
         io.println(res(a.map(|x| x * 2)))?\n    \
         io.println(res(e.map(|x| x * 2)))?\n    \
         io.println(res(a.map_err(|m| \"{m}!\")))?\n    \
         io.println(res(e.map_err(|m| \"{m}!\")))?\n    \
         io.println(res(a.and_then(small)))?\n    \
         io.println(res(a.and_then(|x| small(x * 10))))?\n    \
         io.println(res(e.and_then(small)))?",
    );
    assert_runs(
        &src,
        &[],
        "ok 8\nerr bad\nok 4\nerr bad!\nok 4\nerr big\nerr bad\n",
    );
}

#[test]
fn result_map_err_changes_the_error_type() {
    let src = program(
        "    let e: Result<Int, String> = Err(\"bad\")\n    \
         let n = e.map_err(|m| m.len())\n    \
         io.println(match n {\n        Ok(_) => \"ok\"\n        Err(k) => \"err {k}\"\n    })?",
    );
    assert_runs(&src, &[], "err 3\n");
}

#[test]
fn result_unwrap_or_else_is_err_ok_and_err() {
    let src = program(
        "    let a: Result<Int, String> = Ok(4)\n    let e: Result<Int, String> = Err(\"bad\")\n    \
         let x = a.unwrap_or_else(|m| m.len())\n    \
         let y = e.unwrap_or_else(|m| m.len())\n    \
         io.println(\"{x}\")?\n    io.println(\"{y}\")?\n    \
         io.println(yes(a.is_err()))?\n    \
         io.println(yes(e.is_err()))?\n    \
         io.println(opt(a.ok()))?\n    \
         io.println(opt(e.ok()))?\n    \
         io.println(match a.err() {\n        Some(m) => m\n        None => \"none\"\n    })?\n    \
         io.println(match e.err() {\n        Some(m) => m\n        None => \"none\"\n    })?",
    );
    assert_runs(&src, &[], "4\n3\nF\nT\nsome 4\nnone\nnone\nbad\n");
}

#[test]
fn a_user_function_named_like_a_method_is_unaffected() {
    let src = "fn map(x: Int) -> Int { x + 1 }\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let y = map(1)\n    io.println(\"{y}\")?\n    Ok(())\n}\n";
    assert_runs(src, &[], "2\n");
}

#[test]
fn unwrap_and_expect_are_refused_with_the_alternatives() {
    for call in ["o.unwrap()", "o.expect(\"missing\")"] {
        let src = format!(
            "fn main(io: Stdout) -> Result<(), IoError> {{\n    \
             let o: Option<Int> = Some(1)\n    let n = {call}\n    Ok(())\n}}\n"
        );
        assert_refused(&src, "E0228");
    }
}
