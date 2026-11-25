// SPDX-License-Identifier: Apache-2.0
//! `std.map` through the CLI (#601): a word counter prints its words in the
//! order they first appear, byte for byte the same on the interpreter and
//! on a native build. The expected output is the golden file
//! `golden/map_words.txt`.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

/// Counts the words of stdin, ignoring case, and prints each word with its
/// count in the order the word first appeared.
const WORDS: &str = r#"use std.map

fn main(io: Stdout, stdin: Stdin) -> Result<(), IoError> {
    var counts: map.Map<Untrusted<String>, Int> = map.Map.new()
    while true {
        let line = match stdin.read_line()? {
            Some(line) => line
            None => {
                break
            }
        }
        for word in line.to_lowercase().split(" ") {
            if word == "" {
                continue
            }
            let seen = counts.get(word)
            let next = match seen {
                Some(n) => n + 1
                None => 1
            }
            counts.put(word, next)
        }
    }
    for entry in counts.entries() {
        let n = entry.value
        io.println("{entry.key} {n}")?
    }
    Ok(())
}
"#;

const INPUT: &str = "the quick brown fox\n\
jumps over the lazy dog\n\
The dog barks and the fox runs\n\
  quick  quick  \n\
over and over\n";

#[test]
fn a_word_count_prints_in_first_seen_order_on_both_backends() {
    let outcome = assert_same(&Case::file("map_words", WORDS).with_stdin(INPUT));
    assert_eq!(outcome.stdout, include_str!("golden/map_words.txt"));
}
