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

# check.sh against a stub gh, reading the repo's real workflows.
mkdir -p "$work/bin"
printf '%s\n' '#!/usr/bin/env bash' \
  'case "$1 $2" in' \
  '  "pr view") echo main ;;' \
  '  "pr diff") echo crates/a/x.rs ;;' \
  '  "pr checks") printf "%s\n" "$REPORTED" ;;' \
  'esac' 'exit 0' >"$work/bin/gh"
chmod +x "$work/bin/gh"
all=$(printf 'crates/a/x.rs\n' | python3 -I scripts/expected-workflows/expected.py --base main)
rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$all" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 0 ]; then ok "check.sh passes when every expected workflow reported"; else bad "check.sh passes: $out"; fi
first=$(printf '%s\n' "$all" | head -n 1)
rest=$(printf '%s\n' "$all" | tail -n +2)
rc=0
out=$(PATH="$work/bin:$PATH" REPORTED="$rest" bash scripts/expected-workflows/check.sh 1 2>&1) || rc=$?
if [ "$rc" -eq 1 ] && printf '%s\n' "$out" | grep -qF "MISSING  $first"; then
  ok "check.sh fails and names a workflow with no checks"
else
  bad "check.sh fails on a missing workflow (exit $rc): $out"
fi

exit "$fail"
