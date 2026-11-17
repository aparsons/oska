#!/usr/bin/env bash
# Validates the evaluation task set: every task in eval/tasks is well formed,
# and each active task's hidden checks pass its reference solution and catch
# its insecure one, run with the oskac just built. Offline: no model is
# called. scripts/check.sh runs this.
#
#   scripts/eval-tasks/run.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo build --quiet -p oskac -p oska-eval
bin="${CARGO_TARGET_DIR:-target}/debug"
if ! report=$("$bin/oska-eval" validate --oskac "$bin/oskac" eval/tasks); then
  printf '%s\n' "$report"
  echo "eval-tasks/run.sh: a task in eval/tasks does not validate (the report is above)" >&2
  exit 1
fi
echo "every task in eval/tasks validates"
