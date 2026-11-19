# Evaluation results

The committed results of the evaluation harness: one directory per batch, named by the batch's id (such as `2026-11-20-hook`), holding `batch.json` (the batch config it ran), `runs/` (one run record per run, as `oska-eval record` writes them) and `summary.json` (what `oska-eval summarise` makes of them). Their formats are in [`../README.md`](../README.md#results); [Running a batch](../README.md#running-a-batch) says how a batch is run, what it costs, and what to commit.

The site's Evaluation page, `docs/evaluation.md`, is rendered from this directory:

```sh
target/debug/oska-eval report eval/results --out docs/evaluation.md
```

A test fails when the page does not match what `report` renders from here, or when a batch's `summary.json` is not what its runs give.
