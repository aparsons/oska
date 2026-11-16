#!/usr/bin/env bash
# shellcheck disable=SC2016  # single quotes hold text on purpose: patterns, commands and scripts that must not expand here
# Tests for scripts/check.sh's step registry: `--list` prints the steps in
# the order a run prints their headers, so the list cannot drift from the
# steps that run. Each case runs the real check.sh in a scratch tree where
# every script it calls, cargo and shellcheck are stubs that log their name,
# so a whole run takes well under a second.
#
#   scripts/check-steps/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

ok() {
  printf 'ok   %s\n' "$1"
  passed=$((passed + 1))
}
bad() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures=$((failures + 1))
}

# The stub every called script and tool becomes: logs its directory/name
# (cargo: `cargo SUBCOMMAND`), and fails when that is STUB_FAIL. The
# temp-leak wrapper stub then runs the command it wraps.
stub='#!/usr/bin/env bash
name=$(basename "$(dirname "$0")")/$(basename "$0")
if [ "$name" = bin/cargo ]; then name="cargo $1"; fi
echo "$name" >>"$STUB_LOG"
if [ "$name" = "${STUB_FAIL:-}" ]; then exit 1; fi
if [ "$name" = pass-stamp/stamp.sh ] && [ "${1:-}" = tree ]; then echo tree; fi
if [ "$name" = temp-leak/run.sh ]; then exec "$@"; fi
exit 0
'

# The scratch tree: the real check.sh, a stub for every script it names, a
# build-slot stub that logs being sourced, a Cargo.lock with no outside crate
# and no workflows.
tree="$work/tree"
mkdir -p "$tree/scripts/build-slots" "$tree/.github/workflows" "$tree/bin"
cp "$repo_root/scripts/check.sh" "$tree/scripts/check.sh"
: >"$tree/Cargo.lock"
# shellcheck disable=SC2016 # expands when check.sh sources it
echo 'echo build-slots/acquire.sh >>"$STUB_LOG"' >"$tree/scripts/build-slots/acquire.sh"
grep -oE 'scripts/[A-Za-z0-9_-]+/[A-Za-z0-9_-]+\.sh' "$repo_root/scripts/check.sh" | sort -u |
  while read -r path; do
    [ "$path" = scripts/build-slots/acquire.sh ] && continue
    mkdir -p "$tree/$(dirname "$path")"
    printf '%s' "$stub" >"$tree/$path"
    chmod +x "$tree/$path"
  done
for tool in cargo shellcheck pwsh; do
  printf '%s' "$stub" >"$tree/bin/$tool"
  chmod +x "$tree/bin/$tool"
done

# check ARGS...: runs the scratch check.sh. Sets `status`, `out` (stdout and
# stderr) and `log` (what the stubs logged, one name per line).
check() {
  : >"$work/log"
  set +e
  out=$(cd "$tree" && PATH="$tree/bin:$PATH" STUB_LOG="$work/log" scripts/check.sh "$@" 2>&1)
  status=$?
  set -e
  log=$(cat "$work/log")
}

# --- --list ------------------------------------------------------------------
check --list
list=$out
if [ "$status" -eq 0 ]; then ok "--list exits 0"; else bad "--list exits 0" "exit $status: $out"; fi
if [ -z "$log" ]; then ok "--list runs no step"; else bad "--list runs no step" "ran: $log"; fi
first=$(printf '%s\n' "$list" | head -n 1)
if [ "$first" = "no external crates" ]; then
  ok "--list starts with the first step"
else
  bad "--list starts with the first step" "first line: $first"
fi
order=$(printf '%s\n' "$list" | grep -xF -e "cargo fmt" -e "cargo clippy" -e "pass stamp tests" | tr '\n' ,)
if [ "$order" = "cargo fmt,cargo clippy,pass stamp tests," ]; then
  ok "--list prints the steps in order"
else
  bad "--list prints the steps in order" "got: $list"
