//! `oskac explain <code>`: the catalog entry of a diagnostic code, offline.
//!
//! The catalog is `docs/checker.md` and `docs/runtime.md`, compiled into
//! the binary. A code's row in a table gives its meaning and, in the
//! checker tables, its machine-applicable fix; a pair of fenced blocks
//! tagged `error=CODE` and `fixed=CODE` gives a program that fails with
//! the code and the same program fixed. The documentation site is built
//! from the same files, so a code reads the same in both places. The
//! tests in `tests/catalog.rs` keep them in step with the codes the
//! source defines.

use crate::json::{quote, quote_option};
use std::fmt::Write;

/// The docs that hold the catalog, in the order their codes are listed.
const DOCS: [&str; 2] = [
    include_str!("../../../docs/checker.md"),
    include_str!("../../../docs/runtime.md"),
];

/// Where the site has a page for each code.
const SITE: &str = oska_syntax::CODE_PAGES;

/// How far an unknown code may be from a known one to be suggested.
const SUGGESTION_DISTANCE: usize = 2;

/// One code's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub code: String,
    /// What the code means, as Markdown.
    pub message: String,
    /// What the machine-applicable fix does, when the checker offers one.
    pub machine_fix: Option<String>,
    /// How safe that fix is to apply, as a diagnostic's `fix` gives it:
    /// `machine`, `maybe` or `placeholders`.
    pub fix_applicability: Option<String>,
    /// A program that fails with this code, when one can.
    pub example: Option<String>,
    /// The same program fixed.
    pub fixed: Option<String>,
}

/// Every entry of the catalog, in the order the docs list them.
pub fn catalog() -> Vec<Entry> {
    let mut entries = Vec::new();
    for doc in DOCS {
        read_doc(doc, &mut entries);
    }
    entries
}

/// A fenced block whose tag names a code, while it is being read.
struct Block {
    /// `Some((is_error, code))` for an `error=CODE` or `fixed=CODE` block.
    tag: Option<(bool, String)>,
    body: String,
}

fn is_code(word: &str) -> bool {
    word.len() == 5 && word.starts_with(['E', 'P']) && word[1..].chars().all(|c| c.is_ascii_digit())
}

/// The tag in the info string of a fence: `oska error=E0300`.
fn tag_of(info: &str) -> Option<(bool, String)> {
    info.split_whitespace().skip(1).find_map(|word| {
        let (kind, code) = word.split_once('=')?;
        let is_error = match kind {
            "error" => true,
            "fixed" => false,
            _ => return None,
        };
        is_code(code).then(|| (is_error, code.to_string()))
    })
}

/// A table row `` | `E0300` | meaning | fix | applicability | ``, as an
/// entry without examples. The runtime tables have no fix column and no
/// applicability. A `|` inside a cell is written `\|`.
fn row_of(line: &str) -> Option<Entry> {
    let inner = line.strip_prefix("| `")?.strip_suffix(" |")?;
    let (code, cells) = inner.split_once("` | ")?;
    if !is_code(code) {
        return None;
    }
    let mut cells = cells.split(" | ").map(|cell| cell.replace("\\|", "|"));
    let message = cells.next()?;
    let machine_fix = cells.next().filter(|fix| !fix.starts_with("None"));
    let fix_applicability = cells.next().filter(|_| machine_fix.is_some());
    Some(Entry {
        code: code.to_string(),
        message,
        machine_fix,
        fix_applicability,
        example: None,
        fixed: None,
    })
}

fn read_doc(doc: &str, entries: &mut Vec<Entry>) {
    let mut open: Option<Block> = None;
    let mut examples: Vec<(bool, String, String)> = Vec::new();
    let first_of_doc = entries.len();
    for line in doc.lines() {
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(info)) => {
                open = Some(Block {
                    tag: tag_of(info),
                    body: String::new(),
                });
            }
            (Some(_), Some(rest)) if rest.trim().is_empty() => {
                if let Some(Block {
                    tag: Some((is_error, code)),
                    body,
                }) = open.take()
                {
                    examples.push((is_error, code, body));
                }
                open = None;
            }
            (Some(block), _) => {
                block.body.push_str(line);
                block.body.push('\n');
            }
            (None, None) => entries.extend(row_of(line)),
        }
    }
    for (is_error, code, body) in examples {
        if let Some(entry) = entries[first_of_doc..].iter_mut().find(|e| e.code == code) {
            if is_error {
                entry.example = Some(body);
            } else {
                entry.fixed = Some(body);
            }
        }
    }
}

/// The entry for `code`, in any case.
pub fn find<'a>(entries: &'a [Entry], code: &str) -> Option<&'a Entry> {
    let code = code.to_ascii_uppercase();
    entries.iter().find(|entry| entry.code == code)
}

/// The known code closest to `code`, when one is close enough to be what
/// was meant.
pub fn closest<'a>(entries: &'a [Entry], code: &str) -> Option<&'a Entry> {
    let code = code.to_ascii_uppercase();
    // Of the codes at the least distance, the one that starts most like
    // `code` wins, so a typo in the last digit stays in its series.
    entries
        .iter()
        .map(|entry| (distance(&entry.code, &code), entry))
        .filter(|(distance, _)| *distance <= SUGGESTION_DISTANCE)
        .min_by_key(|(distance, entry)| {
            (
                *distance,
                std::cmp::Reverse(common_prefix(&entry.code, &code)),
            )
        })
        .map(|(_, entry)| entry)
}

