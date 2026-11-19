//! Programs in Oska Core (ADR 0018, #425): functions, `let`, `if`, `Int`
//! and `Bool`, each a whole literal, so that `lean_reference.rs` also runs
//! them on the Lean reference and compares. A program in Oska Core cannot
//! print, so each one shows what it computed by how it ends: `check`
//! panics with `P0004` on a false condition, and a run that returns
//! cleanly passed every check.

mod common;

use common::{assert_panics, assert_runs};

#[test]
fn integer_arithmetic_rounds_toward_zero_and_compares() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn main() {
    check(2 + 3 * 4 == 14)
    check(10 - 4 - 3 == 3)
    check(-7 / 2 == -3)
    check(-7 % 2 == -1)
    check(7 % -2 == 1)
    check(-(5 - 8) == 3)
    let min = -9223372036854775807 - 1
    let minus_one = -1
    check(min < 0 && min % minus_one == 0)
    check(9223372036854775807 > 0)
    check(1 < 2 && 2 <= 2 && 3 > 2 && 3 >= 3 && 1 != 2)
    check(!(1 == 2) && true != false && true == true)
}
",
        &[],
        "",
    );
}

#[test]
fn logical_operators_evaluate_rhs_only_when_needed() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn boom() -> Bool {
    let zero = 0
    1 / zero == 0
}

fn main() {
    let a = false && boom()
    let b = true || boom()
    let c = false implies boom()
    check(!a && b && c)
    check(true implies true)
    check(!(true implies false))
    check(true && true && !(true && false))
    check(false || true)
}
",
        &[],
        "",
    );
}

#[test]
fn recursion_lets_blocks_and_ifs_compute_values() {
    assert_runs(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn fact(n: Int) -> Int {
    if n <= 1 {
        1
    } else {
        n * fact(n - 1)
    }
}

fn fib(n: Int) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn sum_to(n: Int) -> Int {
    if n == 0 {
        0
    } else {
        n + sum_to(n - 1)
    }
}

fn even(n: Int) -> Bool {
    if n == 0 {
        true
    } else {
        odd(n - 1)
    }
}

fn odd(n: Int) -> Bool {
    if n == 0 {
        false
    } else {
        even(n - 1)
    }
}

fn second(_: Int, b: Int) -> Int {
    b
}

fn main() {
    check(fact(20) == 2432902008176640000)
    check(fib(15) == 610)
    check(sum_to(1000) == 500500)
    check(even(10) && odd(7) && !even(3))
    check(second(99, 7) == 7)
    let x = 1
    let x = x + 1
    let y = {
        let x = x * 10
        x + 1
    }
    check(x == 2 && y == 21)
    let z = if x > 1 {
        3
    } else {
        4
    }
    check(z == 3)
    if x == 2 {
        check(true)
    }
    let _ = fact(3)
}
",
        &[],
        "",
    );
}

#[test]
fn a_false_check_panics_with_p0004() {
    assert_panics(
        "fn fail() {
    let zero = 0
    let _ = 1 / zero
}

fn check(ok: Bool) {
    if !ok {
        fail()
    }
}

fn main() {
    check(1 + 1 == 3)
}
",
        "P0004",
        "1 / zero",
    );
}

#[test]
fn a_product_past_int_panics_with_p0001() {
    assert_panics(
        "fn fact(n: Int) -> Int {
    if n <= 1 {
        1
    } else {
        n * fact(n - 1)
    }
}

fn main() {
    let big = fact(21)
}
",
        "P0001",
        "n * fact(n - 1)",
    );
}

#[test]
fn the_smallest_int_divided_by_minus_one_panics_with_p0001() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let minus_one = -1
    let q = min / minus_one
}
",
        "P0001",
        "min / minus_one",
    );
}

#[test]
fn negating_the_smallest_int_panics_with_p0001() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let flipped = -min
}
",
        "P0001",
        "-min",
    );
}

#[test]
fn a_remainder_by_zero_panics_with_p0004_before_any_overflow() {
    assert_panics(
        "fn main() {
    let min = -9223372036854775807 - 1
    let zero = 0
    let r = min % zero
}
",
        "P0004",
        "min % zero",
    );
}

#[test]
fn operands_and_arguments_run_left_to_right() {
    assert_panics(
        "fn overflow() -> Int {
    let max = 9223372036854775807
    max + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn pair(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let first = pair(overflow(), divide())
}
",
        "P0001",
        "max + 1",
    );
    assert_panics(
        "fn overflow() -> Int {
    let max = 9223372036854775807
    max + 1
}

fn divide() -> Int {
    let zero = 0
    1 / zero
}

fn main() {
    let first = divide() + overflow()
}
",
        "P0004",
        "1 / zero",
    );
}
