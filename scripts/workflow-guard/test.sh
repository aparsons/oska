#!/usr/bin/env bash
# Tests for guard.sh: each case builds a scratch tree with workflow files under
# .github/workflows and expects the guard to pass it, or to refuse it with a
# message naming the file and the job.
#
#   scripts/workflow-guard/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

# An empty tree in a new directory, which it prints.
tree() {
  local dir
  dir=$(mktemp -d "$work/tree.XXXXXX")
  mkdir -p "$dir/.github/workflows"
  printf '%s\n' "$dir"
}

# Write a workflow: workflow DIR NAME, the YAML on stdin.
workflow() {
  cat >"$1/.github/workflows/$2"
}

expect_pass() {
  local name=$1 dir=$2 out
  if out=$("$guard" "$dir" 2>&1); then
    passed=$((passed + 1))
  else
    printf 'FAIL %s: the guard refused\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  fi
}

# Expect the guard to refuse `dir` with `message` (a fixed string) in its output.
expect_refused() {
  local name=$1 dir=$2 message=$3 out
  if out=$("$guard" "$dir" 2>&1); then
    printf 'FAIL %s: the guard passed\n' "$name" >&2
    failures=$((failures + 1))
  elif ! grep -qF -- "$message" <<<"$out"; then
    printf 'FAIL %s: expected "%s" in\n%s\n' "$name" "$message" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - run: true
YAML
expect_pass "a bounded job" "$dir"

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: true
YAML
expect_refused "a job with no timeout" "$dir" ".github/workflows/ci.yml: test: "

dir=$(tree)
workflow "$dir" multi.yml <<'YAML'
name: Multi
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - run: true
  # A comment between jobs.
  assemble:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - run: true
  publish:
    needs: assemble
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: true
YAML
expect_refused "one of several jobs unbounded names that job" "$dir" ".github/workflows/multi.yml: assemble: "
out=$("$guard" "$dir" 2>&1 || true)
if grep -qE '(build|publish): ' <<<"$out"; then
  printf 'FAIL the bounded jobs of a multi-job file were reported\n%s\n' "$out" >&2
  failures=$((failures + 1))
else
  passed=$((passed + 1))
fi

# A value that is not a whole number from 1 to 60.
for bad in 0 61 -5 '${{ matrix.minutes }}' thirty 2.5 '"30"' 030 360; do
  dir=$(tree)
  workflow "$dir" ci.yml <<YAML
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: $bad
    steps:
      - run: true
YAML
  expect_refused "timeout-minutes: $bad" "$dir" ".github/workflows/ci.yml: test: "
done

for good in 1 60 '30 # a comment'; do
  dir=$(tree)
  workflow "$dir" ci.yml <<YAML
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: $good
    steps:
      - run: true
YAML
  expect_pass "timeout-minutes: $good" "$dir"
done

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: true
        timeout-minutes: 5
YAML
expect_refused "the key nested in a step is not job level" "$dir" ".github/workflows/ci.yml: test: "

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          echo "timeout-minutes: 5"
    timeout-minutes: 15
YAML
expect_pass "the key after the steps still counts" "$dir"

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          echo "text"
          timeout-minutes: 5
YAML
expect_refused "the key inside a script is not job level" "$dir" ".github/workflows/ci.yml: test: "

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: true
  timeout-minutes: 5
YAML
expect_refused "the key at the jobs indent is not in a job" "$dir" ".github/workflows/ci.yml: test: "

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
timeout-minutes: 5
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: true
YAML
expect_refused "the key at the top level is not in a job" "$dir" ".github/workflows/ci.yml: test: "

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  call:
    uses: ./.github/workflows/other.yml
    timeout-minutes: 10
YAML
expect_refused "a reusable-workflow job" "$dir" ".github/workflows/ci.yml: call: calls a reusable workflow"

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
    timeout-minutes: 10
YAML
expect_pass "a step that uses an action is not a reusable workflow" "$dir"

dir=$(tree)
workflow "$dir" ci.yml <<'YAML'
name: CI
on: push
YAML
expect_refused "a workflow with no jobs" "$dir" ".github/workflows/ci.yml: -: has no jobs"

dir=$(tree)
workflow "$dir" good.yml <<'YAML'
name: Good
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: true
YAML
workflow "$dir" bad.yaml <<'YAML'
name: Bad
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: true
YAML
expect_refused "one bad file among good ones, with the .yaml extension" "$dir" ".github/workflows/bad.yaml: test: "

dir=$(tree)
workflow "$dir" crlf.yml <<'YAML'
name: CRLF
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: true
YAML
sed -i 's/$/\r/' "$dir/.github/workflows/crlf.yml"
expect_pass "Windows line endings" "$dir"

dir=$(mktemp -d "$work/tree.XXXXXX")
expect_refused "a tree with no workflows" "$dir" "no workflow files"

# The real workflows pass.
real=$(cd "$here/../.." && pwd)
expect_pass "the real workflows" "$real"

printf '%s passed, %s failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
