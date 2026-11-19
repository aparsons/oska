#!/usr/bin/env bash
# The live driver of the evaluation harness: runs a batch of tasks through
# an AI agent CLI (Claude Code, `claude -p`) and records the results. Each
# run costs money, so only the maintainer runs this, by hand, on their own
# machine; it refuses to run live in CI. eval/README.md ("Running a batch")
# says how to run one and what it costs.
#
#   scripts/eval-batch/run.sh <config> [--live] [--yes] [--tasks DIR]
#       [--results DIR] [--work DIR] [--bin DIR] [--report FILE]
#
# <config> is a batch config, named <batch-id>.json. Without --live it
# prints the batch's plan (every run, the most turns and time, and the cost
# estimate against the budget) and runs nothing. With --live it prints the
# plan, asks for the batch id to be typed (or takes --yes), then for each
# run of the plan, in order:
#
#   oska-eval prepare   a fresh run directory under --work
#   claude -p           in its workspace, with the task's turn limit and
#                       timeout, the config's model and a tool allowlist;
#                       its stream-json transcript stays in the run directory
#   oska-eval grade     the workspace, through the oskac under test
#   oska-eval analyse   the transcript
#   oska-eval record    the run record, in --results/<batch>/runs/
#
# and finally `oska-eval summarise` and `oska-eval report`. A run already
# recorded is skipped, so a batch that stopped resumes where it stopped. No
# run starts once the recorded cost reaches the budget. Any step that fails,
# a missing tool, or an unreadable file stops the batch with an error.
#
# The agent's output is untrusted: it is only ever written to files and
# read by oska-eval, never run, sourced or expanded by this script.
#
# Defaults: --tasks eval/tasks, --results eval/results, --report
# docs/evaluation.md, --bin target/release (where oska-eval and the oskac
# under test are), --work $XDG_STATE_HOME/oska-eval (else
# ~/.local/state/oska-eval), which must be outside the repo.
#
# Exit codes: 0 done (or a dry run), 1 refused or a step failed, 2 usage.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)

# The site the agent may fetch the guide from, in a variant with `web`.
site=oskalang.dev

die() {
  echo "eval-batch: $*" >&2
  exit 1
}

usage() {
  echo "usage: scripts/eval-batch/run.sh <config> [--live] [--yes] [--tasks DIR] [--results DIR] [--work DIR] [--bin DIR] [--report FILE]" >&2
  exit 2
}

config=
live=false
yes=false
tasks=$repo/eval/tasks
results=$repo/eval/results
report=$repo/docs/evaluation.md
bin=$repo/target/release
work=${XDG_STATE_HOME:-${HOME:?HOME is not set}/.local/state}/oska-eval
while [ $# -gt 0 ]; do
  case $1 in
    --live) live=true ;;
    --yes) yes=true ;;
    --tasks | --results | --work | --bin | --report)
      [ $# -ge 2 ] || usage
      case $1 in
        --tasks) tasks=$2 ;;
        --results) results=$2 ;;
        --work) work=$2 ;;
        --bin) bin=$2 ;;
        --report) report=$2 ;;
      esac
      shift
      ;;
    -*) usage ;;
    *)
      [ -z "$config" ] || usage
      config=$1
      ;;
  esac
  shift
done
[ -n "$config" ] || usage
[ -f "$config" ] || die "no batch config at $config"

