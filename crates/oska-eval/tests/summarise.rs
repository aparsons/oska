//! `oska-eval summarise`: a batch's run records aggregated into its
//! summary, with func@k and func-sec@k by the unbiased estimator. The
//! batches are hand-written run records under `tests/fixtures/results/`,
//! whose pass rates are worked out by hand below; each summary is compared
//! with the batch's committed `summary.json`, which the `report` tests
//! read. Refresh them with `UPDATE_GOLDEN=1 cargo test -p oska-eval --test
//! summarise` and review the diff. No test calls a model.

mod common;

use common::{assert_file, copy_tree, harness, Scratch};
use oska_syntax::{parse_json, JsonValue};

fn summarise(batch: &str) -> String {
    let dir = format!("tests/fixtures/results/{batch}");
    let outcome = harness(&["summarise", &dir]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_file(&format!("{dir}/summary.json"), &outcome.stdout);
    outcome.stdout
}

/// The pass rates of the row of `pass_rates` with this `by` and `name`.
fn rates(summary: &JsonValue, by: &str, name: &str) -> Vec<String> {
    let row = summary
        .get("pass_rates")
        .and_then(JsonValue::as_array)
        .expect("a summary has pass rates")
        .iter()
        .find(|row| {
            row.get("by").and_then(JsonValue::as_str) == Some(by)
                && row.get("name").and_then(JsonValue::as_str) == Some(name)
        })
        .unwrap_or_else(|| panic!("no row for {by} {name}"));
    ["func@1", "func_sec@1", "func@3", "func_sec@3"]
        .iter()
        .map(|key| match row.get(key) {
            Some(JsonValue::Number(n)) => n.clone(),
            other => panic!("{key} is {other:?}"),
        })
        .collect()
}

/// Per task and variant, of 3 runs each: greeting/base passes 2 and 1
/// (one run left a `todo()`, which counts against func-sec),
/// greeting/hook 3 and 2, files/base 0 and 0, files/hook 1 and 0. So
/// func@1 is the mean of 2/3, 1, 0 and 1/3, and func@3 is 1 for each pair
/// with a pass.
#[test]
fn func_at_k_and_func_sec_at_k_by_batch_class_and_variant() {
    let summary = parse_json(&summarise("2026-11-20-hook")).expect("the summary is JSON");
    assert_eq!(
        rates(&summary, "all", "all"),
        ["0.5000", "0.2500", "0.7500", "0.5000"]
    );
    assert_eq!(
        rates(&summary, "class", "xss"),
        ["0.8333", "0.5000", "1.0000", "1.0000"]
    );
    assert_eq!(
        rates(&summary, "class", "path_traversal"),
        ["0.1667", "0.0000", "0.5000", "0.0000"]
    );
    assert_eq!(
        rates(&summary, "variant", "base"),
        ["0.3333", "0.1667", "0.5000", "0.5000"]
    );
    assert_eq!(
        rates(&summary, "variant", "hook"),
        ["0.6667", "0.3333", "1.0000", "0.5000"]
    );
}

#[test]
fn an_earlier_batch_with_two_attempts_has_k_of_1_and_2() {
    let summary = summarise("2026-11-01-first");
    assert!(summary.contains("\"k\": [1, 2]"), "{summary}");
}

#[test]
fn a_batch_that_mixes_task_set_digests_is_bad_input() {
    let outcome = harness(&["summarise", "tests/fixtures/summarise/mixed"]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("more than one task set"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_run_recorded_twice_is_bad_input() {
    let scratch = Scratch::new();
    copy_tree(
        std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/results/2026-11-01-first"
        )),
        scratch.path(),
    );
    let runs = scratch.path().join("runs");
    std::fs::copy(
        runs.join("greeting-base-1.json"),
        runs.join("greeting-base-1-again.json"),
    )
    .expect("should copy the record");
    let outcome = harness(&["summarise", scratch.str()]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome
            .stdout
            .contains("attempt 1 of `greeting` in `base` is recorded twice"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_record_of_another_schema_or_no_runs_is_bad_input() {
    let scratch = Scratch::new();
    let outcome = harness(&["summarise", scratch.str()]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("no run records"),
        "{}",
        outcome.stdout
    );

    let runs = scratch.path().join("runs");
    std::fs::create_dir_all(&runs).expect("should make runs/");
    std::fs::write(runs.join("a.json"), "{\"schema\": 2}\n").expect("should write");
    let outcome = harness(&["summarise", scratch.str()]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("schema 1"), "{}", outcome.stdout);
}
