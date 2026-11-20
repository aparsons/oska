#!/usr/bin/env bash
# The merge gate for a ticket PR: every workflow the PR must trigger, and
# every job of it, has reported checks. A workflow GitHub cannot parse, or
# that never ran, reports nothing and the PR looks green (#329), so a missing
# workflow or job is a failure.
#
#   scripts/expected-workflows/check.sh PR
#
# Expected workflows and their jobs come from expected.py (every workflow
# with a `pull_request` trigger whose `branches`, `paths` and `paths-ignore`
# filters pass this PR), read from the working tree: run it from the PR's
# branch when the PR changes a workflow. Reported checks are those in
# `gh pr checks PR`. A job counts as reported when a check of its workflow
# has the job's name, or the job's name and a matrix suffix ` (...)`; a job
# skipped by its `if:` still reports. Run it once CI has had time to start;
# exit 0 if all reported, 1 naming each missing workflow and job.

set -euo pipefail

[ $# -eq 1 ] || { echo "usage: scripts/expected-workflows/check.sh PR" >&2; exit 2; }
pr=$1
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.."

base=$(gh pr view "$pr" --json baseRefName --jq .baseRefName)
files=$(gh pr diff "$pr" --name-only)
want=$(printf '%s\n' "$files" | python3 -I "$here/expected.py" --base "$base")
want_jobs=$(printf '%s\n' "$files" | python3 -I "$here/expected.py" --jobs --base "$base")
# One `WORKFLOW<TAB>CHECK NAME` row per check. gh pr checks exits non-zero (8)
# while checks are pending; its list is still valid.
have=$(gh pr checks "$pr" --json workflow,name --jq '.[] | "\(.workflow)\t\(.name)"' || true)
have_wf=$(printf '%s\n' "$have" | cut -f 1)

missing=0
while IFS= read -r wf; do
  [ -n "$wf" ] || continue
  if printf '%s\n' "$have_wf" | grep -qxF -- "$wf"; then
    echo "ok       $wf"
  else
    echo "MISSING  $wf: no checks reported for this PR"
    missing=1
  fi
done <<<"$want"
[ -n "$want" ] || echo "no workflow is expected for this PR"

while IFS=$'\t' read -r wf job; do
  [ -n "$wf" ] || continue
  # A missing workflow is already reported above.
  printf '%s\n' "$have_wf" | grep -qxF -- "$wf" || continue
  found=0
  while IFS=$'\t' read -r hwf hname; do
    [ "$hwf" = "$wf" ] || continue
    case "$hname" in
      "$job" | "$job ("*")") found=1 && break ;;
    esac
  done <<<"$have"
  if [ "$found" -eq 1 ]; then
    echo "ok       $wf / $job"
  else
    echo "MISSING  $wf / $job: no check reported for this job"
    missing=1
  fi
done <<<"$want_jobs"
exit "$missing"
