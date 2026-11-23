// SPDX-License-Identifier: Apache-2.0
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

/// The row of `rows` whose `key` is `value`.
fn row<'a>(rows: &'a [JsonValue], key: &str, value: &str) -> &'a JsonValue {
    rows.iter()
        .find(|r| r.get(key).and_then(JsonValue::as_str) == Some(value))
        .unwrap_or_else(|| panic!("no row with {key} {value}"))
}

fn number(value: &JsonValue, key: &str) -> String {
    match value.get(key) {
        Some(JsonValue::Number(n)) => n.clone(),
        Some(JsonValue::Null) => "null".to_string(),
        other => panic!("{key} is {other:?}"),
    }
}

/// `E0300` was seen 3 times, each with a fix: applied twice, and gone at
/// the next check after one of those. `E0206` was seen twice, once with a
/// fix, which was applied and helped.
#[test]
fn each_code_has_its_fix_rates_for_the_batch_and_for_each_variant() {
    let summary = parse_json(&summarise("2026-11-20-hook")).expect("the summary is JSON");
    let codes = summary
        .get("codes")
        .and_then(JsonValue::as_array)
        .expect("codes");
    let e0300 = row(codes, "code", "E0300");
    let rates: Vec<String> = [
        "fix_offered_rate",
        "fix_applied_rate",
        "fix_helped_rate",
        "mean_iterations_to_resolve",
    ]
    .iter()
    .map(|k| number(e0300, k))
    .collect();
    assert_eq!(rates, ["1.0000", "0.6667", "0.5000", "1.5000"]);
    let e0206 = row(codes, "code", "E0206");
    assert_eq!(number(e0206, "fix_offered_rate"), "0.5000");
    assert_eq!(number(e0206, "fix_helped_rate"), "1.0000");

    let by_variant = summary
        .get("codes_by_variant")
        .and_then(JsonValue::as_array)
        .expect("codes by variant");
    let names = |variant: &str| -> Vec<String> {
        row(by_variant, "variant", variant)
            .get("codes")
            .and_then(JsonValue::as_array)
            .expect("codes")
            .iter()
            .map(|c| {
                c.get("code")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    };
    assert_eq!(names("base"), ["E0300", "E0371", "E0206"]);
    assert_eq!(names("hook"), ["E0206"]);
}

/// k is the attempts the records declare, 3, even though a task has only
/// 2 runs: that task is listed as incomplete, and its rates at k = 3 are
/// null rather than estimated at a smaller k.
#[test]
fn a_task_with_fewer_runs_than_attempts_is_incomplete_and_has_no_rate_at_k() {
    let outcome = harness(&["summarise", "tests/fixtures/summarise/incomplete"]);
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
    let summary = parse_json(&outcome.stdout).expect("the summary is JSON");
    assert!(
        outcome.stdout.contains("\"k\": [1, 3]"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains(
            "\"incomplete\": [\n    {\"task\": \"greeting\", \"variant\": \"base\", \"runs\": 2, \"attempts\": 3}\n  ]"
        ),
        "{}",
        outcome.stdout
    );
    let rows = summary
        .get("pass_rates")
        .and_then(JsonValue::as_array)
        .expect("pass rates");
    let all = row(rows, "by", "all");
    assert_eq!(number(all, "func@1"), "0.5000");
    assert_eq!(number(all, "func@3"), "null");
    assert_eq!(number(all, "func_sec@3"), "null");
}

/// The batch config the driver copied into the batch directory reaches the
/// summary as it is, so the published results say what the batch was
/// asked to run and what it was allowed to spend.
#[test]
fn the_summary_carries_the_batch_config() {
    let summary = parse_json(&summarise("2026-11-20-hook")).expect("the summary is JSON");
    let config = summary.get("config").expect("a config");
    assert_eq!(
        config.get("budget_usd"),
        Some(&JsonValue::Number("20.0000".to_string()))
    );
    let variants = config
        .get("variants")
        .and_then(JsonValue::as_array)
        .expect("variants");
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants[1].get("name").and_then(JsonValue::as_str),
        Some("hook")
    );
}

/// A batch with no config, or whose runs are of a task or variant its
/// config does not list, is bad input: the summary would misstate what ran.
#[test]
fn a_batch_without_its_config_or_with_runs_the_config_does_not_list_is_bad_input() {
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/results/2026-11-20-hook"
    ));
    let cases = [
        (None, "batch.json"),
        (
            Some(("\"name\": \"hook\"", "\"name\": \"other\"")),
            "the run of `files` in `hook` is of a variant the batch config does not list",
        ),
        (
            Some(("\"files\"", "\"other\"")),
            "the run of `files` in `base` is of a task the batch config does not list",
        ),
    ];
    for (edit, message) in cases {
        let scratch = Scratch::new();
        copy_tree(fixture, scratch.path());
        let config = scratch.path().join("batch.json");
        match edit {
            None => std::fs::remove_file(&config).expect("should remove"),
            Some((from, to)) => {
                let text = std::fs::read_to_string(&config).expect("a config");
                std::fs::write(&config, text.replace(from, to)).expect("should write");
            }
        }
        let outcome = harness(&["summarise", scratch.str()]);
        assert_eq!(outcome.code, 2, "{}", outcome.stdout);
        assert!(
            outcome.stdout.contains(message),
            "expected `{message}` in {}",
            outcome.stdout
        );
    }
}
