//! Native lists and value semantics (ticket #132), run on both backends
//! through the differential harness (seam 1): list literals and every
//! `List` method for scalar, text, struct and nested-list items, `for`
//! over a list, list equality, `P0002`, copy-on-write aliasing, freed
//! lists releasing their items, and the `String` methods that make or
//! take a list (`split`, `lines`, `chars`, `join`). Also the text search
//! and copy paths the native text speed-up rewrote.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// The helpers every program has. Interpolation takes only names, so a
/// program prints through these.
const HELPERS: &str = r#"fn show(io: Stdout, text: String) {
    let _ = io.println(text)
}

fn yes(io: Stdout, b: Bool) {
    let _ = if b { io.println("true") } else { io.println("false") }
}

fn num(io: Stdout, n: Int) {
    let _ = io.println("{n}")
}
"#;

/// A program with `items` whose `main` runs `body` with `io: Stdout`.
fn program(items: &str, body: &str) -> String {
    format!("{HELPERS}\n{items}\n\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run the program on both backends, require the same outcome, and that
/// it printed `stdout` and exited 0.
fn assert_prints(name: &str, items: &str, body: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, &program(items, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

#[test]
fn text_search_and_copy_handle_every_needle_and_piece_size() {
    // Needles of 1 to 17 bytes, found at the start, the end, in every
    // byte of a word, many times over (the positions buffer grows), and
    // pieces of every length from 0 to 40 copied around them.
    let body = r#"    let base = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOP"
    var n = 1
    while n <= 17 {
        let needle = base.slice(3, 3 + n)
        let text = "{needle}..{needle}{needle}x{needle}"
        let replaced = text.replace(needle, "<>")
        show(io, replaced)
        yes(io, text.contains(needle))
        yes(io, base.contains("{needle}!"))
        n = n + 1
    }
    var gap = 0
    while gap <= 40 {
        let piece = base.slice(0, gap)
        let text = "|{piece}|{piece}|".repeat(3)
        let replaced = text.replace("|", "[]")
        num(io, replaced.byte_len())
        show(io, replaced.replace("[]", "|").replace(piece, "-"))
        gap = gap + 1
    }
    let many = "ab".repeat(5000)
    let swapped = many.replace("b", "cd")
    num(io, swapped.byte_len())
    yes(io, swapped.ends_with("acd"))
    let shorter = many.replace("ab", "")
    num(io, shorter.byte_len())
    let same = many.replace("zz", "y")
    yes(io, same == many)"#;
    let outcome = assert_same(&Case::file("text_search_sizes", &program("", body)));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(
        outcome.stdout.starts_with("<>..<><>x<>\ntrue\nfalse\n"),
        "{outcome:#?}"
    );
    assert!(
        outcome.stdout.ends_with("15000\ntrue\n0\ntrue\n"),
        "{outcome:#?}"
    );
}

#[test]
fn arguments_passed_in_any_order_arrive_intact() {
    // Parameters and arguments share registers, so a call that permutes
    // its caller's parameters needs a parallel move (with cycles).
    let items = r#"fn digits(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {
    ((((((a * 10 + b) * 10 + c) * 10 + d) * 10 + e) * 10 + f) * 10 + g) * 10 + h
}

fn shuffle(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {
    let x = digits(h, g, f, e, d, c, b, a)
    let y = digits(b, a, d, c, f, e, h, g)
    let z = digits(c, a, b, a, h, h, g, d)
    x + y + z
}"#;
    assert_prints(
        "argument_moves",
        items,
        "    num(io, shuffle(1, 2, 3, 4, 5, 6, 7, 8))",
        &format!("{}\n", 87654321 + 21436587 + 31218874),
    );
}
