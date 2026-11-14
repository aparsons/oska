#!/usr/bin/env bash
# Tests for run.sh, the temp-leak wrapper. Each case runs the wrapper over a
# small command and checks only what it prints, its exit status and what is
# left in the temporary directory.
#
#   scripts/temp-leak/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
wrapper="$here/run.sh"

work=$(mktemp -d)
trap 'chmod -R u+rwx "$work" 2>/dev/null || true; rm -rf "$work"' EXIT

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

# A new empty directory to serve as the "real" temporary directory.
real_tmp() {
  mktemp -d "$work/real.XXXXXX"
}

# Runs the wrapper over `bash -c "$script"` with TMPDIR=$real; sets `out`
# (stdout and stderr) and `status`. The script sees the real directory as
# $REAL.
run() {
  local real=$1 script=$2
  set +e
  out=$(REAL="$real" TMPDIR="$real" "$wrapper" bash -c "$script" 2>&1)
  status=$?
  set -e
}

expect_status() { # name expected
  if [ "$status" -eq "$2" ]; then ok "$1"; else bad "$1" "exit status $status, expected $2 (output: $out)"; fi
}
expect_contains() { # name text
  case $out in *"$2"*) ok "$1" ;; *) bad "$1" "output lacks '$2' (output: $out)" ;; esac
}
expect_empty_output() { # name
  if [ -z "$out" ]; then ok "$1"; else bad "$1" "output not empty: $out"; fi
}
expect_nothing_left() { # name dir
  if [ -z "$(ls -A "$2")" ]; then ok "$1"; else bad "$1" "left behind: $(ls -A "$2")"; fi
}

real=$(real_tmp)
run "$real" 'echo hi >"$TMPDIR/scratch"; rm "$TMPDIR/scratch"'
expect_status "a command that cleans up exits 0" 0
expect_empty_output "a command that cleans up prints nothing"
expect_nothing_left "a command that cleans up leaves no private directory" "$real"

real=$(real_tmp)
run "$real" 'echo hi >"$TMPDIR/leaked-file"'
expect_status "a command that leaves a file fails the check" 1
expect_contains "the leaked file's name is printed" "leaked-file"
expect_nothing_left "a leaked file is removed" "$real"

real=$(real_tmp)
run "$real" 'mkdir -p "$TMPDIR/oskac-leaked-dir/sub"; echo hi >"$TMPDIR/oskac-leaked-dir/sub/x"'
expect_status "a command that leaves a directory fails the check" 1
expect_contains "the leaked directory's name is printed" "oskac-leaked-dir"
expect_nothing_left "a leaked directory is removed" "$real"

real=$(real_tmp)
run "$real" 'mkdir -p "$TMPDIR/ro/inner"; touch "$TMPDIR/ro/inner/x"; chmod 500 "$TMPDIR/ro/inner" "$TMPDIR/ro"'
expect_status "a leaked read-only directory fails the check" 1
expect_nothing_left "a leaked read-only directory is removed" "$real"

real=$(real_tmp)
run "$real" 'exit 7'
expect_status "a failing command keeps its exit status" 7
expect_nothing_left "a failing command leaves no private directory" "$real"

real=$(real_tmp)
run "$real" 'echo hi >"$TMPDIR/leaked-too"; exit 7'
expect_status "a failing command that also leaks keeps its exit status" 7
expect_contains "a failing command that also leaks has the leak reported" "leaked-too"
expect_nothing_left "a failing command that also leaks is cleaned up" "$real"

real=$(real_tmp)
run "$real" 'case $TMPDIR in "$REAL"/*) ;; *) exit 9 ;; esac; [ -d "$TMPDIR" ] && [ -z "$(ls -A "$TMPDIR")" ]'
expect_status "the command sees its own empty TMPDIR under the real one" 0
expect_nothing_left "the real temporary directory gains nothing" "$real"

real=$(real_tmp)
set +e
TMPDIR="$real" "$wrapper" bash -c 'echo hi >"$TMPDIR/x"; sleep 30' >"$work/interrupt.out" 2>&1 &
pid=$!
# Wait until the command has written into the private directory.
for _ in $(seq 1 50); do
  [ -n "$(find "$real" -type f 2>/dev/null)" ] && break
  sleep 0.1
done
kill -TERM "$pid"
wait "$pid"
status=$?
set -e
if [ "$status" -ne 0 ]; then ok "an interrupted run exits non-zero"; else bad "an interrupted run exits non-zero" "exit status 0"; fi
expect_nothing_left "an interrupted run removes its private directory" "$real"

printf '%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
