// SPDX-License-Identifier: Apache-2.0
//! The result formats documented in `eval/README.md` (its "Results"
//! section) name exactly the fields the harness writes: the first table
//! the run record's, the second the batch summary's. The written files are
//! the ones the `record` and `summarise` tests hold to the harness's
//! output, so the documentation cannot drift from it. Likewise its
//! "Running a batch" section documents exactly the batch config's fields,
//! and its example config plans against the real task set.

mod common;

use common::{harness, Scratch};
use oska_syntax::{parse_json, JsonValue};
use std::collections::BTreeSet;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The README's section `## <title>`, up to the next section.
fn section(title: &str) -> String {
    let readme = read("../../eval/README.md");
    let start = readme
        .find(&format!("\n## {title}\n"))
        .unwrap_or_else(|| panic!("a {title} section"));
    let section = &readme[start + 1..];
    section[..section[1..].find("\n## ").map_or(section.len(), |e| e + 1)].to_string()
}

/// The fields named in the first column of each table of the README's
/// "Results" section, one set per table, in order.
fn documented() -> Vec<BTreeSet<String>> {
    tables(&section("Results"))
}

/// The fields named in the first column of each table of `section`, one
/// set per table, in order.
fn tables(section: &str) -> Vec<BTreeSet<String>> {
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

#[test]
fn the_documented_batch_config_fields_are_exactly_those_planned() {
    let tables = tables(&section("Running a batch"));
    assert_eq!(tables.len(), 1, "one table: the batch config");
    let plan = parse_json(&read("tests/golden/plan.json")).expect("the plan is JSON");
    let written: BTreeSet<String> = match plan.get("config") {
        Some(JsonValue::Object(fields)) => fields.iter().map(|(k, _)| k.clone()).collect(),
        other => panic!("the plan's config is not an object: {other:?}"),
    };
    assert_eq!(tables[0], written);
}

/// The README's example batch config is one the maintainer can run: it
/// plans against the repo's task set, within its budget.
#[test]
fn the_documented_example_config_plans_against_the_task_set() {
    let section = section("Running a batch");
    let start = section.find("```json\n").expect("an example config") + "```json\n".len();
    let example = &section[start..start + section[start..].find("```").expect("its end")];
    let scratch = Scratch::new();
    let config = scratch.path().join("2026-11-20-first.json");
    std::fs::write(&config, example).expect("should write the config");
    let outcome = harness(&["plan", "../../eval/tasks", config.to_str().expect("UTF-8")]);
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("\"runs\": 40,"),
        "{}",
        outcome.stdout
    );
}