fi
repeated=$(printf '%s\n' "$list" | sort | uniq -d)
if [ -z "$repeated" ]; then ok "--list names each step once"; else bad "--list names each step once" "repeated: $repeated"; fi
if printf '%s\n' "$list" | grep -qE '^== |All checks passed'; then
  bad "--list prints only step names" "got: $list"
else
  ok "--list prints only step names"
fi
if printf '%s\n' "$log" | grep -qx build-slots/acquire.sh; then
  bad "--list takes no build slot" "log: $log"
else
  ok "--list takes no build slot"
fi

# --- a passing run prints the listed steps, in order, and stamps last -------
check
headers=$(printf '%s\n' "$out" | sed -n 's/^== //p')
if [ "$status" -eq 0 ]; then ok "a passing run exits 0"; else bad "a passing run exits 0" "exit $status: $out"; fi
if [ "$headers" = "$list" ]; then
  ok "a run's step headers are exactly --list"
else
  bad "a run's step headers are exactly --list" "$(diff <(printf '%s\n' "$list") <(printf '%s\n' "$headers") || true)"
fi
if [ "$(printf '%s\n' "$out" | tail -n 1)" = "All checks passed." ]; then
  ok "a passing run ends with All checks passed."
else
  bad "a passing run ends with All checks passed." "got: $out"
fi
if [ "$(printf '%s\n' "$log" | grep -cx pass-stamp/stamp.sh)" -eq 2 ] &&
  [ "$(printf '%s\n' "$log" | tail -n 1)" = pass-stamp/stamp.sh ]; then
  ok "a passing run writes the pass stamp last"
else
  bad "a passing run writes the pass stamp last" "log: $log"
fi
slot_and_neighbours=$(printf '%s\n' "$log" | grep -x -B1 -A1 build-slots/acquire.sh | tr '\n' ,)
if [ "$slot_and_neighbours" = "workflow-guard/guard.sh,build-slots/acquire.sh,cargo fmt," ]; then
  ok "a run takes the build slot once, after the cheap guards and before cargo fmt"
else
  bad "a run takes the build slot once, after the cheap guards and before cargo fmt" "log: $log"
fi

# Discovery: every script test suite in the tree, and cargo's fmt, clippy and
# test, run in a full run. A dropped step fails here.
missing=
for suite in "$repo_root"/scripts/*/test.sh; do
  name=$(basename "$(dirname "$suite")")/test.sh
  printf '%s\n' "$log" | grep -qxF "$name" || missing="$missing $name"
done
for name in "cargo fmt" "cargo clippy" "cargo test"; do
  printf '%s\n' "$log" | grep -qxF "$name" || missing="$missing '$name'"
done
if [ -z "$missing" ]; then
  ok "a run runs every scripts/*/test.sh and cargo fmt, clippy and test"
else
  bad "a run runs every scripts/*/test.sh and cargo fmt, clippy and test" "not run:$missing"
fi

# --- a failing step stops the run, non-zero, with no stamp ------------------
STUB_FAIL=install/test.sh check
if [ "$status" -eq 1 ]; then ok "a failing step exits 1"; else bad "a failing step exits 1" "exit $status"; fi
last=$(printf '%s\n' "$out" | sed -n 's/^== //p' | tail -n 1)
if [ "$last" = "installer tests" ]; then
  ok "a failing step is the last step run"
else
  bad "a failing step is the last step run" "last header: $last"
fi
if [ "$(printf '%s\n' "$log" | grep -cx pass-stamp/stamp.sh)" -eq 1 ]; then
  ok "a failing run does not write the stamp"
else
  bad "a failing run does not write the stamp" "log: $log"
fi

# --- an unknown argument is a usage error -----------------------------------
check --bogus
if [ "$status" -eq 2 ] && [ -z "$log" ]; then
  ok "an unknown argument exits 2 and runs nothing"
else
  bad "an unknown argument exits 2 and runs nothing" "exit $status, ran: $log"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
