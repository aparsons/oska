//! A closure that captures a `Scope` handle is scoped: like the handle
//! itself, it cannot outlive its `scope` block (issue #44, `E0292`).

mod common;

use common::{assert_errors, assert_ok};

const WORK: &str = "fn work() -> Int {\n    1\n}\n";

#[test]
fn a_scope_block_cannot_produce_a_closure_that_captures_its_handle() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    let later = scope |s| {{ || s.spawn(|| work()) }}\n    later()\n    1\n}}"
    );
    assert_errors(&src, &[("E0292", "scope |s| { || s.spawn(|| work()) }")]);
}

#[test]
fn a_scope_block_cannot_produce_such_a_closure_inside_a_wrapper() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    let later = scope |s| {{ Some(|| s.spawn(|| work())) }}\n    1\n}}"
    );
    assert_errors(
        &src,
        &[("E0292", "scope |s| { Some(|| s.spawn(|| work())) }")],
    );
}

#[test]
fn a_closure_that_captures_such_a_closure_is_scoped_too() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    let later = scope |s| {{\n        let go = || s.spawn(|| work())\n        || go()\n    }}\n    1\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0292",
            "scope |s| {\n        let go = || s.spawn(|| work())\n        || go()\n    }",
        )],
    );
}

#[test]
fn a_function_cannot_return_a_closure_that_captures_a_scope() {
    let src =
        format!("{WORK}fn leak(s: Scope) -> fn() -> Task<Int> {{\n    || s.spawn(|| work())\n}}");
    assert_errors(&src, &[("E0292", "|| s.spawn(|| work())")]);
}

#[test]
fn a_field_cannot_hold_a_closure_that_captures_a_scope() {
    let src = format!(
        "{WORK}struct Later {{ go: fn() -> Task<Int> }}\n\
         fn leak() -> Int {{\n    scope |s| {{\n        let l = Later(go: || s.spawn(|| work()))\n        1\n    }}\n}}"
    );
    assert_errors(&src, &[("E0292", "|| s.spawn(|| work())")]);
}

#[test]
fn a_closure_that_captures_a_scope_cannot_be_passed_to_a_user_function() {
    let src = format!(
        "{WORK}fn keep(f: fn() -> Task<Int>) -> fn() -> Task<Int> {{\n    f\n}}\n\
         fn leak() -> Int {{\n    scope |s| {{\n        keep(|| s.spawn(|| work()))\n        1\n    }}\n}}"
    );
    assert_errors(&src, &[("E0292", "|| s.spawn(|| work())")]);
}

#[test]
fn a_closure_that_captures_a_scope_can_be_used_inside_its_block() {
    let src = format!(
        "{WORK}fn fine(xs: List<Int>) -> Int {{\n    scope |s| {{\n        let go = || s.spawn(|| work())\n        let a = go().join()\n        let tasks = xs.map(|x| s.spawn(|| x + work()))\n        let b = s.spawn(|| s.spawn(|| work()).join()).join()\n        a + b\n    }}\n}}"
    );
    assert_ok(&src);
}

#[test]
fn a_var_outside_the_block_cannot_keep_a_closure_that_captures_its_handle() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    var later = None\n    scope |s| {{\n        later = Some(|| s.spawn(|| work()))\n    }}\n    1\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0292",
            "scope |s| {\n        later = Some(|| s.spawn(|| work()))\n    }",
        )],
    );
}

#[test]
fn a_var_outside_the_block_cannot_keep_its_handle() {
    let src = "fn leak() -> Int {\n    var keep = List.new()\n    scope |s| {\n        keep.push(s)\n    }\n    1\n}";
    assert_errors(
        src,
        &[("E0292", "scope |s| {\n        keep.push(s)\n    }")],
    );
}

// A `var` that already holds a handle, of an enclosing block, must not
// take the handle of an inner block: after the inner block ends it would
// hold a finished scope.

#[test]
fn an_outer_var_holding_a_handle_cannot_take_an_inner_blocks_handle() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    scope |a| {{\n        var h = a\n        scope |b| {{\n            h = b\n        }}\n        h.spawn(|| work()).join()\n    }}\n}}"
    );
    assert_errors(
        &src,
        &[("E0292", "scope |b| {\n            h = b\n        }")],
    );
}

#[test]
fn an_outer_var_holding_a_scoped_closure_cannot_take_one_capturing_an_inner_handle() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    scope |a| {{\n        var go = || a.spawn(|| work())\n        scope |b| {{\n            go = || b.spawn(|| work())\n        }}\n        go().join()\n    }}\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0292",
            "scope |b| {\n            go = || b.spawn(|| work())\n        }",
        )],
    );
}

#[test]
fn an_outer_list_of_handles_cannot_take_an_inner_blocks_handle() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    scope |a| {{\n        var l = [a]\n        scope |b| {{\n            l.push(b)\n        }}\n        l.at(1).spawn(|| work()).join()\n    }}\n}}"
    );
    assert_errors(
        &src,
        &[("E0292", "scope |b| {\n            l.push(b)\n        }")],
    );
}

#[test]
fn an_inner_handle_kept_through_a_local_of_the_inner_block_is_caught_too() {
    let src = format!(
        "{WORK}fn leak() -> Int {{\n    scope |a| {{\n        var h = a\n        scope |b| {{\n            let inner = Some(b)\n            h = inner.unwrap_or(a)\n        }}\n        h.spawn(|| work()).join()\n    }}\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0292",
            "scope |b| {\n            let inner = Some(b)\n            h = inner.unwrap_or(a)\n        }",
        )],
    );
}

#[test]
fn an_outer_list_may_take_the_outer_handle_in_an_inner_block() {
    let src = format!(
        "{WORK}fn fine() -> Int {{\n    scope |a| {{\n        var l = [a]\n        scope |b| {{\n            l.push(a)\n            let t = b.spawn(|| work())\n        }}\n        l.at(1).spawn(|| work()).join()\n    }}\n}}"
    );
    assert_ok(&src);
}

#[test]
fn a_var_that_already_held_an_outer_handle_may_be_reassigned_in_an_inner_block() {
    let src = format!(
        "{WORK}fn fine() -> Int {{\n    scope |a| {{\n        var go = Some(|| a.spawn(|| work()))\n        scope |b| {{\n            go = Some(|| a.spawn(|| work()))\n        }}\n        1\n    }}\n}}"
    );
    assert_ok(&src);
}
