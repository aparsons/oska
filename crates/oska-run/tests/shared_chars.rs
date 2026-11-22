//! `String.chars` shares its one-character texts (#501): the interpreter
//! makes each ASCII character's text once and hands out that text, not a
//! new copy for every character. A program sees the same characters as
//! before, and under `--max-memory` each character's text is still
//! charged, and freed, as a text of its own.

mod common;

use common::{assert_runs, run_under_limit};
use oska_run::{codes, RunOutcome};

/// The `--max-memory` of the metered tests: 1 MiB.
const ONE_MIB: u64 = 1 << 20;

fn run_metered(src: &str) -> RunOutcome {
    run_under_limit(src, ONE_MIB)
}

#[test]
fn chars_gives_each_character_of_ascii_non_ascii_and_mixed_text() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               for text in [\"abc\", \"日本\", \"a é~\\n🎉z\", \"\"] {\n        \
               var out = \"\"\n        \
               for c in text.chars() {\n            \
               out = \"{out}[{c}]\"\n        \
               }\n        \
               io.println(out)?\n    \
               }\n    \
               Ok(())\n}\n";
    assert_runs(src, &[], "[a][b][c]\n[日][本]\n[a][ ][é][~][\n][🎉][z]\n\n");
}

#[test]
fn the_same_character_taken_twice_is_equal_to_itself_and_to_a_literal() {
    let src = "fn yes(b: Bool) -> String {\n    \
               if b {\n        \
               return \"yes\"\n    \
               }\n    \
               \"no\"\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let first = \"aXa\".chars()\n    \
               let second = \"a\".chars()\n    \
               let same = yes(first.at(0) == first.at(2))\n    \
               let across = yes(first.at(0) == second.at(0))\n    \
               let literal = yes(first.at(1) == \"X\")\n    \
               let other = yes(first.at(0) == first.at(1))\n    \
               io.println(\"{same} {across} {literal} {other}\")\n}\n";
    assert_runs(src, &[], "yes yes yes no\n");
}

/// Keeps 14 lists of the characters of a 1,000-character ASCII text.
/// Each list holds 1,000 texts: about 113 KB a list when each character
/// is charged as a text of its own, 1.58 MB in all, past 1 MiB; about
/// 48 KB a list, 0.68 MB in all, if only the lists were charged.
const KEEPS_CHARS: &str = "fn main(io: Stdout) {\n    \
                           let text = \"a\".repeat(1000)\n    \
                           var kept: List<List<String>> = []\n    \
                           var i = 0\n    \
                           while i < 14 {\n        \
                           kept.push(text.chars())\n        \
                           i = i + 1\n    \
                           }\n    \
                           let n = kept.len()\n    \
                           let _ = io.println(\"kept {n}\")\n}\n";

#[test]
fn under_max_memory_each_character_chars_makes_is_charged_as_a_text() {
    let outcome = run_metered(KEEPS_CHARS);
    let Some(panic) = &outcome.panic else {
        panic!("14 lists of 1,000 charged texts pass 1 MiB: {outcome:#?}");
    };
    assert_eq!(panic.code, codes::MEMORY_LIMIT, "{panic:#?}");
    assert_eq!(outcome.stdout, "");
}

#[test]
fn under_max_memory_the_characters_chars_made_are_freed_once_dropped() {
    // 500 lists of 1,000 characters, about 56 MB if none were freed,
    // one list held at a time.
    let src = "fn main(io: Stdout) {\n    \
               let text = \"a\".repeat(1000)\n    \
               var total = 0\n    \
               var i = 0\n    \
               while i < 500 {\n        \
               let chars = text.chars()\n        \
               total = total + chars.len()\n        \
               i = i + 1\n    \
               }\n    \
               let _ = io.println(\"total {total}\")\n}\n";
    let outcome = run_metered(src);
    assert!(outcome.panic.is_none(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "total 500000\n");
}
