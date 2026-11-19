#!/usr/bin/env bash
# Tests for the evaluation driver, scripts/eval-batch/run.sh. Run from
# anywhere:
#
#   scripts/eval-batch/test.sh
#
# Offline: no test calls a model or needs a credential. A stub `claude`
# (stub-claude.sh) on PATH replays fixtures/transcript.jsonl and leaves the
# task's reference solution in the workspace, so the whole pipeline (plan,
# prepare, the agent, grade, analyse, record, summarise, report) runs on
# real files. The tests run the driver as a local maintainer would, with
# CI and GITHUB_ACTIONS unset, against a copy of one real task; one test
# checks that the driver refuses a live batch when either is set.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

(cd "$repo" && cargo build --quiet -p oskac -p oska-eval)
target=${CARGO_TARGET_DIR:-$repo/target}
case $target in
  /*) ;;
  *) target=$repo/$target ;;
esac
bin=$target/debug

failures=0
passes=0

fail() {
  echo "    $*" >&2
  return 1
}

run_test() {
  local name=$1
  local dir="$scratch/$name"
  mkdir -p "$dir"
  local status
  set +e
  (
    set -e
    cd "$dir"
    "$name"
  ) >"$dir.log" 2>&1
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    passes=$((passes + 1))
    echo "ok   $name"
  else
    failures=$((failures + 1))
    echo "FAIL $name"
    sed 's/^/    /' "$dir.log"
  fi
}

task=xss-greeting

# A task set with one real task, its attempts set to $1 (default 1) and its
# timeout to $2 seconds (default 60); the stub `claude` first on PATH; and a
# batch config, test-batch.json, with the variants $3 (default one, `base`,
# with web access), a budget of $4 and an estimate of $5 a run.
setup() {
  local attempts=${1:-1} timeout=${2:-60}
  local variants=${3:-'[{"name": "base", "web": true}]'}
  local budget=${4:-5} estimate=${5:-0.5}
  mkdir -p tasks stub
  cp -R "$repo/eval/tasks/$task" tasks/
  jq --argjson a "$attempts" --argjson t "$timeout" \
    '.attempts = $a | .timeout_seconds = $t' "$repo/eval/tasks/$task/task.json" \
    >"tasks/$task/task.json"
  cp "$here/stub-claude.sh" stub/claude
  chmod +x stub/claude
  cat >test-batch.json <<JSON
{
  "schema": 1,
  "model": "test-model",
  "tasks": ["$task"],
  "variants": $variants,
  "budget_usd": $budget,
  "estimate_usd_per_run": $estimate
}
JSON
  export STUB_LOG=$PWD/stub.log
  export STUB_TRANSCRIPT=$here/fixtures/transcript.jsonl
  export STUB_SOLUTION=$PWD/tasks/$task/solutions/reference
  export PATH="$PWD/stub:$PATH"
}

# Runs the driver as a maintainer would on their own machine.
drive() {
  env -u CI -u GITHUB_ACTIONS "$here/run.sh" test-batch.json --tasks tasks \
    --results results --work work --bin "$bin" --report evaluation.md "$@"
}

calls() {
  if [ -e stub.log ]; then grep -c '^call$' stub.log; else echo 0; fi
}

test_without_a_config_it_prints_its_usage() {
  local status=0
  "$here/run.sh" >out 2>&1 || status=$?
  [ "$status" -eq 2 ] || fail "exit code $status, not 2"
  grep -q 'usage: scripts/eval-batch/run.sh <config>' out || fail "no usage"
}

test_a_dry_run_prints_the_plan_and_runs_nothing() {
  setup 3
  drive >out 2>&1 || fail "the dry run failed: $(cat out)"
  grep -q '^Batch test-batch: test-model on 1 task(s) in 1 variant(s)$' out || fail "no batch line: $(cat out)"
  grep -q '^  runs:      3$' out || fail "no run count"
  grep -q '^  estimate:  1.5000 US dollars (0.5000 a run)$' out || fail "no estimate"
  grep -q "^    $task in base: 3 run(s), 40 turns and 60 s each$" out || fail "no per-task line"
  grep -q 'Dry run: nothing ran' out || fail "not called a dry run"
  [ "$(calls)" = 0 ] || fail "the agent ran"
  [ ! -e results ] || fail "results were written"
}

test_a_plan_over_the_budget_is_refused() {
  setup 3 60 '[{"name": "base", "web": true}]' 1 0.5
  local status=0
  drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'the estimate is over the budget' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 0 ] || fail "the agent ran"
}

test_a_bad_config_is_refused_before_anything_runs() {
  setup
  jq '.tasks = ["no-such-task"]' test-batch.json >c && mv c test-batch.json
  local status=0
  drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -qF "no task \`no-such-task\`" out || fail "no reason: $(cat out)"
  [ "$(calls)" = 0 ] || fail "the agent ran"
}

test_it_refuses_a_live_batch_in_ci() {
  setup
  local status
  for var in CI GITHUB_ACTIONS; do
    status=0
    env "$var=true" "$here/run.sh" test-batch.json --tasks tasks --results results \
      --work work --bin "$bin" --report evaluation.md --live --yes >out 2>&1 || status=$?
    [ "$status" -eq 1 ] || fail "$var: exit code $status, not 1"
    grep -q 'refusing a live batch in CI' out || fail "$var: no reason: $(cat out)"
  done
  [ "$(calls)" = 0 ] || fail "the agent ran"
  [ ! -e results ] || fail "results were written"
}

test_it_refuses_a_live_batch_that_is_not_confirmed() {
  setup
  local status=0
  drive --live </dev/null >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "no answer: exit code $status, not 1"
  grep -q 'not confirmed; nothing ran' out || fail "no reason: $(cat out)"
  status=0
  echo yes | drive --live >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "a wrong answer: exit code $status, not 1"
  [ "$(calls)" = 0 ] || fail "the agent ran"
  [ ! -e results ] || fail "results were written"
}

test_a_live_batch_runs_every_step_and_records_each_run() {
  setup 1 60 '[{"name": "base", "web": true}, {"name": "offline", "web": false}]'
  echo test-batch | drive --live >out 2>&1 || fail "the batch failed: $(cat out)"
  local runs=results/test-batch/runs
  [ "$(calls)" = 2 ] || fail "$(calls) agent runs, not 2"
  for record in "$runs/$task-base-1.json" "$runs/$task-offline-1.json"; do
    [ -f "$record" ] || fail "no $record: $(cat out)"
    [ "$(jq -r '[.stop, .grade.passed, .grade.secure, .usage.cost_usd, .model, .agent.version, .oskac.version] | join(" ")' "$record")" \
      = "finished true true 0.5 test-model 9.9.9 $("$bin/oskac" --version | awk '{ print $2 }')" ] ||
      fail "unexpected record $record: $(cat "$record")"
    jq -r .task_set_digest "$record" | grep -Eqx 'sha256:[0-9a-f]{64}' || fail "no task-set digest"
    jq -r .oskac.commit "$record" | grep -Eq '^[0-9a-f]{40}(-dirty)?$' || fail "no oskac commit"
  done
  cmp test-batch.json results/test-batch/batch.json || fail "the config was not kept"
  jq -e '.config.variants[1] == {"name": "offline", "web": false}' results/test-batch/summary.json >/dev/null ||
    fail "the summary lacks the config"
  grep -q 'Batch\*\*: test-batch' evaluation.md || fail "no report"
  if grep -rq "$scratch" results evaluation.md; then
    fail "a local path reached the results"
  fi
  # The agent ran in its workspace, with the task's prompt and limits.
  local workspace=$PWD/work/test-batch/$task/base/1/workspace
  grep -qx "cwd $workspace" stub.log || fail "not run in $workspace: $(cat stub.log)"
  grep -qx 'arg --max-turns' stub.log || fail "no turn limit"
  grep -qx 'arg --model' stub.log || fail "no model flag"
  grep -qx 'arg test-model' stub.log || fail "not the config's model"
  grep -qx 'arg Read,Edit,Write,Glob,Grep,Bash(oskac:\*),WebFetch(domain:oskalang.dev)' stub.log ||
    fail "the web variant's tools are wrong"
  grep -qx 'arg Read,Edit,Write,Glob,Grep,Bash(oskac:\*)' stub.log || fail "the offline variant's tools are wrong"
  grep -qF "$(head -n 1 "tasks/$task/prompt.md")" stub.log || fail "the prompt was not given"
  # The transcript is data: nothing in it was run.
  [ -z "$(find "$scratch" -name 'pwned*')" ] || fail "the transcript's text was run"
}

test_a_resumed_batch_skips_the_runs_already_recorded() {
  setup 2
  drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  rm "results/test-batch/runs/$task-base-2.json" stub.log
  drive --live --yes >out 2>&1 || fail "the resumed batch failed: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  grep -q 'attempt 1: recorded already' out || fail "attempt 1 not skipped: $(cat out)"
  [ -f "results/test-batch/runs/$task-base-2.json" ] || fail "attempt 2 not recorded"
}

test_an_interrupted_run_is_kept_aside_and_run_again() {
  setup
  local run_dir=work/test-batch/$task/base/1
  mkdir -p "$run_dir/workspace"
  echo half-done >"$run_dir/workspace/note"
  drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  [ -f "results/test-batch/runs/$task-base-1.json" ] || fail "the run was not recorded"
  [ ! -e "$run_dir/workspace/note" ] || fail "the run reused the interrupted directory"
  ls -d "$run_dir".interrupted-*/workspace/note >/dev/null || fail "the interrupted run was not kept"
}

