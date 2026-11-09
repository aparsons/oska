//! Conformance: every Oska sample in the design notes, and every example
//! program, parses as a module. The design notes are embedded at compile time
//! and the examples directory is scanned at run time, so editing either side
//! re-runs this check against the new text.

use oska_syntax::parse_module;

const DESIGN: &str = include_str!("../../../docs/design.md");

/// Every ```` ```oska ```` block in `doc`, with the line its body starts on.
fn oska_blocks(doc: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (index, line) in doc.lines().enumerate() {
        match &mut current {
            None if line.trim_start().starts_with("```oska") => {
                current = Some((index + 2, String::new()));
            }
            Some(_) if line.trim_start().starts_with("```") => {
                blocks.extend(current.take());
            }
            Some((_, body)) => {
                body.push_str(line);
                body.push('\n');
            }
            None => {}
        }
    }
    assert!(current.is_none(), "an ```oska block is never closed");
    blocks
}

#[test]
fn every_design_sample_parses() {
    let blocks = oska_blocks(DESIGN);
    // Syntax, Capabilities, Root of authority, and Contracts at least.
    assert!(blocks.len() >= 4, "found only {} samples", blocks.len());
    let failures: Vec<String> = blocks
        .iter()
        .filter_map(|(line, src)| {
            parse_module(src)
                .err()
                .map(|err| format!("docs/design.md sample at line {line}: {err}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_example_parses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("examples/ should be readable") {
        let path = entry.expect("examples/ entry should be readable").path();
        if path.extension().is_some_and(|ext| ext == "osk") {
            let src = std::fs::read_to_string(&path).expect("example should be readable");
            if let Err(err) = parse_module(&src) {
                panic!("{}: {err}", path.display());
            }
            names.extend(path.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
    }
    for expected in ["capabilities.osk", "hello.osk"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing examples/{expected}"
        );
    }
}
