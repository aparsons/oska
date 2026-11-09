//! Conformance (#42): the example programs check cleanly, and the documented
//! list of `E02xx` codes matches the codes the checker defines.
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

/// The `E02xx` codes defined in the `codes` module of `src/lib.rs`.
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

/// The rows of a Markdown table that document an `E02xx` code, as the code
/// and its cells after the code column.
fn documented_rows<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<(String, Vec<String>)> {
    lines
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            let code = cells
                .first()?
                .trim()
                .strip_prefix("`E02")?
                .strip_suffix('`')?;
            let rest = cells[1..].iter().map(|c| c.trim().to_string()).collect();
            Some((format!("E02{code}"), rest))
        })
        .collect()
}

/// `docs/checker.md`: code, meaning, and fix (`None.` when there is none).
fn checker_md_rows() -> Vec<(String, Vec<String>)> {
    let doc = read(&repo_root().join("docs/checker.md"));
    documented_rows(doc.lines())
}

/// The crate docs in `src/lib.rs`: code and meaning, which describes the fix.
fn lib_doc_rows() -> Vec<(String, Vec<String>)> {
    let lib = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"));
    documented_rows(lib.lines().filter_map(|line| line.strip_prefix("//!")))
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

#[test]
fn lib_docs_document_exactly_the_defined_codes() {
    assert_documents_exactly(
        "the crate docs in src/lib.rs",
        &lib_doc_rows(),
        &defined_codes(),
    );
}

#[test]
fn lib_docs_mention_a_fix_exactly_where_checker_md_has_one() {
    let lib: Vec<(String, bool)> = lib_doc_rows()
        .into_iter()
        .map(|(code, cells)| (code, cells.join(" ").contains("fix")))
        .collect();
    let md: Vec<(String, bool)> = checker_md_rows()
        .into_iter()
        .map(|(code, cells)| (code, cells.last().is_some_and(|fix| fix != "None.")))
        .collect();
    let disagree: Vec<String> = md
        .iter()
        .zip(&lib)
        .filter(|(m, l)| m != l)
        .map(|((code, has_fix), _)| {
            let side = if *has_fix {
                "only checker.md"
            } else {
                "only the crate docs"
            };
            format!("{code} ({side} describes a fix)")
        })
        .collect();
    assert!(
        disagree.is_empty(),
        "fix descriptions disagree: {disagree:?}"
    );
}
