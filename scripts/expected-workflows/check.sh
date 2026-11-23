#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# The merge gate for a ticket PR: every workflow the PR must trigger has
# reported checks. A workflow GitHub cannot parse, or that never ran, reports
# nothing and the PR looks green (#329), so a missing workflow is a failure.
#
#   scripts/expected-workflows/check.sh PR
#
# Expected workflows come from expected.py (every workflow with a
# `pull_request` trigger whose `branches`, `paths` and `paths-ignore` filters
# pass this PR), read from the working tree: run it from the PR's branch when
# the PR changes a workflow. Reported workflows are those in
# `gh pr checks PR --json workflow`. Run it once CI has had time to start;
# exit 0 if all reported, 1 naming each missing workflow.

set -euo pipefail

[ $# -eq 1 ] || { echo "usage: scripts/expected-workflows/check.sh PR" >&2; exit 2; }
pr=$1
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.."

base=$(gh pr view "$pr" --json baseRefName --jq .baseRefName)
files=$(gh pr diff "$pr" --name-only)
want=$(printf '%s\n' "$files" | python3 -I "$here/expected.py" --base "$base")
# gh pr checks exits non-zero (8) while checks are pending; its list is still valid.
have=$(gh pr checks "$pr" --json workflow --jq '.[].workflow' || true)

missing=0
while IFS= read -r wf; do
  [ -n "$wf" ] || continue
  if printf '%s\n' "$have" | grep -qxF -- "$wf"; then
    echo "ok       $wf"
  else
    echo "MISSING  $wf: no checks reported for this PR"
    missing=1
  fi
done <<<"$want"
[ -n "$want" ] || echo "no workflow is expected for this PR"
exit "$missing"