# Every path absolute, so that each step finds it from any directory.
absolute() {
  case $1 in
    /*) printf '%s' "$1" ;;
    *) printf '%s/%s' "$PWD" "$1" ;;
  esac
}
config=$(absolute "$config")
tasks=$(absolute "$tasks")
results=$(absolute "$results")
report=$(absolute "$report")
bin=$(absolute "$bin")
work=$(absolute "$work")

need() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is not installed; it is needed to $2"
}
need jq "read the plan and the run records"
harness=$bin/oska-eval
oskac=$bin/oskac
[ -x "$harness" ] || die "no oska-eval at $harness; build it: cargo build --release -p oskac -p oska-eval"
[ -x "$oskac" ] || die "no oskac at $oskac; build it: cargo build --release -p oskac -p oska-eval"

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# The plan. oska-eval checks the config and the tasks; it exits with 1
# when the estimate is over the budget and 2 on a bad config.
plan=$scratch/plan.json
set +e
"$harness" plan "$tasks" "$config" >"$plan"
status=$?
set -e
if [ "$status" -eq 2 ]; then
  cat "$plan" >&2
  die "the batch config does not plan (the report is above)"
fi
[ "$status" -le 1 ] || die "oska-eval plan failed with exit code $status"

batch=$(jq -r .batch "$plan")
model=$(jq -r .config.model "$plan")
budget=$(jq -r .config.budget_usd "$plan")
cap=$(jq -r .config.cap_usd_per_run "$plan")
jq -r '
  "Batch \(.batch): \(.config.model) on \(.config.tasks | length) task(s) in \(.config.variants | length) variant(s)",
  "  runs:      \(.estimate.runs)",
  "  at most:   \(.estimate.turns_at_most) turns and \(.estimate.seconds_at_most) seconds of agent time",
  "  estimate:  \(.estimate.cost_usd) US dollars (\(.config.estimate_usd_per_run) a run) and \(.estimate.tokens) tokens",
  "  at worst:  \(.estimate.cost_usd_at_most) US dollars (the cap of \(.config.cap_usd_per_run) a run)",
  "  budget:    \(.estimate.budget_usd) US dollars; no run starts once the recorded cost reaches it",
  "  per task and variant:",
  (.runs | group_by([.task, .variant])[] | "    \(.[0].task) in \(.[0].variant): \(length) run(s), \(.[0].turn_limit) turns and \(.[0].timeout_seconds) s each"),
  "  variants:",
  (.config.variants[] | "    \(.name): site \(.web), hook \(.hook), oskac fix \(.fix)")
' "$plan"
if [ "$status" -eq 1 ]; then
  die "the estimate is over the budget: raise budget_usd or plan fewer runs"
fi

if [ "$live" != true ]; then
  echo "Dry run: nothing ran. Add --live to run this batch."
  exit 0
fi

# Live runs never happen in CI: no credential belongs there, and no run
# happens unasked.
if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
  die "refusing a live batch in CI: only the maintainer runs one, locally"
fi
need claude "run the agent"
need git "record the oskac commit"
if command -v timeout >/dev/null 2>&1; then
  timeout=timeout
elif command -v gtimeout >/dev/null 2>&1; then
  timeout=gtimeout
else
  die "timeout (GNU coreutils; gtimeout on macOS) is needed to limit each run's time"
fi
if command -v sha256sum >/dev/null 2>&1; then
  sha256=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  sha256=(shasum -a 256)
else
  die "sha256sum or shasum is needed to digest the task set"
fi

# The work directory holds each run's workspace: outside the repo, so the
# agent cannot reach the hidden checks or the solutions (both paths with
# links resolved), and a path the tool rules can name as it is.
mkdir -p "$work"
work=$(cd "$work" && pwd -P)
case $work in
  *[!A-Za-z0-9._/-]*) die "the work directory $work may hold only letters, digits, '.', '_', '-' and '/'; choose another with --work" ;;
esac
repo_real=$(cd "$repo" && pwd -P)
case $work/ in
  "$repo_real"/*) die "the work directory $work is inside the repo $repo_real: an agent there could read the hidden checks; choose another with --work" ;;
esac

if [ "$yes" != true ]; then
  printf 'This batch calls a paid model. Type the batch id (%s) to start it: ' "$batch"
  answer=
  read -r answer || true
  [ "$answer" = "$batch" ] || die "not confirmed; nothing ran"
fi

# What produced the runs, which the harness cannot see for itself.
agent_version=$(claude --version | awk 'NR == 1 { print $1 }')
oskac_version=$("$oskac" --version | awk 'NR == 1 { print $2 }')
oskac_commit=$(git -C "$repo" rev-parse HEAD)
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=no -- . ':(exclude)eval/results' ':(exclude)docs/evaluation.md')" ]; then
  oskac_commit=$oskac_commit-dirty
  echo "eval-batch: the repo has uncommitted changes; the oskac commit is recorded as $oskac_commit" >&2
fi
# The task set's digest: of every file's path and SHA-256, in order.
digest=sha256:$(cd "$tasks" && find . -type f -print0 | LC_ALL=C sort -z |
  xargs -0 "${sha256[@]}" | "${sha256[@]}" | awk '{ print $1 }')
for value in "$agent_version" "$oskac_version" "$oskac_commit"; do
  case $value in
    '' | *[[:space:]]*) die "could not read the agent CLI or oskac version, or the commit" ;;
  esac
done

# The batch directory holds the config it ran; a resumed batch must be
# resumed with the same config.
batch_dir=$results/$batch
mkdir -p "$batch_dir/runs"
if [ -e "$batch_dir/batch.json" ]; then
  cmp -s "$config" "$batch_dir/batch.json" ||
    die "$batch_dir/batch.json differs from $config: a batch resumes only with its own config; name a new batch"
else
  cp "$config" "$batch_dir/batch.json"
fi

# The oskac under test, alone in a directory the agent's PATH starts with,
# copied so that a rebuild during the batch changes nothing.
pinned=$work/$batch/bin
mkdir -p "$pinned"
cp "$oskac" "$pinned/oskac"
oskac=$pinned/oskac

# The batch's cost so far: what each record reports, or the per-run cap for
# a run whose cost is unknown (it failed, or its usage could not be read),
# so that an unknown is never counted as free. A run recorded as `budget`
# never started, and cost nothing.
spent() {
  local records=("$batch_dir"/runs/*.json)
  if [ ! -e "${records[0]}" ]; then
    echo 0
    return
  fi
  jq -s --argjson cap "$cap" \
    'map(if .stop == "budget" then 0 else (.usage.cost_usd // $cap) end) | add' "${records[@]}"
}

now() { date -u +%Y-%m-%dT%H:%M:%SZ; }

# record ARGS...: writes the run record of the current run to $out, from
# `oska-eval record` with the run's provenance and ARGS; a failure leaves no
# partial record behind.
record() {
  if ! "$harness" record "$tasks" "$task" --attempt "$attempt" --variant "$variant" \
    --model "$model" --agent claude-code --agent-version "$agent_version" \
    --oskac-version "$oskac_version" --oskac-commit "$oskac_commit" \
    --task-set "$digest" "$@" >"$out.partial"; then
    local why
    why=$(cat "$out.partial")
    rm -f "$out.partial"
    die "$label: oska-eval record failed: $why"
  fi
  mv "$out.partial" "$out"
}

total=$(jq '.runs | length' "$plan")
n=0
stopped=
failed=
while IFS=$'\t' read -r task variant web hook fix attempt turn_limit timeout_seconds record <&3; do
  n=$((n + 1))
  label="run $n/$total: $task in $variant, attempt $attempt"
  out=$batch_dir/runs/$record
  if [ -e "$out" ]; then
    echo "$label: recorded already"
    continue
  fi
  if [ -z "$stopped" ] &&
    [ "$(jq -n --argjson spent "$(spent)" --argjson budget "$budget" '$spent >= $budget')" = true ]; then
    stopped="the recorded cost, $(spent) US dollars, reached the budget of $budget"
  fi
  if [ -n "$stopped" ]; then
    # Never dropped: a run the budget stopped is recorded as `budget`.
    started=$(now)
    record --started "$started" --ended "$started" --stop budget
    echo "$label: not run (budget)"
    continue
  fi

  run_dir=$work/$batch/$task/$variant/$attempt
  if [ -e "$run_dir" ]; then
    # An interrupted run: keep it for inspection, and run it again.
    mv "$run_dir" "$run_dir.interrupted-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  fi
  mkdir -p "$(dirname "$run_dir")"
  setup=(--setup "$repo/site/agent-setup")
  if [ "$hook" = true ]; then
    setup+=(--hook)
  fi
  "$harness" prepare "$tasks" "$task" "$run_dir" "${setup[@]}" >"$scratch/prepare.json" ||
    die "$label: oska-eval prepare failed: $(cat "$scratch/prepare.json")"

  # The agent's tools: reading and editing in its workspace only, the
  # oskac subcommands the loop needs (not `run` or `build`: the agent's
  # program would run with the maintainer's own authority), `oskac fix` and
  # the Oska site only in the variants that allow them. --restricted ignores
  # the maintainer's own settings files and confines the file tools to the
  # workspace; dontAsk denies anything not allowed here.
  workspace=$run_dir/workspace
  tools=(Read Edit Write Glob Grep Bash)
  allow=("Read(/$workspace/**)" "Edit(/$workspace/**)" Glob Grep
    "Bash(oskac check)" "Bash(oskac check *)" "Bash(oskac test)" "Bash(oskac test *)"
    "Bash(oskac explain *)")
  deny=("Bash(oskac run *)" "Bash(oskac build *)" WebSearch)
  if [ "$fix" = true ]; then
    allow+=("Bash(oskac fix)" "Bash(oskac fix *)")
  else
    deny+=("Bash(oskac fix)" "Bash(oskac fix *)")
  fi
  if [ "$web" = true ]; then
    tools+=(WebFetch)
    allow+=("WebFetch(domain:$site)")
  else
    deny+=(WebFetch)
  fi

  echo "$label: running the agent (at most $turn_limit turns, $timeout_seconds s, $cap US dollars)"
  started=$(now)
  set +e
  (
    cd "$workspace"
    PATH="$pinned:$PATH" "$timeout" --kill-after=30 "$timeout_seconds" \
      claude -p --output-format stream-json --verbose \
      --model "$model" --max-turns "$turn_limit" --max-budget-usd "$cap" \
      --restricted --strict-mcp-config --no-session-persistence \
      --settings "$run_dir/claude-settings.json" \
      --permission-mode dontAsk --permission-prompts none \
      --tools "$(
        IFS=,
        echo "${tools[*]}"
      )" \
      --allowedTools "${allow[@]}" \
      --disallowedTools "${deny[@]}" \
      <"$run_dir/prompt.md" >"$run_dir/transcript.jsonl" 2>"$run_dir/agent.stderr"
  )
  agent_status=$?
  set -e
  ended=$(now)
  stop=()
  timed_out=false
  case $agent_status in
    124 | 137)
      stop=(--stop timeout)
      timed_out=true
      ;;
  esac

  set +e
  "$harness" grade --oskac "$oskac" --setup "$repo/site/agent-setup" "$tasks" "$task" "$run_dir" \
    >"$run_dir/grade.json"
  status=$?
  set -e
  [ "$status" -le 1 ] || die "$label: oska-eval grade failed; see $run_dir/grade.json"
  functional=fail
  if [ "$(jq -r .passed "$run_dir/grade.json")" = true ]; then
    functional=pass
  fi

  analysis=(--analysis "$run_dir/analysis.json")
  set +e
  "$harness" analyse --workspace "$workspace" --functional "$functional" \
    "$run_dir/transcript.jsonl" >"$run_dir/analysis.json"
  status=$?
  set -e
  if [ "$status" -ne 0 ]; then
    # A transcript that cannot be read: the run is still recorded, with no
    # analysis (a timeout may leave none); any reason but the timeout is an
    # error, and stops the batch.
    analysis=()
    if [ "$timed_out" != true ]; then
      stop=(--stop error)
      failed="$label: oska-eval analyse could not read the transcript (the agent exited with $agent_status); see $run_dir"
    fi
  elif [ "$timed_out" != true ] && [ "$agent_status" -ne 0 ] &&
    ! jq -e '.stop == "finished" or .stop == "turn_limit"' "$run_dir/analysis.json" >/dev/null; then
    # The agent CLI failed without finishing (no login, no network, a flag
    # it does not know). The run is recorded as an error, at the per-run
    # cap, and the batch stops before failing every run alike.
    stop=(--stop error)
    failed="$label: the agent CLI exited with $agent_status and no result; see $run_dir/agent.stderr"
  fi

  record --started "$started" --ended "$ended" --workspace "$workspace" \
    --grade "$run_dir/grade.json" ${analysis[@]+"${analysis[@]}"} ${stop[@]+"${stop[@]}"}
  echo "$label: $(jq -r '"\(.stop); func \(.grade.passed // "-"); secure \(.grade.secure // "-"); cost \(.usage.cost_usd // "-") US dollars"' "$out")"
  if [ -n "$failed" ]; then
    break
  fi
done 3< <(jq -r '.runs[] | [.task, .variant, .web, .hook, .fix, .attempt, .turn_limit, .timeout_seconds, .record] | @tsv' "$plan")

"$harness" summarise "$batch_dir" >"$scratch/summary.json" ||
  die "oska-eval summarise failed: $(cat "$scratch/summary.json")"
mv "$scratch/summary.json" "$batch_dir/summary.json"
"$harness" report "$results" --out "$report" >"$scratch/report.json" ||
  die "oska-eval report failed: $(cat "$scratch/report.json")"

echo "Recorded cost: $(spent) US dollars of the $budget budget (a run of unknown cost counts at the cap)."
if [ -n "$failed" ]; then
  die "$failed. The run is recorded as an error; fix the cause and run the batch again to resume"
fi
if [ -n "$stopped" ]; then
  echo "Stopped early: $stopped. The runs not made are recorded as stopped by the budget."
fi
echo "Results: $batch_dir (commit runs/, batch.json and summary.json, and $report)."
echo "Transcripts and workspaces stay in $work/$batch and are never committed."
