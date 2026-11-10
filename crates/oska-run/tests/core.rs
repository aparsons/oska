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
    // Every prelude function now runs, so the example is generic dispatch
    // to a sized integer's own impl, which type tags cannot tell apart.
    let src = program(
        "trait Name {\n    fn name(self) -> String\n}\n\n\
         impl Name for Int {\n    fn name(self) -> String { \"int\" }\n}\n\n\
         impl Name for U8 {\n    fn name(self) -> String { \"u8\" }\n}\n\n\
         fn show<T: Name>(x: T) -> Untrusted<String> { x.name() }",
        "    let b: U8 = 3\n    io.println(show(b))",
    );
    assert_refused(&src, "E0380");
    assert_eq!(common::run_fake(&src, &[]).exit_code, 2);
}

/// Assert that `src` is refused with `E0380`, with exactly `message`,
/// spanning `spanned_text`.
fn assert_unsupported(src: &str, message: &str, spanned_text: &str) {
    assert_refused(src, "E0380");
    let outcome = common::run_fake(src, &[]);
    let refusal = &outcome.diagnostics[0];
    assert_eq!(refusal.message, message);
    assert_eq!(&src[refusal.span.start..refusal.span.end], spanned_text);
}

#[test]
fn e0380_names_a_from_bound_conversion() {
    assert_unsupported(
        &program(
            "struct AppError { m: String }\n\
             impl From<IoError> for AppError {\n    fn from(e: IoError) -> AppError { AppError(m: \"io\") }\n}\n\
             fn conv<E: From<IoError>>(r: Result<Int, IoError>) -> Result<Int, E> {\n    let x = r?\n    Ok(x)\n}",
            "    let r: Result<Int, AppError> = conv(Ok(1))\n    Ok(())",
        ),
        "the interpreter cannot run a `From`-bound conversion yet",
        "r?",
    );
}

#[test]
fn e0380_names_a_constructor_used_as_a_value() {
    assert_unsupported(
        &main_with("    let mk = Some\n    let x: Option<Int> = mk(1)\n    Ok(())"),
        "the interpreter cannot run a constructor used as a function value yet",
        "Some",
    );
}

#[test]
fn e0380_names_a_generic_traits_method() {
    assert_unsupported(
        &program(
            "trait Pair<T> {\n    fn pair(self, t: T) -> Int\n}\n\
             struct P { n: Int }\n\
             impl Pair<Int> for P {\n    fn pair(self, t: Int) -> Int { t }\n}\n\
             fn use_it<T: Pair<Int>>(x: T) -> Int { x.pair(1) }",
            "    let n = use_it(P(n: 1))\n    Ok(())",
        ),
        "the interpreter cannot run a call of `pair`, a method of the generic trait `Pair`, yet",
        "x.pair(1)",
    );
}

// ---- Deep recursion -------------------------------------------------------------

/// A program whose `main` recurses `depth` calls deep and prints the
/// depth it reached.
fn recursing(depth: u32) -> String {
    program(
        "fn down(n: Int) -> Int {\n    if n == 0 { 0 } else { 1 + down(n - 1) }\n}",
        &format!("    let d = down({depth})\n    io.println(\"{{d}}\")"),
    )
}

#[test]
fn deep_but_bounded_recursion_runs() {
    assert_runs(&recursing(20_000), &[], "20000\n");
}

#[test]
fn runaway_recursion_panics_with_p0021_instead_of_crashing() {
    assert_panics(&recursing(1_000_000), "P0021", "down(n - 1)");
}
