//! Native integers, control flow and calls (ticket #129), run on both
//! backends through the differential harness (seam 1): arithmetic on
//! every integer type with its overflow and division panics, `for` over
//! ranges, calls and recursion with the call-depth limit, and integer
//! text (interpolation and `to_string`) on the runtime's heap.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// A program whose `main` runs `body`, after the items `items`, with
/// `io: Stdout`. (`?` is a later ticket's, so bodies print with
/// `let _ = io.println(..)`.)
fn program(items: &str, body: &str) -> String {
    format!("{items}\n\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run `src` on both backends, require the same outcome, and that it
/// printed `stdout` and exited 0.
fn assert_prints(name: &str, src: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// Run `src` on both backends, require the same outcome, and that it
/// printed `stdout`, then panicked with the report `stderr`, exit 101.
fn assert_panics(name: &str, src: &str, stdout: &str, stderr: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.stderr, stderr, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
}

#[test]
fn int_arithmetic_computes_what_the_interpreter_does() {
    let src = program(
        "fn check(io: Stdout, ok: Bool, what: String) {\n    \
         let _ = if ok { io.println(what) } else { io.println(\"wrong\") }\n}",
        "    check(io, 2 + 3 == 5, \"add\")\n    \
         check(io, 2 - 7 == -5, \"sub\")\n    \
         check(io, -6 * 7 == -42, \"mul\")\n    \
         check(io, -7 / 2 == -3, \"div truncates\")\n    \
         check(io, -7 % 2 == -1, \"rem takes the dividend's sign\")\n    \
         check(io, 7 % -2 == 1, \"rem by a negative\")\n    \
         check(io, 1 + 2 * 3 - 4 / 2 == 5, \"precedence\")",
    );
    assert_prints(
        "int_arithmetic",
        &src,
        "add\nsub\nmul\ndiv truncates\nrem takes the dividend's sign\nrem by a negative\nprecedence\n",
    );
}

#[test]
fn integers_print_through_interpolation_and_to_string() {
    let src = program(
        "",
        "    let zero = 0\n    \
         let small = -7\n    \
         let big = 9223372036854775807\n    \
         let least = -9223372036854775807 - 1\n    \
         let _ = io.println(\"{zero} {small} {big} {least}\")\n    \
         let _ = io.println(\"[{small}]: no text around {zero}\")\n    \
         let _ = io.println(1234567890.to_string())\n    \
         let text = \"ünï {big}\"\n    \
         let _ = io.println(text)",
    );
    assert_prints(
        "int_text",
        &src,
        "0 -7 9223372036854775807 -9223372036854775808\n\
         [-7]: no text around 0\n1234567890\nünï 9223372036854775807\n",
    );
}

#[test]
fn int_overflow_panics_with_p0001_and_the_interpreters_report() {
    // The column counts characters: `ünï` is three, in six bytes.
    let src = program(
        "fn pick(label: String, n: Int) -> Int {\n    n\n}",
        "    let big = 9223372036854775807\n    \
         let _ = io.println(\"before\")\n    \
         let r = pick(\"ünï\", big + 1)\n    \
         let _ = io.println(\"after {r}\")",
    );
    assert_panics(
        "int_add_overflow",
        &src,
        "before\n",
        "main.osk:8:25: error[P0001]: `9223372036854775807 + 1` overflows `Int`, which \
         ranges from -9223372036854775808 to 9223372036854775807 (bytes 164..171)\n",
    );
}

#[test]
fn every_int_operation_that_overflows_panics() {
    for (name, expr, message) in [
        (
            "int_sub",
            "least - 1",
            "`-9223372036854775808 - 1` overflows",
        ),
        (
            "int_mul",
            "least * 2",
            "`-9223372036854775808 * 2` overflows",
        ),
        (
            "int_mul_neg",
            "least * -1",
            "`-9223372036854775808 * -1` overflows",
        ),
        (
            "int_mul_big",
            "3037000500 * 3037000500",
            "`3037000500 * 3037000500` overflows",
        ),
        (
            "int_div",
            "least / -1",
            "`-9223372036854775808 / -1` overflows",
        ),
        (
            "int_neg",
            "-least",
            "`--9223372036854775808` overflows `Int` (",
        ),
    ] {
        let src = program(
            "",
            &format!(
                "    let least = -9223372036854775807 - 1\n    \
                 let r = {expr}\n    let _ = io.println(\"{{r}}\")"
            ),
        );
        let outcome = assert_same(&Case::file(name, &src));
        assert!(
            outcome.stderr.contains(message) && outcome.stderr.contains("error[P0001]"),
            "case `{name}`: {outcome:#?}"
        );
        assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
    }
}

#[test]
fn division_and_remainder_by_zero_panic_with_p0004() {
    for (name, op) in [("int_div_zero", "/"), ("int_rem_zero", "%")] {
        let src = program(
            "fn zero() -> Int {\n    0\n}",
            &format!("    let r = -5 {op} zero()\n    let _ = io.println(\"{{r}}\")"),
        );
        assert_panics(
            name,
            &src,
            "",
            &format!("main.osk:6:13: error[P0004]: division by zero: `-5 {op} 0` (bytes 62..73)\n"),
        );
    }
}

#[test]
fn the_most_negative_int_remainder_minus_one_is_zero() {
    let src = program(
        "",
        "    let least = -9223372036854775807 - 1\n    \
         let r = least % -1\n    let _ = io.println(\"{r}\")",
    );
    assert_prints("int_min_rem", &src, "0\n");
}
