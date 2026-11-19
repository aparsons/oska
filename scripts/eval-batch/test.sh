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
# with web access and `oskac fix` and no hook), a budget of $4, an estimate
# of $5 a run and a cap of $6 a run.
setup() {
  local attempts=${1:-1} timeout=${2:-60}
  local variants=${3:-'[{"name": "base", "web": true, "hook": false, "fix": true}]'}
  local budget=${4:-5} estimate=${5:-0.5} cap=${6:-2}
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
  "estimate_usd_per_run": $estimate,
  "cap_usd_per_run": $cap,
  "estimate_tokens_per_run": 50000
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

# The arguments of the agent's call $1 (from 1), one per line.
call_args() {
  awk -v k="$1" '/^call$/ { n++ } n == k && /^arg / { sub(/^arg /, ""); print }' stub.log
}

# The values call $1 gave the variadic flag $2: its arguments up to the
# next flag.
flag_values() {
  call_args "$1" | awk -v flag="$2" '$0 == flag { on = 1; next } on && /^--/ { exit } on { print }'
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
  grep -q '^  estimate:  1.5000 US dollars (0.5000 a run) and 150000 tokens$' out || fail "no estimate"
  grep -q '^  at worst:  6.0000 US dollars (the cap of 2.0000 a run)$' out || fail "no worst case"
  grep -q '^    base: site true, hook false, oskac fix true$' out || fail "no variant line"
  grep -q "^    $task in base: 3 run(s), 40 turns and 60 s each$" out || fail "no per-task line"
  grep -q 'Dry run: nothing ran' out || fail "not called a dry run"
  [ "$(calls)" = 0 ] || fail "the agent ran"
  [ ! -e results ] || fail "results were written"
}

test_a_plan_over_the_budget_is_refused() {
  setup 3 60 '[{"name": "base", "web": true, "hook": false, "fix": true}]' 1 0.5
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
  setup 1 60 '[{"name": "base", "web": true, "hook": false, "fix": true}, {"name": "offline", "web": false, "hook": true, "fix": false}]'
  echo test-batch | drive --live >out 2>&1 || fail "the batch failed: $(cat out)"
  local runs=results/test-batch/runs
  [ "$(calls)" = 2 ] || fail "$(calls) agent runs, not 2"
  for record in "$runs/$task-base-1.json" "$runs/$task-offline-1.json"; do
    [ -f "$record" ] || fail "no $record: $(cat out)"
    [ "$(jq -r '[.stop, .grade.passed, .grade.secure, .grade.clean, .usage.cost_usd, .model, .agent.version, .oskac.version] | join(" ")' "$record")" \
      = "finished true true true 0.5 test-model 9.9.9 $("$bin/oskac" --version | awk '{ print $2 }')" ] ||
      fail "unexpected record $record: $(cat "$record")"
    [ "$(jq -c .grade.changed "$record")" = '["src/page.osk"]' ] || fail "the setup files counted as changes: $(cat "$record")"
    jq -r .task_set_digest "$record" | grep -Eqx 'sha256:[0-9a-f]{64}' || fail "no task-set digest"
    jq -r .oskac.commit "$record" | grep -Eq '^[0-9a-f]{40}(-dirty)?$' || fail "no oskac commit"
  done
  cmp test-batch.json results/test-batch/batch.json || fail "the config was not kept"
  jq -e '.config.variants[1] == {"name": "offline", "web": false, "hook": true, "fix": false}' results/test-batch/summary.json >/dev/null ||
    fail "the summary lacks the config"
  grep -q 'Batch\*\*: test-batch' evaluation.md || fail "no report"
  if grep -rq "$scratch" results evaluation.md; then
    fail "a local path reached the results"
  fi
  # The agent ran in its workspace, with the agent-setup files, the task's
  # prompt and limits.
  local base_run offline_run
  base_run=$(cd work/test-batch/$task/base/1 && pwd -P)
  offline_run=$(cd work/test-batch/$task/offline/1 && pwd -P)
  call_args 1 >args1
  call_args 2 >args2
  grep -qx "cwd $base_run/workspace" stub.log || fail "not run in its workspace: $(cat stub.log)"
  cmp "$base_run/workspace/AGENTS.md" "$repo/site/agent-setup/AGENTS.template.md" || fail "no AGENTS.md"
  grep -qF "$(head -n 1 "tasks/$task/prompt.md")" stub.log || fail "the prompt was not given"
  for flag in -p --restricted --strict-mcp-config --no-session-persistence; do
    grep -qx -- "$flag" args1 || fail "no $flag"
  done
  [ "$(grep -x -A1 -- --max-turns args1 | tail -n 1)" = 40 ] || fail "no turn limit of 40"
  [ "$(grep -x -A1 -- --max-budget-usd args1 | tail -n 1)" = 2.0000 ] || fail "no cap of 2.0000"
  [ "$(grep -x -A1 -- --model args1 | tail -n 1)" = test-model ] || fail "not the config's model"
  [ "$(grep -x -A1 -- --permission-mode args1 | tail -n 1)" = dontAsk ] || fail "not dontAsk"
  [ "$(grep -x -A1 -- --permission-prompts args1 | tail -n 1)" = none ] || fail "prompts not refused"
  # Nothing may bypass the allowlist.
  if grep -qiE -- 'dangerously|bypassPermissions' args1 args2; then
    fail "a call can bypass permissions: $(cat args1 args2)"
  fi
  # The tools: file tools scoped to the workspace, only the loop's oskac
  # subcommands, and fix and the site as the variant says.
  [ "$(grep -x -A1 -- --tools args1 | tail -n 1)" = 'Read,Edit,Write,Glob,Grep,Bash,WebFetch' ] || fail "base: wrong tools"
  [ "$(grep -x -A1 -- --tools args2 | tail -n 1)" = 'Read,Edit,Write,Glob,Grep,Bash' ] || fail "offline: wrong tools"
  printf '%s\n' "Read(/$base_run/workspace/**)" "Edit(/$base_run/workspace/**)" Glob Grep \
    'Bash(oskac check)' 'Bash(oskac check *)' 'Bash(oskac test)' 'Bash(oskac test *)' \
    'Bash(oskac explain *)' 'Bash(oskac fix)' 'Bash(oskac fix *)' 'WebFetch(domain:oskalang.dev)' >expected
  flag_values 1 --allowedTools >allowed
  diff expected allowed || fail "base: the allowlist is not exactly the scoped list"
  printf '%s\n' "Read(/$offline_run/workspace/**)" "Edit(/$offline_run/workspace/**)" Glob Grep \
    'Bash(oskac check)' 'Bash(oskac check *)' 'Bash(oskac test)' 'Bash(oskac test *)' \
    'Bash(oskac explain *)' >expected
  flag_values 2 --allowedTools >allowed
  diff expected allowed || fail "offline: the allowlist is not exactly the scoped list"
  flag_values 2 --disallowedTools >denied
  printf '%s\n' 'Bash(oskac run *)' 'Bash(oskac build *)' WebSearch 'Bash(oskac fix)' 'Bash(oskac fix *)' WebFetch >expected
  diff expected denied || fail "offline: the deny list is wrong"
  # The variant's settings: the published check-on-edit hook, or none.
  [ "$(grep -x -A1 -- --settings args2 | tail -n 1)" = "$offline_run/claude-settings.json" ] || fail "no settings"
  cmp "$offline_run/claude-settings.json" "$repo/site/agent-setup/claude-settings-post-edit.json" || fail "the hook variant has no hook"
  [ "$(cat "$base_run/claude-settings.json")" = '{}' ] || fail "the base variant has settings"
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
  setup 3 60 '[{"name": "base", "web": true, "hook": false, "fix": true}]' 0.75 0.25 1
  drive --live --yes >out 2>&1 || fail "the batch failed: $(cat out)"
  [ "$(calls)" = 2 ] || fail "$(calls) agent runs, not 2"
  grep -q 'Stopped early: the recorded cost, 1 US dollars, reached the budget of 0.7500' out ||
    fail "no budget stop: $(cat out)"
  # The run not made is recorded as stopped by the budget, never dropped.
  [ "$(jq -r '[.stop, (.grade == null), (.usage == null)] | join(" ")' "results/test-batch/runs/$task-base-3.json")" = "budget true true" ] ||
    fail "the third run is not recorded as budget"
  jq -e '.runs.budget == 1 and .runs.finished == 2 and .incomplete == []' \
    results/test-batch/summary.json >/dev/null || fail "the summary does not count the budget stop"
}

# A run whose cost is unknown counts at the cap: never as free.
test_a_run_of_unknown_cost_counts_at_the_cap_against_the_budget() {
  # A budget of 2.5 and a cap of 2: a failed run (unknown cost) counts 2,
  # the next run costs 0.5, and the third would pass the budget.
  setup 3 60 '[{"name": "base", "web": true, "hook": false, "fix": true}]' 2.5 0.5 2
  local status=0
  STUB_TRANSCRIPT=/dev/null STUB_EXIT=1 drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'Recorded cost: 2.0000 US dollars' out || fail "the failed run did not count at the cap: $(cat out)"
  drive --live --yes >out 2>&1 || fail "the resumed batch failed: $(cat out)"
  [ "$(calls)" = 2 ] || fail "$(calls) agent runs, not 2"
  [ "$(jq -r .stop "results/test-batch/runs/$task-base-3.json")" = budget ] || fail "the third run was not stopped"
}

test_an_agent_cli_that_fails_without_a_result_is_recorded_as_an_error_and_stops_the_batch() {
  setup 2
  local status=0
  STUB_TRANSCRIPT=/dev/null STUB_EXIT=1 drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'the agent CLI exited with 1 and no result' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  [ "$(jq -r '[.stop, (.usage == null)] | join(" ")' "results/test-batch/runs/$task-base-1.json")" = "error true" ] ||
    fail "the run is not recorded as an error"
  [ ! -e "results/test-batch/runs/$task-base-2.json" ] || fail "the batch went on"
}

test_a_malformed_transcript_is_recorded_as_an_error_and_stops_the_batch() {
  setup 2
  printf '{"type": "result", oops\n' >bad.jsonl
  local status=0
  STUB_TRANSCRIPT=$PWD/bad.jsonl drive --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'oska-eval analyse could not read the transcript (the agent exited with 0)' out || fail "no reason: $(cat out)"
  [ "$(calls)" = 1 ] || fail "$(calls) agent runs, not 1"
  [ "$(jq -r '[.stop, (.analysis == null)] | join(" ")' "results/test-batch/runs/$task-base-1.json")" = "error true" ] ||
    fail "the run is not recorded as an error"
}

# A record that cannot be written leaves nothing behind that looks recorded.
test_a_failed_record_leaves_no_partial_record() {
  setup
  mkdir fakebin
  ln -s "$bin/oskac" fakebin/oskac
  # oska-eval, except that `record` fails.
  cat >fakebin/oska-eval <<'SH'
#!/usr/bin/env bash
if [ "$1" = record ]; then
  echo '{"errors": ["boom"]}'
  exit 2
fi
exec "$REAL_HARNESS" "$@"
SH
  chmod +x fakebin/oska-eval
  local status=0
  REAL_HARNESS=$bin/oska-eval env -u CI -u GITHUB_ACTIONS "$here/run.sh" test-batch.json --tasks tasks --results results \
    --work work --bin fakebin --report evaluation.md --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "exit code $status, not 1"
  grep -q 'oska-eval record failed: {"errors": \["boom"\]}' out || fail "no reason: $(cat out)"
  [ -z "$(ls -A results/test-batch/runs)" ] || fail "something was left in runs/: $(ls -A results/test-batch/runs)"
}

# The workspaces must be outside the repo, links resolved: an agent there
# could read the hidden checks and the solutions.
test_attack_a_work_directory_inside_the_repo_is_refused() {
  setup
  local inside=$repo/target/eval-batch-test-$$
  ln -s "$repo" repo-link
  local status
  for work in "$inside" "$PWD/repo-link/target/eval-batch-test-$$"; do
    status=0
    env -u CI -u GITHUB_ACTIONS "$here/run.sh" test-batch.json --tasks tasks --results results \
      --work "$work" --bin "$bin" --report evaluation.md --live --yes >out 2>&1 || status=$?
    [ "$status" -eq 1 ] || fail "$work: exit code $status, not 1"
    grep -q 'is inside the repo' out || fail "$work: no reason: $(cat out)"
  done
  rmdir "$inside"
  status=0
  mkdir 'a b'
  env -u CI -u GITHUB_ACTIONS "$here/run.sh" test-batch.json --tasks tasks --results results \
    --work 'a b' --bin "$bin" --report evaluation.md --live --yes >out 2>&1 || status=$?
  [ "$status" -eq 1 ] || fail "a space: exit code $status, not 1"
  grep -q 'may hold only letters' out || fail "a space: no reason: $(cat out)"
  [ "$(calls)" = 0 ] || fail "the agent ran"
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
