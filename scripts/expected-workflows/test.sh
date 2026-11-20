#!/usr/bin/env bash
# Tests expected.py (filter logic) and check.sh (against a stubbed gh).

set -euo pipefail
cd "$(dirname "$0")/../.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ok() { printf 'ok   %s\n' "$1"; }
bad() {
  printf 'FAIL %s\n' "$1"
  fail=1
}

wf="$work/wf"
mkdir -p "$wf"
printf '%s\n' 'name: Always' 'on:' '  push:' '    branches: [main]' '  pull_request:' 'jobs: {}' >"$wf/always.yml"
printf '%s\n' 'name: Filtered' 'on:' '  pull_request:' '    paths:' '      - crates/**' '      - "!crates/docs-only/**"' '      - Cargo.toml' 'jobs: {}' >"$wf/filtered.yml"
printf '%s\n' 'name: Ignore' 'on:' '  pull_request:' '    paths-ignore:' '      - docs/**' 'jobs: {}' >"$wf/ignore.yml"
printf '%s\n' 'name: Branches' 'on:' '  pull_request:' '    branches: [release/**]' 'jobs: {}' >"$wf/branches.yml"
printf '%s\n' 'name: PushOnly' 'on:' '  push:' '  workflow_dispatch:' 'jobs: {}' >"$wf/pushonly.yml"
printf '%s\n' 'on: [pull_request]' 'jobs: {}' >"$wf/noname.yml"

# exp BASE FILES...: the expected workflows, space-separated.
exp() {
  local base=$1
  shift
  printf '%s\n' "$@" | python3 -I scripts/expected-workflows/expected.py --base "$base" --dir "$wf" | tr '\n' ' '
}
expect() {
  if [ "$3" = "$2" ]; then ok "$1"; else bad "$1: want [$2] got [$3]"; fi
}

expect "a docs-only change" "Always Ignore noname.yml " "$(exp main notes.md)"
expect "a change under a paths filter" "Always Filtered Ignore noname.yml " "$(exp main crates/a/x.rs docs/y.md)"
expect "a negated path does not match" "Always Ignore noname.yml " "$(exp main crates/docs-only/x.md)"
expect "paths-ignore skips a docs-only change" "Always noname.yml " "$(exp main docs/a.md)"
expect "a branches filter follows the base" "Always Branches Ignore noname.yml " "$(exp release/1 x.txt)"

# The repo's own workflows parse, and a crates change expects CI.
real=$(printf 'crates/oskac/src/main.rs\n' | python3 -I scripts/expected-workflows/expected.py --base main)
case "$real" in
  *CI*) ok "the repo's workflows parse; a crates change expects CI" ;;
  *) bad "the repo's workflows parse: got [$real]" ;;
esac

# Jobs: each job of an expected workflow must report a check too. A job is
# named by its `name:`, else its id.
printf '%s\n' 'name: Jobs' 'on:' '  pull_request:' 'jobs:' '  build:' '    strategy:' \
  '      matrix:' '        os: [a, b]' '    runs-on: x' '  lint:' '    name: Lint all' '    runs-on: x' >"$wf/jobs.yml"
jobs=$(printf 'x\n' | python3 -I scripts/expected-workflows/expected.py --jobs --base main --dir "$wf" | grep '^Jobs' | tr '\t\n' ': ')
expect "--jobs lists each job of an expected workflow" "Jobs:build Jobs:Lint all " "$jobs"

# The repo's CI expects its musl job (ticket #462).
tab=$(printf '\t')
real=$(printf 'crates/oskac/src/main.rs\n' | python3 -I scripts/expected-workflows/expected.py --jobs --base main)
if printf '%s\n' "$real" | grep -qxF "CI${tab}musl"; then
  ok "a crates change expects CI's musl job"
else
  bad "a crates change expects CI's musl job: got [$real]"
fi

# check.sh against a stub gh, reading the repo's real workflows. The stub's
# `pr checks` prints REPORTED: one `WORKFLOW<TAB>CHECK NAME` row per check.
mkdir -p "$work/bin"
# shellcheck disable=SC2016 # the stub gh expands its variables when it runs
printf '%s\n' '#!/usr/bin/env bash' \
  'case "$1 $2" in' \
  '  "pr view") echo main ;;' \
  '  "pr diff") echo crates/a/x.rs ;;' \
  '  "pr checks") printf "%s\n" "$REPORTED" ;;' \
  'esac' 'exit 0' >"$work/bin/gh"
chmod +x "$work/bin/gh"
# Every job reports, a matrix job as `NAME (VALUES)`, as GitHub names it.
all=$(printf 'crates/a/x.rs\n' | python3 -I scripts/expected-workflows/expected.py --jobs --base main | sed 's/$/ (x)/')
rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$all" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 0 ]; then ok "check.sh passes when every expected workflow and job reported"; else bad "check.sh passes: $out"; fi

first=$(printf '%s\n' "$all" | head -n 1 | cut -f 1)
rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$(printf '%s\n' "$all" | grep -v "^$first$tab")" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 1 ] && printf '%s\n' "$out" | grep -qF "MISSING  $first:"; then
  ok "check.sh fails and names a workflow with no checks"
else
  bad "check.sh fails on a missing workflow (exit $rc): $out"
fi

rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$(printf '%s\n' "$all" | grep -vxF "CI${tab}musl (x)")" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 1 ] && printf '%s\n' "$out" | grep -qF "MISSING  CI / musl"; then
  ok "check.sh fails and names a job with no checks"
else
  bad "check.sh fails on a missing job (exit $rc): $out"
fi

# A check whose name only starts with the job's name is another job.
rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$(printf '%s\n' "$all" | sed "s/^CI${tab}musl (x)\$/CI${tab}muslx/")" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 1 ] && printf '%s\n' "$out" | grep -qF "MISSING  CI / musl"; then
  ok "check.sh does not count another job with the same prefix"
else
  bad "check.sh counts a prefix as the job (exit $rc): $out"
fi

exit "$fail"
