// SPDX-License-Identifier: Apache-2.0
//! The core library's `String` methods (spec #660, ticket #669, ADR 0034):
//! `to_uppercase`, `trim_start`, `trim_end`, `find`, `split_once`,
//! `strip_prefix`, `strip_suffix` and `is_empty`. Each runs on empty,
//! ASCII and non-ASCII text, and every program also runs natively on
//! Linux (`common/mod.rs`), so both backends give the same result. The
//! trust tests at the end fail if a method loses `#[follows_trust]`.

mod common;

use common::{assert_refused, assert_runs};

fn main_with(body: &str) -> String {
    format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n\
         fn text(o: Option<String>) -> String {{\n    \
         match o {{\n        Some(s) => \"[{{s}}]\"\n        None => \"none\"\n    }}\n}}\n\
         fn pos(o: Option<Int>) -> String {{\n    \
         match o {{\n        Some(n) => \"at {{n}}\"\n        None => \"none\"\n    }}\n}}\n\
         fn yn(b: Bool) -> String {{\n    if b {{ \"true\" }} else {{ \"false\" }}\n}}\n\
         fn both(p: List<String>) -> String {{\n    let a = p.at(0)\n    let b = p.at(1)\n    \"[{{a}}|{{b}}]\"\n}}\n\
         fn pair(o: Option<List<String>>) -> String {{\n    \
         match o {{\n        Some(p) => both(p)\n        None => \"none\"\n    }}\n}}\n"
    )
}

#[test]
fn to_uppercase_maps_each_character_on_its_own() {
    let src = main_with(
        "    let empty = \"\"\n    \
         io.println(\"abc xyz 123 @\".to_uppercase())?\n    \
         io.println(\"é ñ ж ǆ ÿ\".to_uppercase())?\n    \
         io.println(\"𐐨 日本 😀\".to_uppercase())?\n    \
         io.println(\"ß ŉ\".to_uppercase())?\n    \
         io.println(empty.to_uppercase())",
    );
    assert_runs(&src, &[], "ABC XYZ 123 @\nÉ Ñ Ж Ǆ Ÿ\n𐐀 日本 😀\nß ŉ\n\n");
}

#[test]
fn trim_start_and_trim_end_cut_one_side() {
    let src = main_with(
        "    let s = \"  \\t é ü \\n\"\n    \
         let wide = \"\u{3000}x\u{3000}\"\n    \
         let blank = \"   \"\n    \
         let empty = \"\"\n    \
         let a = s.trim_start()\n    \
         let b = s.trim_end()\n    \
         let c = wide.trim_start()\n    \
         let d = wide.trim_end()\n    \
         let e = blank.trim_start()\n    \
         let f = blank.trim_end()\n    \
         let g = empty.trim_start()\n    \
         let h = empty.trim_end()\n    \
         let i = \"x\".trim_start()\n    \
         io.println(\"[{a}]\")?\n    \
         io.println(\"[{b}]\")?\n    \
         io.println(\"[{c}]\")?\n    \
         io.println(\"[{d}]\")?\n    \
         io.println(\"[{e}][{f}][{g}][{h}][{i}]\")",
    );
    assert_runs(
        &src,
        &[],
        "[é ü \n]\n[  \t é ü]\n[x\u{3000}]\n[\u{3000}x]\n[][][][][x]\n",
    );
}

#[test]
fn find_gives_the_character_position_of_the_first_match() {
    let src = main_with(
        "    io.println(pos(\"hello\".find(\"l\")))?\n    \
         io.println(pos(\"héllo wörld\".find(\"wö\")))?\n    \
         io.println(pos(\"日本語\".find(\"語\")))?\n    \
         io.println(pos(\"abcabc\".find(\"cab\")))?\n    \
         io.println(pos(\"abc\".find(\"abc\")))?\n    \
         io.println(pos(\"abc\".find(\"abcd\")))?\n    \
         io.println(pos(\"abc\".find(\"z\")))?\n    \
         io.println(pos(\"\".find(\"a\")))",
    );
    assert_runs(
        &src,
        &[],
        "at 2\nat 6\nat 2\nat 2\nat 0\nnone\nnone\nnone\n",
    );
}

#[test]
fn find_of_an_empty_needle_is_the_start() {
    let src = main_with(
        "    io.println(pos(\"abc\".find(\"\")))?\n    \
         io.println(pos(\"\".find(\"\")))?\n    \
         io.println(pos(\"é\".find(\"\")))",
    );
    assert_runs(&src, &[], "at 0\nat 0\nat 0\n");
}

#[test]
fn find_and_slice_agree_on_non_ascii_text() {
    let src = main_with(
        "    let s = \"añb→c\"\n    \
         let at = s.find(\"→\").unwrap_or(-1)\n    \
         io.println(s.slice(at, s.len()))",
    );
    assert_runs(&src, &[], "→c\n");
}

#[test]
fn split_once_splits_at_the_first_separator() {
    let src = main_with(
        "    io.println(pair(\"key=value=x\".split_once(\"=\")))?\n    \
         io.println(pair(\"novalue\".split_once(\"=\")))?\n    \
         io.println(pair(\"=a\".split_once(\"=\")))?\n    \
         io.println(pair(\"a=\".split_once(\"=\")))?\n    \
         io.println(pair(\"=\".split_once(\"=\")))?\n    \
         io.println(pair(\"añ→b→c\".split_once(\"→\")))?\n    \
         io.println(pair(\"abc\".split_once(\"\")))?\n    \
         io.println(pair(\"\".split_once(\"\")))?\n    \
         io.println(pair(\"\".split_once(\"x\")))",
    );
    assert_runs(
        &src,
        &[],
        "[key|value=x]\nnone\n[|a]\n[a|]\n[|]\n[añ|b→c]\n[|abc]\n[|]\nnone\n",
    );
}

#[test]
fn strip_prefix_and_strip_suffix_remove_one_end_or_give_none() {
    let src = main_with(
        "    io.println(text(\"foobar\".strip_prefix(\"foo\")))?\n    \
         io.println(text(\"foobar\".strip_prefix(\"bar\")))?\n    \
         io.println(text(\"foo\".strip_prefix(\"foo\")))?\n    \
         io.println(text(\"foo\".strip_prefix(\"\")))?\n    \
         io.println(text(\"foo\".strip_prefix(\"foobar\")))?\n    \
         io.println(text(\"ñandú\".strip_prefix(\"ñ\")))?\n    \
         io.println(text(\"\".strip_prefix(\"\")))?\n    \
         io.println(text(\"foobar\".strip_suffix(\"bar\")))?\n    \
         io.println(text(\"foobar\".strip_suffix(\"foo\")))?\n    \
         io.println(text(\"bar\".strip_suffix(\"bar\")))?\n    \
         io.println(text(\"bar\".strip_suffix(\"\")))?\n    \
         io.println(text(\"bar\".strip_suffix(\"foobar\")))?\n    \
         io.println(text(\"ñandú\".strip_suffix(\"ú\")))?\n    \
         io.println(text(\"\".strip_suffix(\"\")))",
    );
    assert_runs(
        &src,
        &[],
        "[bar]\nnone\n[]\n[foo]\nnone\n[andú]\n[]\n[foo]\nnone\n[]\n[bar]\nnone\n[ñand]\n[]\n",
    );
}

#[test]
fn is_empty_is_true_only_for_no_characters() {
    let src = main_with(
        "    let a = yn(\"\".is_empty())\n    \
         let b = yn(\" \".is_empty())\n    \
         let c = yn(\"é\".is_empty())\n    \
         io.println(\"{a} {b} {c}\")",
    );
    assert_runs(&src, &[], "true false false\n");
}

// ---- Trust (#223, ADR 0006): each test fails if `#[follows_trust]` goes ----

/// A program whose function takes untrusted text and returns `ret` built
/// from `expr`.
fn from_untrusted(ret: &str, expr: &str) -> String {
    format!(
        "fn f(raw: Untrusted<String>) -> {ret} {{\n    {expr}\n}}\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    Ok(())\n}}\n"
    )
}

/// The method accepts untrusted text, and its result is untrusted: a
/// trusted result type refuses it (`E0300`), an untrusted one accepts it.
fn assert_text_result_is_untrusted(ret_trusted: &str, ret_untrusted: &str, expr: &str) {
    assert_runs(&from_untrusted(ret_untrusted, expr), &[], "");
    assert_refused(&from_untrusted(ret_trusted, expr), "E0300");
}

#[test]
fn to_uppercase_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted("String", "Untrusted<String>", "raw.to_uppercase()");
}

#[test]
fn trim_start_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted("String", "Untrusted<String>", "raw.trim_start()");
}

#[test]
fn trim_end_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted("String", "Untrusted<String>", "raw.trim_end()");
}