/// How many characters `a` and `b` start with in common.
fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The edit distance between two texts: the fewest insertions, deletions,
/// and substitutions of a character that turn one into the other.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The entry as `oskac explain` prints it.
pub fn to_human(entry: &Entry) -> String {
    let mut out = format!("{}: {}\n\n", entry.code, entry.message);
    match &entry.machine_fix {
        Some(fix) => {
            let _ = writeln!(out, "{}: {fix}", fix_label(entry));
            if let Some(applicability) = &entry.fix_applicability {
                let _ = writeln!(out, "Fix applicability: {applicability}.");
            }
        }
        None => out.push_str("Machine-applicable fix: none.\n"),
    }
    if let Some(example) = &entry.example {
        let _ = write!(out, "\nExample, which fails with {}:\n\n", entry.code);
        out.push_str(&indented(example));
    }
    if let Some(fixed) = &entry.fixed {
        out.push_str("\nFixed:\n\n");
        out.push_str(&indented(fixed));
    }
    let _ = write!(out, "\n{SITE}/{}\n", entry.code);
    out
}

/// The label of an entry's fix, as the site's code pages print it. Only a
/// `machine` fix is certainly right (the glossary's Fix applicability); a
/// `maybe` or `placeholders` fix is not machine-applicable.
fn fix_label(entry: &Entry) -> &'static str {
    match entry.fix_applicability.as_deref() {
        Some("maybe" | "placeholders") => "Fix",
        _ => "Machine-applicable fix",
    }
}

/// `text` with four spaces before each line that has any text.
fn indented(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if !line.is_empty() {
            out.push_str("    ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// The entry as one line of JSON, the same text the site's `codes.json`
/// holds for the code.
pub fn to_json(entry: &Entry) -> String {
    format!(
        "{{\"code\":{},\"message\":{},\"machine_fix\":{},\"machine_fix_description\":{},\"fix_applicability\":{},\"example\":{},\"fix\":{}}}",
        quote(&entry.code),
        quote(&entry.message),
        entry.machine_fix.is_some(),
        quote_option(entry.machine_fix.as_deref()),
        quote_option(entry.fix_applicability.as_deref()),
        quote_option(entry.example.as_deref()),
        quote_option(entry.fixed.as_deref()),
    )
}

/// An unknown code as one line of JSON.
pub fn unknown_to_json(code: &str, suggestion: Option<&Entry>) -> String {
    format!(
        "{{\"error\":\"unknown code\",\"code\":{},\"suggestion\":{}}}",
        quote(code),
        quote_option(suggestion.map(|e| e.code.as_str()))
    )
}

/// An unknown code as the line `oskac explain` prints on stderr.
pub fn unknown_to_human(code: &str, suggestion: Option<&Entry>) -> String {
    match suggestion {
        Some(entry) => format!(
            "error: unknown diagnostic code `{code}`; did you mean `{}`?",
            entry.code
        ),
        None => {
            format!("error: unknown diagnostic code `{code}`; a code looks like E0300 or P0004")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog, closest, distance, find, row_of, to_human};

    #[test]
    fn the_distance_counts_single_character_edits() {
        assert_eq!(distance("E0300", "E0300"), 0);
        assert_eq!(distance("E0300", "E0301"), 1);
        assert_eq!(distance("E0300", "E300"), 1);
        assert_eq!(distance("E0300", "300"), 2);
        assert_eq!(distance("", "abc"), 3);
    }

    #[test]
    fn a_row_has_a_message_and_a_fix_unless_the_fix_is_none() {
        let with = row_of("| `E0200` | Unknown name. | Replace it. |").expect("a row");
        assert_eq!(with.message, "Unknown name.");
        assert_eq!(with.machine_fix.as_deref(), Some("Replace it."));
        let without = row_of("| `E0201` | A trait. | None. |").expect("a row");
        assert_eq!(without.machine_fix, None);
        let runtime = row_of("| `P0004` | Division by zero. |").expect("a row");
        assert_eq!(runtime.machine_fix, None);
        assert_eq!(row_of("| Code | Meaning |"), None);
    }

    #[test]
    fn only_a_machine_fix_is_labelled_machine_applicable() {
        let fix_line = |row: &str| {
            let entry = row_of(row).expect("a row");
            to_human(&entry)
                .lines()
                .nth(2)
                .expect("a fix line")
                .to_string()
        };
        assert_eq!(
            fix_line("| `E0200` | Unknown name. | Replace it. | machine |"),
            "Machine-applicable fix: Replace it."
        );
        assert_eq!(
            fix_line("| `E0200` | Unknown name. | Replace it. | maybe |"),
            "Fix: Replace it."
        );
        assert_eq!(
            fix_line("| `E0200` | Unknown name. | Fill it. | placeholders |"),
            "Fix: Fill it."
        );
        assert_eq!(
            fix_line("| `E0201` | A trait. | None. | — |"),
            "Machine-applicable fix: none."
        );
    }

    #[test]
    fn an_escaped_pipe_in_a_cell_is_a_pipe() {
        let row = row_of("| `E0140` | An or-pattern such as `A \\| B`. | None. |").expect("a row");
        assert_eq!(row.message, "An or-pattern such as `A | B`.");
    }

    #[test]
    fn the_catalog_finds_codes_in_any_case_and_suggests_the_nearest() {
        let entries = catalog();
        assert!(find(&entries, "e0300").is_some());
        assert!(find(&entries, "E0301").is_none());
        let near = closest(&entries, "E0301").map(|e| e.code.as_str());
        assert_eq!(near, Some("E0300"));
        assert!(closest(&entries, "frobnicate").is_none());
    }
}
