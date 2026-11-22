#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Proves the workflow validator rejects the #329 bug and accepts good files.
# Needs actionlint (see run.sh): skips without it locally, fails in CI.

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

cat >"$work/good.yml" <<'YML'
name: good
on:
  workflow_dispatch:
    inputs:
      crate:
        description: "a (b, _ : c)"
        required: false
jobs:
  j:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    steps:
      - run: echo hi
YML
# The first call also finds out whether actionlint is available (exit 3).
# The repo's own workflows are checked by check.sh's "workflow validation".
rc=0
bash scripts/actionlint/run.sh "$work/good.yml" >"$work/good.out" 2>&1 || rc=$?
if [ "$rc" -eq 3 ]; then
  if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
    cat "$work/good.out"
    echo "FAIL actionlint unavailable in CI"
    exit 1
  fi
  echo "skip: actionlint unavailable ($(cat "$work/good.out"))"
  exit 0
fi
if [ "$rc" -eq 0 ]; then ok "a valid workflow passes"; else cat "$work/good.out"; bad "a valid workflow passes"; fi

# The #329 bug: an unquoted ": " inside an input description.
sed 's/description: "a (b, _ : c)"/description: a (b, _ : c)/' "$work/good.yml" >"$work/bad.yml"
grep -q 'description: a (b, _ : c)' "$work/bad.yml" || bad "fixture has the unquoted description"
rc=0
bash scripts/actionlint/run.sh "$work/bad.yml" >"$work/bad.out" 2>&1 || rc=$?
if [ "$rc" -eq 1 ]; then
  ok "an unquoted ': ' description fails"
else
  cat "$work/bad.out"
  bad "an unquoted ': ' description fails (exit $rc)"
fi

printf 'on: [push\njobs: {\n' >"$work/broken.yml"
rc=0
bash scripts/actionlint/run.sh "$work/broken.yml" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 1 ]; then ok "invalid YAML fails"; else bad "invalid YAML fails (exit $rc)"; fi

exit "$fail"