#[test]
fn strip_prefix_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted(
        "Option<String>",
        "Option<Untrusted<String>>",
        "raw.strip_prefix(\"a\")",
    );
}

#[test]
fn strip_suffix_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted(
        "Option<String>",
        "Option<Untrusted<String>>",
        "raw.strip_suffix(\"a\")",
    );
}

#[test]
fn split_once_of_untrusted_text_is_untrusted() {
    assert_text_result_is_untrusted(
        "Option<List<String>>",
        "Option<List<Untrusted<String>>>",
        "raw.split_once(\"a\")",
    );
}

#[test]
fn an_untrusted_separator_or_affix_taints_the_result() {
    let src = "fn f(raw: Untrusted<String>) -> Option<List<String>> {\n    \
               \"abc\".split_once(raw)\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    Ok(())\n}\n";
    assert_refused(src, "E0300");
    let src = "fn f(raw: Untrusted<String>) -> Option<String> {\n    \
               \"abc\".strip_prefix(raw)\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    Ok(())\n}\n";
    assert_refused(src, "E0300");
}

#[test]
fn find_of_untrusted_text_is_a_trusted_position() {
    // It takes untrusted text (the flag is on the method) and gives a
    // trusted `Option<Int>`, as `len` does.
    assert_runs(&from_untrusted("Option<Int>", "raw.find(\"a\")"), &[], "");
    assert_runs(&from_untrusted("Option<Int>", "\"abc\".find(raw)"), &[], "");
}

#[test]
fn is_empty_of_untrusted_text_is_a_trusted_bool() {
    assert_runs(&from_untrusted("Bool", "raw.is_empty()"), &[], "");
}

#[test]
fn the_methods_of_untrusted_text_run_with_the_same_result() {
    let src = "fn main(io: Stdout, args: Args) -> Result<(), IoError> {\n    \
               let raw = args.get(1).unwrap_or(\"\")\n    \
               let up = raw.to_uppercase()\n    \
               io.println(up.assume_trusted())\n}\n";
    assert_runs(src, &["é x"], "É X\n");
}

/// The language tour's example for these methods runs, on both backends.
#[test]
fn the_language_tour_example_runs() {
    let tour = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/guide/language-tour.md"
    ))
    .expect("the tour");
    let block = tour
        .split("```oska\n")
        .skip(1)
        .map(|rest| rest.split("```").next().unwrap_or(""))
        .find(|code| code.contains("split_once"))
        .expect("the example");
    assert_runs(block, &[], "name 4 ZOË\n");
}
