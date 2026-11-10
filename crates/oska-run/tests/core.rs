//! Core execution (#61): integers, bindings, control flow, calls, and
//! panics.

mod common;

use common::{assert_exits, assert_panics, assert_refused, assert_runs};

/// A `main` that runs `body` with `io: Stdout` in scope.
fn main_with(body: &str) -> String {
    format!("fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// A program of `items` and a `main` that runs `body`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\n{}", main_with(body))
}

// ---- Integers ------------------------------------------------------------

#[test]
fn arithmetic_follows_precedence_and_prints_through_interpolation() {
    assert_runs(
        &main_with(
            "    let a = 2\n    let b = a * 3 + 10 / 4 - 7 % 4\n    io.println(\"b is {b}\")",
        ),
        &[],
        "b is 5\n",
    );
}

#[test]
fn division_truncates_toward_zero_and_negation_works() {
    assert_runs(
        &main_with("    let a = -7\n    let q = a / 2\n    let r = a % 2\n    let n = -q\n    io.println(\"{q} {r} {n}\")"),
        &[],
        "-3 -1 3\n",
    );
}

#[test]
fn int_overflow_panics_with_p0001_at_the_operation() {
    assert_panics(
        &main_with(
            "    let big = 9223372036854775807\n    let more = big + 1\n    io.println(\"{more}\")",
        ),
        "P0001",
        "big + 1",
    );
}

#[test]
fn multiplying_past_the_range_of_int_panics_with_p0001() {
    assert_panics(
        &main_with(
            "    let big = 3037000500\n    let square = big * big\n    io.println(\"{square}\")",
        ),
        "P0001",
        "big * big",
    );
}

#[test]
fn negating_the_smallest_int_panics_with_p0001() {
    assert_panics(
        &main_with("    let small = -9223372036854775808\n    let flipped = -small\n    io.println(\"{flipped}\")"),
        "P0001",
        "-small",
    );
}

#[test]
fn sized_integers_overflow_at_their_own_range() {
    assert_panics(
        &main_with("    let x: U8 = 250\n    let y: U8 = x + 6\n    io.println(\"unreachable\")"),
        "P0001",
        "x + 6",
    );
    assert_panics(
        &main_with("    let x: U8 = 0\n    let y = x - 1\n    io.println(\"unreachable\")"),
        "P0001",
        "x - 1",
    );
    assert_panics(
        &main_with("    let x: I8 = -128\n    let y = x / -1\n    io.println(\"unreachable\")"),
        "P0001",
        "x / -1",
    );
}

#[test]
fn sized_integers_compute_within_their_range() {
    assert_runs(
        &main_with("    let x: U8 = 250\n    let y: U8 = x + 5\n    let z: I8 = -128\n    let w = z + 127\n    let big: U64 = 18446744073709551615\n    let half = big / 2\n    if y == 255 && w == -1 && half == 9223372036854775807 {\n        let _ = io.println(\"in range\")\n    }\n    io.println(\"done\")"),
        &[],
        "in range\ndone\n",
    );
}

#[test]
fn division_by_zero_panics_with_p0004_at_the_operation() {
    assert_panics(
        &main_with("    let zero = 0\n    let q = 10 / zero\n    io.println(\"{q}\")"),
        "P0004",
        "10 / zero",
    );
}

#[test]
fn remainder_by_zero_panics_with_p0004() {
    assert_panics(
        &main_with("    let zero: U16 = 0\n    let r = 10 % zero\n    io.println(\"unreachable\")"),
        "P0004",
        "10 % zero",
    );
}

#[test]
fn a_panic_keeps_what_was_printed_before_it() {
    let outcome = common::run_fake(
        &main_with("    let _ = io.println(\"before\")\n    let n: Int = todo()\n    io.println(\"after\")"),
        &[],
    );
    assert_eq!(outcome.stdout, "before\n");
    assert_eq!(outcome.exit_code, 101);
}

// ---- Bindings and assignment ------------------------------------------------

#[test]
fn bools_and_comparisons() {
    assert_runs(
        &main_with(
            "    let a = 3 < 4 && !(2 >= 5)\n    let b = 3 == 3 || todo()\n    let c = false && todo()\n    let d = false implies todo()\n    if a && b && !c && d {\n        let _ = io.println(\"all hold\")\n    }\n    if \"x\" != \"y\" && true == true {\n        let _ = io.println(\"text and bools compare\")\n    }\n    io.println(\"done\")",
        ),
        &[],
        "all hold\ntext and bools compare\ndone\n",
    );
}

#[test]
fn var_assignment_replaces_the_value_without_changing_a_copy() {
    assert_runs(
        &main_with(
            "    var n = 1\n    let copy = n\n    n = n + 41\n    io.println(\"{n} {copy}\")",
        ),
        &[],
        "42 1\n",
    );
}

