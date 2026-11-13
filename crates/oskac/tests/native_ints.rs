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
fn every_digit_count_and_digit_pair_prints_as_the_interpreter_prints_it() {
    // Each power of ten and its neighbours, both signs, up to `Int`'s
    // ends, then every number below 100,000 folded into a checksum of
    // its text (digit pairs, odd and even lengths). `U64` above `Int`
    // prints only in overflow reports (`sized_integer_overflow_...`).
    let src = program(
        "fn show(io: Stdout, n: Int) {\n    \
         let m = 0 - n\n    let _ = io.println(\"{n} {m}\")\n}",
        "    var p = 1\n    var i = 0\n    \
         while i < 19 {\n        \
         show(io, p - 1)\n        show(io, p)\n        show(io, p + 1)\n        \
         if i < 18 {\n            p = p * 10\n        }\n        i = i + 1\n    }\n    \
         let big = 9223372036854775807\n    let least = 0 - big - 1\n    \
         let _ = io.println(\"{big} {least}\")\n    \
         show(io, 4294967295)\n    show(io, 4294967296)\n    show(io, 4294967199)\n    show(io, 4294967300)\n    \
         var sum = 0\n    var n = 0\n    \
         while n < 100000 {\n        \
         let text = \"{n}\"\n        let last = text.slice(text.len() - 1, text.len())\n        \
         let first = text.slice(0, 1)\n        \
         sum = (sum * 31 + text.len() * 7 + last.parse_int().unwrap_or(0) + first.parse_int().unwrap_or(0) * 3) % 1000000007\n        \
         n = n + 1\n    }\n    \
         let _ = io.println(\"{sum}\")",
    );
    let outcome = assert_same(&Case::file("int_digits", &src));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(
        outcome.stdout.contains("\n4294967295 -4294967295\n4294967296 -4294967296\n4294967199 -4294967199\n4294967300 -4294967300\n")
            && outcome.stdout.contains("\n999999999999999999 -999999999999999999\n")
            && outcome.stdout.contains("\n1000000000000000000 -1000000000000000000\n")
            && outcome
                .stdout
                .contains("\n9223372036854775807 -9223372036854775808\n"),
        "{outcome:#?}"
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

#[test]
fn loops_run_with_break_and_continue() {
    let src = program(
        "",
        "    var total = 0\n    \
         for i in 0..10 {\n        \
         if i == 2 {\n            continue\n        }\n        \
         if i == 7 {\n            break\n        }\n        \
         total = total + i\n    }\n    \
         let _ = io.println(\"for {total}\")\n    \
         for i in 5..5 {\n        let _ = io.println(\"never {i}\")\n    }\n    \
         for i in 3..-3 {\n        let _ = io.println(\"never {i}\")\n    }\n    \
         let from = -2\n    \
         for i in from..total - 20 {\n        let _ = io.println(\"from {i}\")\n    }\n    \
         var n = 0\n    var steps = 0\n    \
         while true {\n        \
         n = n + 1\n        \
         if n % 2 == 0 {\n            continue\n        }\n        \
         steps = steps + 1\n        \
         if n > 8 {\n            break\n        }\n    }\n    \
         let _ = io.println(\"while {n} {steps}\")\n    \
         var grid = 0\n    \
         for row in 0..3 {\n        for col in 0..4 {\n            \
         if col == row {\n                continue\n            }\n            \
         grid = grid * 10 + col\n        }\n    }\n    \
         let _ = io.println(\"grid {grid}\")",
    );
    assert_prints(
        "loops",
        &src,
        "for 19\nfrom -2\nwhile 9 5\ngrid 123023013\n",
    );
}

#[test]
fn a_for_loop_over_unsigned_bounds_compares_them_unsigned() {
    let src = program(
        "fn top() -> U64 {\n    18446744073709551615\n}",
        "    var count = 0\n    \
         let hi = top()\n    \
         let lo = hi - 3\n    \
         for i in lo..hi {\n        \
         if i > lo {\n            count = count + 1\n        }\n    }\n    \
         let _ = io.println(\"{count}\")",
    );
    assert_prints("for_unsigned", &src, "2\n");
}

#[test]
fn calls_and_recursion_compute_what_the_interpreter_does() {
    let src = program(
        "fn fib(n: Int) -> Int {\n    if n < 2 {\n        return n\n    }\n    \
         fib(n - 1) + fib(n - 2)\n}\n\n\
         fn gcd(a: Int, b: Int) -> Int {\n    if b == 0 { a } else { gcd(b, a % b) }\n}\n\n\
         fn sum8(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {\n    \
         a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7 + h * 8\n}\n\n\
         fn is_even(n: Int) -> Bool {\n    if n == 0 { true } else { is_odd(n - 1) }\n}\n\n\
         fn is_odd(n: Int) -> Bool {\n    if n == 0 { false } else { is_even(n - 1) }\n}",
        "    let f = fib(20)\n    let g = gcd(1071, 462)\n    \
         let s = sum8(1, 2, 3, 4, 5, 6, 7, 8)\n    \
         let _ = io.println(\"{f} {g} {s}\")\n    \
         if is_even(1000) && is_odd(777) {\n        let _ = io.println(\"parity\")\n    }",
    );
    assert_prints("calls", &src, "6765 21 204\nparity\n");
}

/// A program whose `main` recurses `depth` calls deep and prints the
/// depth it reached, as the interpreter's own test of `P0021` does.
fn recursing(depth: u32) -> String {
    program(
        "fn down(n: Int) -> Int {\n    if n == 0 { 0 } else { 1 + down(n - 1) }\n}",
        &format!("    let d = down({depth})\n    let _ = io.println(\"{{d}}\")"),
    )
}

#[test]
fn deep_but_bounded_recursion_runs() {
    assert_prints("deep_recursion", &recursing(20_000), "20000\n");
}

#[test]
fn runaway_recursion_panics_with_p0021_instead_of_crashing() {
    assert_panics(
        "runaway_recursion",
        &recursing(1_000_000),
        "",
        "main.osk:2:32: error[P0021]: calls nest too deep: is this recursion missing its \
         base case? (bytes 56..67)\n",
    );
}

/// The interpreter's limit is `oska_run`'s `MAX_CALL_DEPTH`, but a debug
/// build of the interpreter runs out of its own stack a little before it
/// (and panics with `P0021` early, by design), so the exact boundary is
/// checked against the native executable alone: `main` may nest exactly
/// that many calls, and one more panics.
#[test]
fn recursion_panics_exactly_at_the_interpreters_call_depth() {
    assert_eq!(
        oska_native::MAX_CALL_DEPTH as usize,
        oska_run::MAX_CALL_DEPTH
    );
    let limit = oska_native::MAX_CALL_DEPTH;
    // `down(n)` nests `n + 1` calls under `main`.
    let deepest = differential::run_native(&Case::file("depth_limit_ok", &recursing(limit - 1)));
    assert_eq!(deepest.stdout, format!("{}\n", limit - 1), "{deepest:#?}");
    assert_eq!(deepest.exit_code, Some(0));
    let too_deep = differential::run_native(&Case::file("depth_limit_over", &recursing(limit)));
    assert!(too_deep.stderr.contains("error[P0021]"), "{too_deep:#?}");
    assert_eq!(too_deep.exit_code, Some(101));
}

#[test]
fn heap_text_keeps_its_value_through_locals_calls_and_loops() {
    let src = program(
        "fn label(n: Int) -> String {\n    let text = \"n{n}\"\n    text\n}\n\n\
         fn pick(a: String, b: String, first: Bool) -> String {\n    \
         if first { a } else { b }\n}\n\n\
         fn first_long(limit: Int) -> String {\n    \
         for i in 0..limit {\n        let text = label(i * 1000)\n        \
         if i > 2 {\n            return text\n        }\n    }\n    \"none\"\n}",
        "    var last = \"start\"\n    \
         for i in 0..6 {\n        \
         let shown = label(i)\n        \
         if i == 4 {\n            continue\n        }\n        \
         last = pick(shown, last, i % 2 == 0)\n        \
         let _ = io.println(last)\n    }\n    \
         let copy = last\n    \
         last = \"replaced\"\n    \
         let _ = io.println(\"{copy} {last}\")\n    \
         let _ = io.println(first_long(10))\n    \
         let _ = io.println(first_long(1))\n    \
         var grown = \"\"\n    \
         var i = 0\n    \
         while true {\n        \
         let piece = i.to_string()\n        \
         grown = \"{grown}{piece}\"\n        \
         i = i + 1\n        \
         if i == 12 {\n            break\n        }\n    }\n    \
         let _ = io.println(grown)",
    );
    assert_prints(
        "heap_text",
        &src,
        "n0\nn0\nn2\nn2\nn2\nn2 replaced\nn3000\nnone\n01234567891011\n",
    );
}

/// Building text in a loop that keeps none of it runs in constant
/// memory: each text is freed when its last reference goes. The process
/// runs with an address space far smaller than the text it builds in
/// all, which a leak would exceed. (Native only: the interpreter needs
/// more address space than the limit allows.)
#[test]
fn heap_text_is_freed_when_its_count_drops_to_zero() {
    let src = program(
        "fn label(n: Int) -> String {\n    \"label {n}\"\n}",
        "    var last = \"\"\n    \
         for i in 0..3000000 {\n        \
         let text = label(i)\n        \
         last = \"{text}!\"\n    }\n    \
         let _ = io.println(last)",
    );
    let case = Case::file("heap_text_freed", &src);
    let (exe, dir) = differential::build_native(&case);
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("ulimit -v 98304 && exec \"$0\"")
        .arg(&exe)
        .current_dir(&dir)
        .output()
        .expect("sh should run the program");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "label 2999999!\n",
        "{output:?}"
    );
    assert!(output.status.success(), "{output:?}");
}

/// Each sized integer type, with its smallest and largest values.
const SIZED: [(&str, &str, &str); 8] = [
    ("I8", "-128", "127"),
    ("I16", "-32768", "32767"),
    ("I32", "-2147483648", "2147483647"),
    ("I64", "-9223372036854775808", "9223372036854775807"),
    ("U8", "0", "255"),
    ("U16", "0", "65535"),
    ("U32", "0", "4294967295"),
    ("U64", "0", "18446744073709551615"),
];

#[test]
fn sized_integers_compute_within_their_range() {
    for (ty, min, max) in SIZED {
        // Each check prints its name only when it holds.
        let src = program(
            &format!(
                "fn check(io: Stdout, ok: Bool, what: String) {{\n    \
                 if ok {{\n        let _ = io.println(what)\n    }}\n}}\n\n\
                 fn least() -> {ty} {{\n    {min}\n}}\n\n\
                 fn most() -> {ty} {{\n    {max}\n}}"
            ),
            &format!(
                "    let lo = least()\n    let hi = most()\n    \
                 let two: {ty} = 2\n    let five: {ty} = 5\n    \
                 check(io, hi - 1 + 1 == hi, \"top\")\n    \
                 check(io, lo + 1 - 1 == lo, \"bottom\")\n    \
                 check(io, hi / two * two + hi % two == hi, \"div and rem\")\n    \
                 check(io, five * two == 10, \"mul\")\n    \
                 check(io, lo < hi && hi > lo && lo <= lo && hi >= hi, \"order\")\n    \
                 check(io, hi != lo, \"ne\")"
            ),
        );
        assert_prints(
            &format!("sized_{ty}"),
            &src,
            "top\nbottom\ndiv and rem\nmul\norder\nne\n",
        );
    }
}

#[test]
fn sized_integer_overflow_panics_with_the_interpreters_message() {
    for (ty, min, max) in SIZED {
        let signed = min != "0";
        let mut cases = vec![
            ("add", "hi + one", format!("`{max} + 1` overflows `{ty}`")),
            ("sub", "lo - one", format!("`{min} - 1` overflows `{ty}`")),
            ("mul", "hi * two", format!("`{max} * 2` overflows `{ty}`")),
            (
                "div0",
                "hi / zero",
                format!("division by zero: `{max} / 0`"),
            ),
            (
                "rem0",
                "lo % zero",
                format!("division by zero: `{min} % 0`"),
            ),
        ];
        if signed {
            cases.push((
                "divneg",
                "lo / -one",
                format!("`{min} / -1` overflows `{ty}`"),
            ));
            cases.push(("neg", "-lo", format!("`-{min}` overflows `{ty}` (")));
        }
        for (what, expr, message) in cases {
            let src = program(
                &format!(
                    "fn least() -> {ty} {{\n    {min}\n}}\n\n\
                     fn most() -> {ty} {{\n    {max}\n}}"
                ),
                &format!(
                    "    let lo = least()\n    let hi = most()\n    \
                     let zero: {ty} = 0\n    let one: {ty} = 1\n    let two: {ty} = 2\n    \
                     let r = {expr}\n    let _ = io.println(\"unreachable\")"
                ),
            );
            let outcome = assert_same(&Case::file(&format!("sized_{ty}_{what}"), &src));
            assert!(
                outcome.stderr.contains(&message),
                "`{ty}` {what}: {outcome:#?}, expected {message}"
            );
            assert_eq!(outcome.exit_code, Some(101), "`{ty}` {what}: {outcome:#?}");
        }
    }
}

#[test]
fn small_functions_with_branches_and_loops_give_the_same_results_inlined() {
    // Call-free functions with several returns and loops of their own,
    // which the backend copies into their callers: called in a loop, in a
    // condition, nested, and twice in one expression, each result printed.
    let src = program(
        "fn sign(x: Int) -> Int {\n    if x < 0 {\n        return 0 - 1\n    }\n    \
         if x > 0 {\n        return 1\n    }\n    0\n}\n\n\
         fn ones(x: Int) -> Int {\n    var n = x\n    var count = 0\n    \
         while n > 0 {\n        if n % 2 == 1 {\n            count = count + 1\n        }\n        \
         n = n / 2\n    }\n    count\n}\n\n\
         fn clamp(x: Int, low: Int, high: Int) -> Int {\n    if x < low {\n        return low\n    }\n    \
         if x > high {\n        return high\n    }\n    x\n}",
        "    var total = 0\n    var i = 0 - 5\n    \
         while i < 300 {\n        \
         if sign(i) < 0 {\n            total = total + 1000\n        }\n        \
         total = total + ones(clamp(i, 0, 255)) * sign(i) + ones(i) + clamp(sign(i), 0, 1)\n        \
         i = i + 1\n    }\n    \
         let a = sign(0 - 9)\n    let b = ones(1023)\n    let c = clamp(7, 1, 3)\n    \
         let _ = io.println(\"{total} {a} {b} {c}\")",
    );
    let outcome = assert_same(&Case::file("inlined_branches", &src));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(outcome.stdout.ends_with(" -1 10 3\n"), "{outcome:#?}");
}