test_a_batch_resumes_only_with_its_own_config() {
  setup 2
  drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  rm "results/test-batch/runs/$task-base-2.json" stub.log
  jq '.budget_usd = 50' test-batch.json >c && mv c test-batch.json
  local status=0
  drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'a batch resumes only with its own config' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 0 ] || fail "the agent ran"
}

test_a_run_that_times_out_is_recorded_as_a_timeout() {
  setup 1 1
  STUB_SLEEP=10 drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  local record=results/test-batch/runs/$task-base-1.json
  [ "$(jq -r '[.stop, .grade.passed] | join(" ")' "$record")" = "timeout false" ] ||
    fail "unexpected record: $(cat "$record")"
}

test_no_run_starts_once_the_recorded_cost_reaches_the_budget() {
  # 3 runs at an estimated 0.25 fit a budget of 0.75; each costs 0.5.
  setup 3 60 '[{"name": "base", "web": true}]' 0.75 0.25
  drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  [ "$(calls)" = 2 ] || fail "$(calls) agent runs, not 2"
  grep -q 'Stopped early: the recorded cost, 1 US dollars, reached the budget of 0.7500' out ||
    fail "no budget stop: $(cat out)"
  jq -e '.incomplete == [{"task": "xss-greeting", "variant": "base", "runs": 2, "attempts": 3}]' \
    results/test-batch/summary.json >/dev/null || fail "the summary does not say the batch is incomplete"
}