#[test]
fn shadowing_and_block_scopes_keep_slots_apart() {
    assert_runs(
        &main_with("    let x = 1\n    let y = {\n        let x = 10\n        let z = x + 1\n        z\n    }\n    let x = x + y\n    io.println(\"{x} {y}\")"),
        &[],
        "12 11\n",
    );
}

// ---- Control flow -----------------------------------------------------------

#[test]
fn if_else_chooses_a_branch_and_has_a_value() {
    assert_runs(
        &program(
            "fn sign(n: Int) -> String {\n    if n < 0 {\n        \"negative\"\n    } else if n == 0 {\n        \"zero\"\n    } else {\n        \"positive\"\n    }\n}",
            "    let a = sign(-5)\n    let b = sign(0)\n    let c = sign(7)\n    io.println(\"{a} {b} {c}\")",
        ),
        &[],
        "negative zero positive\n",
    );
}

#[test]
fn while_loops_until_its_condition_fails() {
    assert_runs(
        &main_with("    var i = 0\n    var total = 0\n    while i < 5 {\n        i = i + 1\n        total = total + i\n    }\n    io.println(\"{total}\")"),
        &[],
        "15\n",
    );
}

#[test]
fn for_iterates_over_a_range_excluding_its_end() {
    assert_runs(
        &main_with("    for i in 0..3 {\n        let _ = io.println(\"{i}\")\n    }\n    let lo = 5\n    for _ in lo..lo {\n        let _ = io.println(\"never\")\n    }\n    io.println(\"done\")"),
        &[],
        "0\n1\n2\ndone\n",
    );
}

#[test]
fn break_leaves_and_continue_skips_in_both_loops() {
    assert_runs(
        &main_with("    for i in 0..10 {\n        if i % 2 == 1 {\n            continue\n        }\n        if i == 6 {\n            break\n        }\n        let _ = io.println(\"{i}\")\n    }\n    var n = 0\n    while true {\n        n = n + 1\n        if n < 3 {\n            continue\n        }\n        break\n    }\n    io.println(\"n {n}\")"),
        &[],
        "0\n2\n4\nn 3\n",
    );
}

#[test]
fn break_leaves_only_the_innermost_loop() {
    assert_runs(
        &main_with("    for i in 0..2 {\n        for j in 0..5 {\n            if j == 1 {\n                break\n            }\n            let _ = io.println(\"{i} {j}\")\n        }\n    }\n    io.println(\"done\")"),
        &[],
        "0 0\n1 0\ndone\n",
    );
}

// ---- Calls and return ---------------------------------------------------------

#[test]
fn functions_call_each_other_and_recurse() {
    assert_runs(
        &program(
            "fn square(n: Int) -> Int {\n    n * n\n}\n\nfn factorial(n: Int) -> Int {\n    if n <= 1 {\n        return 1\n    }\n    n * factorial(n - 1)\n}",
            "    let s = square(7)\n    let f = factorial(10)\n    io.println(\"{s} {f}\")",
        ),
        &[],
        "49 3628800\n",
    );
}

#[test]
fn return_leaves_the_function_from_inside_a_loop() {
    assert_runs(
        &program(
            "fn first_multiple(of: Int, above: Int) -> Int {\n    var n = above\n    while true {\n        n = n + 1\n        if n % of == 0 {\n            return n\n        }\n    }\n    0\n}",
            "    let m = first_multiple(7, 30)\n    io.println(\"{m}\")",
        ),
        &[],
        "35\n",
    );
}

#[test]
fn a_function_can_take_and_use_a_capability() {
    assert_runs(
        &program(
            "fn greet(out: Stdout, name: String) -> Result<(), IoError> {\n    out.println(\"hello {name}\")\n}",
            "    let _ = greet(io, \"oska\")\n    greet(io, \"again\")",
        ),
        &[],
        "hello oska\nhello again\n",
    );
}

#[test]
fn associated_functions_are_called_through_their_type() {
    assert_runs(
        &program(
            "struct Math {}\n\nimpl Math {\n    fn double(n: Int) -> Int {\n        n * 2\n    }\n}",
            "    let d = Math.double(21)\n    io.println(\"{d}\")",
        ),
        &[],
        "42\n",
    );
}

#[test]
fn a_panic_in_a_called_function_spans_the_operation_there() {
    assert_panics(
        &program(
            "fn halve(n: Int, by: Int) -> Int {\n    n / by\n}",
            "    let h = halve(4, 0)\n    io.println(\"{h}\")",
        ),
        "P0004",
        "n / by",
    );
}

#[test]
fn a_main_that_returns_unit_exits_with_code_0() {
    assert_exits(
        "fn main(io: Stdout) {\n    let _ = io.println(\"x\")\n}\n",
        0,
        "",
    );
}

// ---- Refusals -------------------------------------------------------------------

#[test]
fn a_program_using_something_not_yet_supported_is_refused_with_e0380() {
    let src = main_with("    let n = \"  hi \".trim()\n    io.println(n)");
    assert_refused(&src, "E0380");
    assert_eq!(common::run_fake(&src, &[]).exit_code, 2);
}
