// SPDX-License-Identifier: Apache-2.0
//! Fewer short copies of text (#572): `String.replace` with a
//! replacement of the same length copies the text once and overwrites
//! each match, `trim`, `slice` and a `replace` that finds nothing share
//! the text they were given when it is unchanged, `Html.raw` and
//! `TrustedHtml.new` share their markup, an `Int` or `String` hole is
//! written straight into the interpolated text, and a struct or variant
//! shares its type's name. A program sees the same text as before, and
//! under `--max-memory` each text is still charged as a text of its own.

mod common;

use common::{assert_runs, run_under_limit};
use oska_run::{codes, RunOutcome};

/// The `--max-memory` of the metered tests: 1 MiB.
const ONE_MIB: u64 = 1 << 20;

fn run_metered(src: &str) -> RunOutcome {
    run_under_limit(src, ONE_MIB)
}

/// A program whose `main` prints each line of `body`'s `lines`.
fn printing(body: &str) -> String {
    format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n    \
         let lines: List<String> = [{body}]\n    \
         for line in lines {{\n        \
         io.println(\"<{{line}}>\")?\n    \
         }}\n    \
         Ok(())\n}}\n"
    )
}

#[test]
fn join_of_characters_and_longer_parts_with_every_kind_of_separator() {
    let src = printing(
        "\"\".join(\"a é🎉z\".chars()), \
         \"-\".join(\"日本語\".chars()), \
         \"é\".join([\"x\", \"\", \"yz\", \"🎉\"]), \
         \", \".join([\"one\", \"t\", \"\", \"three\"]), \
         \"--\".join([\"a\"]), \
         \"/\".join([]), \
         \"\".join([\"\", \"\"]), \
         \"\".join([\"ab\", \"c\", \"de\"])",
    );
    assert_runs(
        &src,
        &[],
        "<a é🎉z>\n<日-本-語>\n<xééyzé🎉>\n<one, t, , three>\n<a>\n<>\n<>\n<abcde>\n",
    );
}

#[test]
fn replace_with_a_replacement_of_the_same_length_replaces_each_match_once() {
    let src = printing(
        "\"aaaaa\".replace(\"aa\", \"bb\"), \
         \"fox and fox, foxes\".replace(\"fox\", \"cat\"), \
         \"café crème\".replace(\"é\", \"è\"), \
         \"xyz\".replace(\"xyz\", \"abc\"), \
         \"日本日本\".replace(\"本\", \"月\"), \
         \"abab\".replace(\"b\", \"🎉\"), \
         \"nothing here\".replace(\"zz\", \"yy\"), \
         \"keep\".replace(\"\", \"x\")",
    );
    assert_runs(
        &src,
        &[],
        "<bbbba>\n<cat and cat, cates>\n<cafè crème>\n<abc>\n<日月日月>\n<a🎉a🎉>\n<nothing here>\n<keep>\n",
    );
}

#[test]
fn trim_slice_and_replace_that_change_nothing_give_an_equal_text() {
    let src = "fn yes(b: Bool) -> String {\n    \
               if b {\n        \
               return \"yes\"\n    \
               }\n    \
               \"no\"\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let text = \"no spaces\"\n    \
               let trimmed = text.trim()\n    \
               let sliced = text.slice(0, text.len())\n    \
               let replaced = text.replace(\"zz\", \"y\")\n    \
               let part = \"  padded \".trim()\n    \
               let inner = text.slice(3, 9)\n    \
               let same = yes(trimmed == text && sliced == text && replaced == text)\n    \
               io.println(\"{same} <{trimmed}> <{sliced}> <{replaced}> <{part}> <{inner}>\")\n}\n";
    assert_runs(
        src,
        &[],
        "yes <no spaces> <no spaces> <no spaces> <padded> <spaces>\n",
    );
}

#[test]
fn interpolated_int_and_string_holes_show_as_before() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let low = -9223372036854775807 - 1\n    \
               let high = 9223372036854775807\n    \
               let zero = 0\n    \
               let negative = -42\n    \
               let word = \"日本\"\n    \
               let empty = \"\"\n    \
               io.println(\"[{low}|{high}|{zero}|{negative}|{word}|{empty}|{negative}{word}]\")\n}\n";
    assert_runs(
        src,
        &[],
        "[-9223372036854775808|9223372036854775807|0|-42|日本||-42日本]\n",
    );
}

#[test]
fn raw_trusted_markup_renders_as_written() {
    let src = "use std.html\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let open = html.Html.raw(html.TrustedHtml.new(\"<p class=\\\"x\\\">\"))\n    \
               let page = open.concat(html.text(\"a < b\")).concat(html.Html.raw(html.TrustedHtml.new(\"</p>\")))\n    \
               io.println(page.render())?\n    \
               io.println(open.render())\n}\n";
    assert_runs(src, &[], "<p class=\"x\">a &lt; b</p>\n<p class=\"x\">\n");
}

#[test]
fn values_of_the_same_type_made_in_many_places_are_equal_and_match() {
    let src = "enum Shape {\n    \
               Dot,\n    \
               Line(Int),\n}\n\
               struct Point {\n    \
               x: Int,\n}\n\
               fn line(n: Int) -> Shape {\n    \
               Shape.Line(n)\n}\n\
               fn name(shape: Shape) -> String {\n    \
               match shape {\n        \
               Shape.Dot => \"dot\"\n        \
               Shape.Line(n) => \"line {n}\"\n    \
               }\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let a = Point(x: 1)\n    \
               let b = Point(x: 1)\n    \
               let sum = a.x + b.x\n    \
               let same = if Some(3) == Some(3) && Ok(sum) == Ok(2) { \"equal\" } else { \"differ\" }\n    \
               let differ = if Some(3) == None { \"equal\" } else { \"differ\" }\n    \
               let five = name(line(5))\n    \
               let dot = name(Shape.Dot)\n    \
               io.println(\"{same} {differ} {five} {dot}\")\n}\n";
    assert_runs(src, &[], "equal differ line 5 dot\n");
}

/// Keeps 14 lists of 1,000 texts, each `make` applied to a 100-byte text
/// that it leaves unchanged. Charged as a text of its own, each is about
/// 160 bytes, 2.2 MB in all, past 1 MiB; if only the lists were
/// charged, about 0.68 MB.
fn keeps_unchanged(make: &str) -> String {
    format!(
        "fn main(io: Stdout) {{\n    \
         let text = \"a\".repeat(100)\n    \
         var kept: List<List<String>> = []\n    \
         var i = 0\n    \
         while i < 14 {{\n        \
         var row: List<String> = []\n        \
         var j = 0\n        \
         while j < 1000 {{\n            \
         row.push({make})\n            \
         j = j + 1\n        \
         }}\n        \
         kept.push(row)\n        \
         i = i + 1\n    \
         }}\n    \
         let n = kept.len()\n    \
         let _ = io.println(\"kept {{n}}\")\n}}\n"
    )
}

#[test]
fn under_max_memory_an_unchanged_trim_slice_or_replace_is_charged_as_a_text() {
    for make in [
        "text.trim()",
        "text.slice(0, 100)",
        "text.replace(\"b\", \"c\")",
        "text.replace(\"a\", \"b\")",
        "\"\".join(text.chars())",
    ] {
        let outcome = run_metered(&keeps_unchanged(make));
        let Some(panic) = &outcome.panic else {
            panic!("14,000 charged texts from `{make}` pass 1 MiB: {outcome:#?}");
        };
        assert_eq!(panic.code, codes::MEMORY_LIMIT, "{make}: {panic:#?}");
        assert_eq!(outcome.stdout, "", "{make}");
    }
}

#[test]
fn under_max_memory_a_few_unchanged_texts_fit() {
    let src = keeps_unchanged("text.trim()").replace("while i < 14", "while i < 2");
    let outcome = run_metered(&src);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "kept 2\n");
}

