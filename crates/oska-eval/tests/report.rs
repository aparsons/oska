//! `oska-eval report`: the Markdown page of results, rendered from the
//! committed batches. The fixture batches are under
//! `tests/fixtures/results/`; the page is compared with
//! `tests/golden/report.md`. The site publishes `docs/evaluation.md`, and a
//! test holds it to what `report` renders from the repo's `eval/results/`.
//! Refresh with `UPDATE_GOLDEN=1 cargo test -p oska-eval --test report`
//! and review the diff. No test calls a model.

mod common;

use common::{assert_file, assert_golden, copy_tree, harness, Scratch};
use std::path::Path;

const RESULTS: &str = "tests/fixtures/results";

#[test]
fn the_page_shows_the_latest_batch_in_full_and_the_earlier_ones_in_a_table() {
    let outcome = harness(&["report", RESULTS]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("report.md", &outcome.stdout);
    let page = &outcome.stdout;
    let latest = page.find("## Latest batch").expect("a latest batch");
    let earlier = page.find("## Earlier batches").expect("earlier batches");
    assert!(latest < earlier, "{page}");
    assert!(
        page[latest..earlier].contains("2026-11-20-hook"),
        "the newest batch is the latest: {page}"
    );
    assert!(page[earlier..].contains("2026-11-01-first"), "{page}");
    assert!(page.contains("## Limitations"), "{page}");
}

#[test]
fn with_no_batch_the_page_says_so_and_still_states_its_limitations() {
    let scratch = Scratch::new();
    let outcome = harness(&["report", scratch.str()]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("report_empty.md", &outcome.stdout);
    assert!(outcome.stdout.contains("No batch has been run yet."));
    assert!(outcome.stdout.contains("## Limitations"));
}

#[test]
fn out_writes_the_page_to_a_file_and_says_where() {
    let scratch = Scratch::new();
    let page = scratch.path().join("evaluation.md");
    let page = page.to_str().expect("UTF-8 path");
    let outcome = harness(&["report", RESULTS, "--out", page]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("\"batches\": 2"),
        "{}",
        outcome.stdout
    );
    let written = std::fs::read_to_string(page).expect("the page is written");
    assert_eq!(written, harness(&["report", RESULTS]).stdout);
}

/// A summary that its runs no longer give is stale: the page would not
/// agree with the data.
#[test]
fn a_stale_or_missing_summary_is_bad_input_and_writes_nothing() {
    let scratch = Scratch::new();
    let results = scratch.path().join("results");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(RESULTS),
        &results,
    );
    let summary = results.join("2026-11-01-first/summary.json");
    let stale = std::fs::read_to_string(&summary)
        .expect("a summary")
        .replace("\"total\": 2", "\"total\": 3");
    std::fs::write(&summary, stale).expect("should write");
    let page = scratch.path().join("evaluation.md");
    let outcome = harness(&[
        "report",
        results.to_str().expect("UTF-8"),
        "--out",
        page.to_str().expect("UTF-8"),
    ]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("is stale"), "{}", outcome.stdout);
    assert!(!page.exists(), "nothing is written on bad input");

    std::fs::remove_file(&summary).expect("should remove");
    let outcome = harness(&["report", results.to_str().expect("UTF-8")]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("summary.json"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_missing_results_directory_is_bad_input() {
    let outcome = harness(&["report", "tests/fixtures/no-such-results"]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("cannot read"), "{}", outcome.stdout);
}

/// The site's Evaluation page is `docs/evaluation.md`: it must be what
/// `report` renders from the committed results in `eval/results/`.
#[test]
fn the_published_page_matches_the_committed_results() {
    let outcome = harness(&["report", "../../eval/results"]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_file("../../docs/evaluation.md", &outcome.stdout);
}
