#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for allowlist.sh: allowed actions pass pinned, and everything else fails.
#
#   scripts/workflow-guard/allowlist-test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sha=0123456789abcdef0123456789abcdef01234567
failures=0

# check NAME WANT USES: a workflow with one `uses:` line; WANT is pass or refuse.
check() {
  local dir
  dir=$(mktemp -d "$work/t.XXXXXX")
  mkdir -p "$dir/.github/workflows"
  printf 'jobs:\n  a:\n    steps:\n      - uses: %s\n' "$3" >"$dir/.github/workflows/w.yml"
  local got=pass
  "$here/allowlist.sh" "$dir" >/dev/null 2>&1 || got=refuse
  if [ "$got" != "$2" ]; then
    printf 'FAIL %s: wanted %s, got %s\n' "$1" "$2" "$got" >&2
    failures=$((failures + 1))
  fi
}

check "official action pinned" pass "actions/checkout@$sha # v4"
check "scorecard action pinned" pass "ossf/scorecard-action@$sha # v2.6.0"
check "sarif upload pinned" pass "github/codeql-action/upload-sarif@$sha # v4.39.4"
check "scorecard action by tag" refuse "ossf/scorecard-action@v2.6.0"
check "scorecard action by short sha" refuse "ossf/scorecard-action@0123456"
check "sarif upload by branch" refuse "github/codeql-action/upload-sarif@main"
check "other ossf action" refuse "ossf/other-action@$sha # v1"
check "other codeql action" refuse "github/codeql-action/analyze@$sha # v4"
check "codeql init" refuse "github/codeql-action@$sha # v4"
check "another third party" refuse "someone/action@$sha # v1"
check "official action by tag" refuse "actions/checkout@v4"

real=$(cd "$here/../.." && pwd)
"$here/allowlist.sh" "$real" || { echo "FAIL the real workflows" >&2; failures=$((failures + 1)); }

printf '%s failed\n' "$failures"
[ "$failures" -eq 0 ]
