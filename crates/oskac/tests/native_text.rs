//! Native text (ticket #131), run on both backends through the
//! differential harness (seam 1): every `String` method that does not
//! return a `List`, `String` equality, `Path` construction and the
//! `Untrusted<String>` operations, on ASCII, multi-byte and edge-case
//! text, with the interpreter's panics (`P0035`, `P0036`, `P0037`).
//! (`String` methods that return a `List` wait for lists, #132.)

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// The helpers every program has. Interpolation takes only names, so a
/// program prints through these: `show` a line, `bracket` a text between
/// `[` and `]`, `pair` two integers, `yes` a `Bool`.
const HELPERS: &str = r#"fn yes(io: Stdout, b: Bool) {
    let _ = if b { io.println("true") } else { io.println("false") }
}

fn show(io: Stdout, text: String) {
    let _ = io.println(text)
}

fn bracket(io: Stdout, text: String) {
    let _ = io.println("[{text}]")
}

fn pair(io: Stdout, a: Int, b: Int) {
    let _ = io.println("{a} {b}")
}
"#;

/// A program whose `main` runs `body` with `io: Stdout`.
fn program(body: &str) -> String {
    format!("{HELPERS}\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run `body` on both backends, require the same outcome, and that it
/// printed `stdout` and exited 0.
fn assert_prints(name: &str, body: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, &program(body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// Run `body` on both backends, require the same outcome, and that it
/// printed `stdout`, then panicked with a report containing `code` and
/// `message`, exit 101.
fn assert_panics(name: &str, body: &str, stdout: &str, code: &str, message: &str) {
    let outcome = assert_same(&Case::file(name, &program(body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains(&format!("error[{code}]: {message} (bytes")),
        "case `{name}`: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
}

#[test]
fn lengths_count_characters_and_bytes() {
    assert_prints(
        "text_lengths",
        r#"    let empty = ""
    let ascii = "abc"
    let accents = "ünï"
    let kanji = "日本語"
    let emoji = "a😀b"
    let long = "é".repeat(1000)
    let mixed = "abcdefgh日本語ijklmnopé😀qrstuvwxyz"
    pair(io, empty.len(), empty.byte_len())
    pair(io, ascii.len(), ascii.byte_len())
    pair(io, accents.len(), accents.byte_len())
    pair(io, kanji.len(), kanji.byte_len())
    pair(io, emoji.len(), emoji.byte_len())
    pair(io, long.len(), long.byte_len())
    pair(io, mixed.len(), mixed.byte_len())"#,
        "0 0\n3 3\n3 5\n3 9\n3 6\n1000 2000\n31 41\n",
    );
}

#[test]
fn searching_finds_what_the_interpreter_finds() {
    assert_prints(
        "text_searching",
        r#"    let text = "héllo wörld, héllo"
    yes(io, text.contains("wör"))
    yes(io, text.contains("wor"))
    yes(io, text.contains(""))
    yes(io, text.contains(text))
    yes(io, text.contains("héllo wörld, héllo!"))
    yes(io, "".contains(""))
    yes(io, "".contains("a"))
    yes(io, text.starts_with("hé"))
    yes(io, text.starts_with("he"))
    yes(io, text.starts_with(""))
    yes(io, text.starts_with("héllo wörld, héllo and more"))
    yes(io, text.ends_with("llo"))
    yes(io, text.ends_with("héllo"))
    yes(io, text.ends_with(""))
    yes(io, text.ends_with("x héllo wörld, héllo"))
    yes(io, "aab".contains("ab"))
    yes(io, "aaa".ends_with("aa"))"#,
        "true\nfalse\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\n\
         true\ntrue\ntrue\nfalse\ntrue\ntrue\n",
    );
}

#[test]
fn slice_takes_characters_not_bytes() {
    assert_prints(
        "text_slice",
        r#"    let text = "aé日😀z"
    let hello = "hello"
    let empty = ""
    bracket(io, text.slice(0, 5))
    bracket(io, text.slice(1, 3))
    bracket(io, text.slice(2, 4))
    bracket(io, text.slice(3, 4))
    bracket(io, text.slice(0, 0))
    bracket(io, text.slice(5, 5))
    bracket(io, text.slice(4, 5))
    bracket(io, empty.slice(0, 0))
    bracket(io, hello.slice(1, 4))
    bracket(io, hello.slice(0, 5))"#,
        "[aé日😀z]\n[é日]\n[日😀]\n[😀]\n[]\n[]\n[z]\n[]\n[ell]\n[hello]\n",
    );
}

#[test]
fn slice_out_of_range_panics_with_p0035() {
    for (name, args, message) in [
        (
            "slice_past_end",
            "1, 9",
            "cannot slice characters 1..9 of a text of 3 characters",
        ),
        (
            "slice_backwards",
            "2, 1",
            "cannot slice characters 2..1 of a text of 3 characters",
        ),
        (
            "slice_negative_start",
            "-1, 2",
            "cannot slice characters -1..2 of a text of 3 characters",
        ),
        (
            "slice_negative_end",
            "0, -1",
            "cannot slice characters 0..-1 of a text of 3 characters",
        ),
        (
            "slice_huge",
            "0, 9223372036854775807",
            "cannot slice characters 0..9223372036854775807 of a text of 3 characters",
        ),
    ] {
        assert_panics(
            name,
            &format!(
                "    show(io, \"before\")\n    let text = \"ünï\"\n    bracket(io, text.slice({args}))"
            ),
            "before\n",
            "P0035",
            message,
        );
    }
}

#[test]
fn repeat_builds_the_text_times_over() {
    assert_prints(
        "text_repeat",
        r#"    let ab = "ab"
    let accents = "é日"
    let empty = ""
    bracket(io, ab.repeat(3))
    bracket(io, accents.repeat(2))
    bracket(io, ab.repeat(0))
    bracket(io, ab.repeat(1))
    bracket(io, empty.repeat(1000000000))
    let x = "x"
    let big = x.repeat(268435456)
    show(io, "ok")
    pair(io, big.len(), big.byte_len())"#,
        "[ababab]\n[é日é日]\n[]\n[ab]\n[]\nok\n268435456 268435456\n",
    );
}

#[test]
fn repeat_panics_with_p0036_and_p0037() {
    assert_panics(
        "repeat_negative",
        "    show(io, \"before\")\n    let ab = \"ab\"\n    bracket(io, ab.repeat(-3))",
        "before\n",
        "P0036",
        "cannot repeat a text -3 times",
    );
    assert_panics(
        "repeat_negative_empty",
        "    let empty = \"\"\n    bracket(io, empty.repeat(-1))",
        "",
        "P0036",
        "cannot repeat a text -1 times",
    );
    assert_panics(
        "repeat_too_long",
        "    let e = \"é\"\n    bracket(io, e.repeat(134217729))",
        "",
        "P0037",
        "repeating a text of 2 bytes 134217729 times is too long",
    );
    assert_panics(
        "repeat_overflowing",
        "    let abc = \"abc\"\n    bracket(io, abc.repeat(9223372036854775807))",
        "",
        "P0037",
        "repeating a text of 3 bytes 9223372036854775807 times is too long",
    );
}

#[test]
fn trim_removes_unicode_whitespace_at_both_ends() {
    assert_prints(
        "text_trim",
        "    let a = \"  a b  \"\n    \
         let b = \"\"\n    \
         let c = \"   \"\n    \
         let d = \"\\t\\n x y \\r\\n\"\n    \
         let e = \"abc\"\n    \
         let f = \"\u{a0}\u{2003}é \u{3000}\"\n    \
         let g = \"\u{200b}z\u{200b}\"\n    \
         let h = \"\u{85}\u{2028}q\u{1680}\"\n    \
         let i = \"\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}k\u{202f}\u{205f}\"\n    \
         let j = \"\u{2029}\"\n    \
         bracket(io, a.trim())\n    \
         bracket(io, b.trim())\n    \
         bracket(io, c.trim())\n    \
         bracket(io, d.trim())\n    \
         bracket(io, e.trim())\n    \
         bracket(io, f.trim())\n    \
         bracket(io, g.trim())\n    \
         bracket(io, h.trim())\n    \
         bracket(io, i.trim())\n    \
         bracket(io, j.trim())",
        "[a b]\n[]\n[]\n[x y]\n[abc]\n[é]\n[\u{200b}z\u{200b}]\n[q]\n[k]\n[]\n",
    );
}

#[test]
fn replace_swaps_every_occurrence_left_to_right() {
    assert_prints(
        "text_replace",
        r#"    let dashes = "a-b-c"
    let aaaa = "aaaa"
    let aaa = "aaa"
    let hello = "héllo"
    let abc = "abc"
    let empty = ""
    let xx = "xx"
    let ab = "ab"
    bracket(io, dashes.replace("-", "+"))
    bracket(io, aaaa.replace("aa", "b"))
    bracket(io, aaa.replace("aa", "b"))
    bracket(io, hello.replace("é", "e"))
    bracket(io, abc.replace("", "-"))
    bracket(io, empty.replace("", "-"))
    bracket(io, empty.replace("a", "b"))
    bracket(io, abc.replace("b", ""))
    bracket(io, abc.replace("abc", ""))
    bracket(io, abc.replace("abcd", "x"))
    bracket(io, xx.replace("x", "xyx"))
    bracket(io, ab.replace("ab", "日本"))"#,
        "[a+b+c]\n[bb]\n[ba]\n[hello]\n[abc]\n[]\n[]\n[ac]\n[]\n[abc]\n[xyxxyx]\n[日本]\n",
    );
}

#[test]
fn text_equality_compares_bytes() {
    assert_prints(
        "text_equality",
        r#"    let a = "héllo"
    let b = "hé".repeat(1)
    yes(io, a == "héllo")
    yes(io, a != "héllo")
    yes(io, a == "hello")
    yes(io, a != "hello")
    yes(io, b == "hé")
    yes(io, "" == "")
    yes(io, "a" == "ab")
    yes(io, "ab" == "a")
    yes(io, "abcdefghij" == "abcdefghik")
    yes(io, "abcdefghijklmnop" == "abcdefghijklmnop")"#,
        "true\nfalse\nfalse\ntrue\ntrue\ntrue\nfalse\nfalse\nfalse\ntrue\n",
    );
}

#[test]
fn paths_and_untrusted_text_run_natively() {
    assert_prints(
        "text_path",
        r#"    let p = Path.new("notes/a.txt")
    let q = Path.new("notes/a.txt")
    let r = Path.new("notes/b.txt")
    yes(io, p == q)
    yes(io, p == r)
    yes(io, p != r)
    show(io, "made")"#,
        "true\nfalse\ntrue\nmade\n",
    );
}

#[test]
fn text_surviving_many_copies_and_frees_stays_intact() {
    assert_prints(
        "text_churn",
        r#"    var i = 0
    var total = 0
    while i < 2000 {
        let word = "é{i}"
        let part = word.repeat(3)
        let wrapped = "[{part}]"
        let text = wrapped.replace("é", "e")
        total = total + text.len() + text.byte_len()
        i = i + 1
    }
    pair(io, total, i)"#,
        "61340 2000\n",
    );
}

#[test]
fn untrusted_text_has_the_same_operations() {
    let src = format!(
        "{HELPERS}\n{}",
        r#"fn inspect(io: Stdout, raw: Untrusted<String>) {
    let n = raw.len()
    let b = raw.byte_len()
    pair(io, n, b)
    let trimmed = raw.trim()
    let safe = trimmed.assume_trusted()
    bracket(io, safe)
    let path = Path.from(raw)
    let same = Path.new(safe)
    yes(io, path == same)
    yes(io, raw.contains("é"))
}

fn main(io: Stdout) {
    let word = "é ünï"
    inspect(io, word)
    let twice = word.repeat(2)
    inspect(io, twice)
}
"#
    );
    let outcome = assert_same(&Case::file("text_untrusted", &src));
    assert_eq!(
        outcome.stdout, "5 8\n[é ünï]\ntrue\ntrue\n10 16\n[é ünïé ünï]\ntrue\ntrue\n",
        "{outcome:#?}"
    );
}
