//! The result formats documented in `eval/README.md` (its "Results"
//! section) name exactly the fields the harness writes: the first table
//! the run record's, the second the batch summary's. The written files are
//! the ones the `record` and `summarise` tests hold to the harness's
//! output, so the documentation cannot drift from it.

use oska_syntax::{parse_json, JsonValue};
use std::collections::BTreeSet;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The fields named in the first column of each table of the README's
/// "Results" section, one set per table, in order.
fn documented() -> Vec<BTreeSet<String>> {
    let readme = read("../../eval/README.md");
    let start = readme.find("\n## Results\n").expect("a Results section");
    let section = &readme[start + 1..];
    let section = &section[..section[1..].find("\n## ").map_or(section.len(), |e| e + 1)];
    let mut tables: Vec<BTreeSet<String>> = Vec::new();
    let mut in_table = false;
    for line in section.lines() {
        let Some(row) = line.strip_prefix('|') else {
            in_table = false;
            continue;
        };
        if !in_table {
            tables.push(BTreeSet::new());
            in_table = true;
        }
        let first = row.split('|').next().unwrap_or("");
        let names = first.split('`').skip(1).step_by(2).map(str::to_string);
        tables.last_mut().expect("a table").extend(names);
    }
    tables
}

/// The top-level fields of the JSON file at `relative`.
fn written(relative: &str) -> BTreeSet<String> {
    match parse_json(&read(relative)).expect("the file is JSON") {
        JsonValue::Object(fields) => fields.into_iter().map(|(k, _)| k).collect(),
        other => panic!("{relative} is not an object: {other:?}"),
    }
}

#[test]
fn the_documented_run_record_fields_are_exactly_those_written() {
    let tables = documented();
    assert_eq!(
        tables.len(),
        2,
        "two tables: the run record and the summary"
    );
    assert_eq!(tables[0], written("tests/golden/record_escape.json"));
}

#[test]
fn the_documented_summary_fields_are_exactly_those_written() {
    let tables = documented();
    assert_eq!(
        tables[1],
        written("tests/fixtures/results/2026-11-20-hook/summary.json")
    );
}