test_an_agent_cli_that_fails_without_a_result_stops_the_batch_unrecorded() {
  setup 2
  local status=0
  STUB_TRANSCRIPT=/dev/null STUB_EXIT=1 drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'the agent CLI exited with 1 and no result' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  [ -z "$(ls results/test-batch/runs)" ] || fail "a run was recorded"
}

test_a_malformed_transcript_stops_the_batch_unrecorded() {
  setup 2
  printf '{"type": "result", oops\n' >bad.jsonl
  local status=0
  STUB_TRANSCRIPT=$PWD/bad.jsonl drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'oska-eval analyse failed (the agent exited with 0)' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  [ -z "$(ls results/test-batch/runs)" ] || fail "a run was recorded"
}

test_it_fails_closed_without_the_agent_cli_or_the_harness() {
  setup
  # A PATH with the tools the driver uses, but no `claude`.
  mkdir tools
  local tool
  for tool in bash env jq git timeout awk date mkdir cp mv cat find sort xargs dirname mktemp rm cmp sha256sum sed; do
    ln -s "$(command -v "$tool")" "tools/$tool"
  done
  local status=0
  PATH=$PWD/tools drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "no claude: exit code $status, not 1"
  grep -q 'claude is not installed' out || fail "no reason: $(cat out)"
  mkdir empty
  status=0
  env -u CI -u GITHUB_ACTIONS "$here/run.sh" test-batch.json --tasks tasks --bin empty >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "no harness: exit code $status, not 1"
  grep -q 'no oska-eval at' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 0 ] || fail "the agent ran"
}

# The driver is never run by a workflow: CI reaches it only through this
# suite, with the stub on PATH.
test_no_workflow_runs_the_driver() {
  if grep -rn 'eval-batch' "$repo/.github/workflows"; then
    fail "a workflow names scripts/eval-batch"
  fi
}

for name in $(declare -F | awk '{ print $3 }' | grep '^test_'); do
  run_test "$name"
done
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
