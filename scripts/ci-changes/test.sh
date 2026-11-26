#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for scripts/ci-changes/docs-only.sh: only files no test reads count
# as docs-only, and an empty list or any other file means the tests run.
#
#   scripts/ci-changes/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/docs-only.sh"

passed=0
failures=0
ok() { printf 'ok   %s\n' "$1"; passed=$((passed + 1)); }
bad() { printf 'FAIL %s\n' "$1"; failures=$((failures + 1)); }

# expect NAME WANT FILES...
expect() {
  local name=$1 want=$2 got
  shift 2
  got=$(printf '%s\n' "$@" | "$script")
  if [ "$got" = "docs_only=$want" ]; then ok "$name"; else bad "$name: want $want, got $got"; fi
}

expect "an agent note is docs-only" true docs/agents/implementer.md
expect "several agent notes and a standards file are docs-only" true docs/agents/rust.md CODING_STANDARDS.md
expect "a guide is not docs-only (tests run its programs)" false docs/guide/cli-reference.md
expect "an ADR is not docs-only (the real-site tests build it)" false docs/adr/0001-a.md
expect "the checker doc is not docs-only (tests read it)" false docs/checker.md
expect "a root readme is not docs-only" false README.md
expect "the contributing file is not docs-only (a test reads it)" false CONTRIBUTING.md
expect "a site file is not docs-only" false site/style.css
expect "a source file is not docs-only" false crates/oskac/src/main.rs
expect "a workflow is not docs-only" false .github/workflows/ci.yml
expect "an agent note beside a source file is not docs-only" false docs/agents/rust.md crates/oskac/src/main.rs
expect "a path that only starts like docs/agents is not docs-only" false docs/agents-extra/x.md

if [ "$(printf '' | "$script")" = "docs_only=false" ]; then
  ok "no changed file is not docs-only"
else
  bad "no changed file must not be docs-only"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
