//! Conformance (#42): the example programs check cleanly, and the table of
//! `E02xx` codes in `docs/checker.md`, the canonical one, matches the codes
//! the checker defines.
//!
//! Design-notes samples are left out on purpose: they are fragments that use
//! types they never declare.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::check;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

// ---- Examples -----------------------------------------------------------------

#[test]
fn every_example_checks_with_zero_diagnostics() {
    let dir = repo_root().join("examples");
    let mut examples: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("examples/ should exist")
        .map(|entry| entry.expect("examples/ entry should be readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "osk"))
        .collect();
    examples.sort();
    assert!(
        examples.len() >= 2,
        "expected the example programs in {}, found {examples:?}",
        dir.display()
    );

    let failures: Vec<String> = examples
        .iter()
        .filter_map(|path| {
            let src = read(path);
            let report = check(&src);
            (!report.diagnostics.is_empty()).then(|| {
                let found: Vec<String> = report
                    .diagnostics
                    .iter()
                    .map(|d| {
                        let text = &src[d.span.start..d.span.end];
                        format!("  {} {:?}: {}", d.code, text, d.message)
                    })
                    .collect();
                format!("{}:\n{}", path.display(), found.join("\n"))
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "examples should check with zero diagnostics:\n{}",
        failures.join("\n")
    );
}

// ---- Documented codes ---------------------------------------------------------

/// The checker codes defined in the `codes` module of `src/lib.rs`.
fn defined_codes() -> BTreeSet<String> {
    let lib = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"));
    let module = lib
        .split("mod codes {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("src/lib.rs should have a `codes` module");
    let codes: BTreeSet<String> = module
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub const "))
        .map(|decl| {
            let value = decl.split('"').nth(1);
            value
                .unwrap_or_else(|| panic!("code constant without a string value: {decl}"))
                .to_string()
        })
        .collect();
    assert!(
        codes.contains("E0200") && codes.contains("E0281"),
        "the `codes` module should be found and parsed, got {codes:?}"
    );
    codes
}

/// A checker code: `E02xx` or above. `E01xx` are parser codes.
fn is_checker_code(code: &str) -> bool {
    let Some(digits) = code.strip_prefix("E0") else {
        return false;
    };
    digits.len() == 3
        && digits.chars().all(|c| c.is_ascii_digit())
        && !digits.starts_with(['0', '1'])
}

/// The rows of a Markdown table that document a checker code, as the code
/// and its cells after the code column.
fn documented_rows<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<(String, Vec<String>)> {
    lines
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            let code = cells.first()?.trim().strip_prefix('`')?.strip_suffix('`')?;
            let rest = cells[1..].iter().map(|c| c.trim().to_string()).collect();
            is_checker_code(code).then(|| (code.to_string(), rest))
        })
        .collect()
}

/// `docs/checker.md`: code, meaning, and fix (`None.` when there is none).
fn checker_md_rows() -> Vec<(String, Vec<String>)> {
    let doc = read(&repo_root().join("docs/checker.md"));
    documented_rows(doc.lines())
}

fn assert_documents_exactly(
    place: &str,
    rows: &[(String, Vec<String>)],
    defined: &BTreeSet<String>,
) {
    let listed: Vec<&String> = rows.iter().map(|(code, _)| code).collect();
    let mut sorted = listed.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        listed, sorted,
        "{place} should list each code once, in order"
    );
    let documented: BTreeSet<String> = listed.into_iter().cloned().collect();
    let undocumented: Vec<&String> = defined.difference(&documented).collect();
    let nonexistent: Vec<&String> = documented.difference(defined).collect();
    assert!(
        undocumented.is_empty() && nonexistent.is_empty(),
        "{place}: codes not documented: {undocumented:?}; documented codes that do not exist: {nonexistent:?}"
    );
}

#[test]
fn checker_md_documents_exactly_the_defined_codes() {
    assert_documents_exactly("docs/checker.md", &checker_md_rows(), &defined_codes());
}