/// Keeps `rows` lists of `per_row` values of type `row_ty`, each made by
/// `make` after `setup`, under `use std.html`.
fn keeps(rows: u32, per_row: u32, row_ty: &str, setup: &str, make: &str) -> String {
    format!(
        "use std.html\n\
         fn main(io: Stdout) {{\n    \
         {setup}\n    \
         var kept: List<List<{row_ty}>> = []\n    \
         var i = 0\n    \
         while i < {rows} {{\n        \
         var row: List<{row_ty}> = []\n        \
         var j = 0\n        \
         while j < {per_row} {{\n            \
         row.push({make})\n            \
         j = j + 1\n        \
         }}\n        \
         kept.push(row)\n        \
         i = i + 1\n    \
         }}\n    \
         let n = kept.len()\n    \
         let _ = io.println(\"kept {{n}}\")\n}}\n"
    )
}

fn assert_over_the_limit(src: &str, what: &str) {
    let outcome = run_metered(src);
    let Some(panic) = &outcome.panic else {
        panic!("{what} passes 1 MiB when each text is charged: {outcome:#?}");
    };
    assert_eq!(panic.code, codes::MEMORY_LIMIT, "{what}: {panic:#?}");
    assert_eq!(outcome.stdout, "", "{what}");
}

/// 1,000 markup values over one 2,000-byte text: about 2 MB when each
/// value's markup is charged as a text of its own, past 1 MiB; about
/// 0.2 MB if only the values holding it were charged.
#[test]
fn under_max_memory_raw_and_trusted_markup_is_charged_as_a_text() {
    let setup = "let text = \"a\".repeat(2000)\n    \
                 let trusted = html.TrustedHtml.new(text)";
    assert_over_the_limit(
        &keeps(
            1,
            1000,
            "html.TrustedHtml",
            setup,
            "html.TrustedHtml.new(text)",
        ),
        "TrustedHtml.new",
    );
    assert_over_the_limit(
        &keeps(1, 1000, "html.Html", setup, "html.Html.raw(trusted)"),
        "Html.raw",
    );
    let fits = keeps(1, 100, "html.Html", setup, "html.Html.raw(trusted)");
    let outcome = run_metered(&fits);
    assert!(outcome.panic.is_none(), "100 fit: {outcome:#?}");
    assert_eq!(outcome.stdout, "kept 1\n");
}

/// Interpolations of a `String` hole, 1,000 of a 2,000-byte text, and of
/// an `Int` hole, 14,000 of a 19-digit integer: each past 1 MiB when
/// every interpolated text is charged as a text of its own, and under it
/// if only the lists were.
#[test]
fn under_max_memory_interpolated_holes_are_charged_as_a_text() {
    assert_over_the_limit(
        &keeps(
            1,
            1000,
            "String",
            "let text = \"a\".repeat(2000)",
            "\"{text}\"",
        ),
        "a String hole",
    );
    assert_over_the_limit(
        &keeps(
            14,
            1000,
            "String",
            "let big = 1000000000000000000",
            "\"{big}\"",
        ),
        "an Int hole",
    );
    let fits = keeps(
        2,
        1000,
        "String",
        "let big = 1000000000000000000",
        "\"{big}\"",
    );
    let outcome = run_metered(&fits);
    assert!(outcome.panic.is_none(), "2,000 fit: {outcome:#?}");
    assert_eq!(outcome.stdout, "kept 2\n");
}
