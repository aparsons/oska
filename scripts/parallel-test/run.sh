#!/usr/bin/env bash
# Runs `cargo test ARGS...` with several test programs at a time. Plain
# `cargo test` runs its test programs one after another; each program runs
# its own tests in parallel, but one slow program holds up all the rest.
# check.sh runs the workspace tests through this (inside scripts/temp-leak).
#
#   scripts/parallel-test/run.sh [CARGO TEST ARGS...]
#
# 1. Builds every test program with `cargo test ARGS --no-run`, and reads
#    the programs from cargo's JSON messages.
# 2. Runs them OSKA_TEST_JOBS at a time (default: the number of processors),
#    each in its package's directory with CARGO and CARGO_MANIFEST_DIR set,
#    as cargo runs them. The doc tests (`cargo test ARGS --doc`) run as one
#    more job.
# 3. Prints one line as each program ends, then every program's output in
#    job order, so outputs do not mix. Every program runs even when one
#    fails; the end names each failure, and the exit status is 1 if any
#    failed.
#
# A test program must not depend on another one running before or after it:
# cargo gives no order either.

set -euo pipefail

cargo=${CARGO:-cargo}
jobs=${OSKA_TEST_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}
case $jobs in
  '' | *[!0-9]* | 0)
    echo "parallel-test: OSKA_TEST_JOBS must be a positive number, not '$jobs'" >&2
    exit 2
    ;;
esac

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-parallel-test.XXXXXX")
trap 'rm -rf "$work"' EXIT

# 1. Build. Cargo's own messages (compiling, errors) still go to stderr.
if ! "$cargo" test "$@" --no-run --message-format=json-render-diagnostics >"$work/build.json"; then
  echo "parallel-test: cargo test --no-run failed" >&2
  exit 1
fi
python3 -I - "$work/build.json" >"$work/programs" <<'PY'
import json, os, sys
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line.startswith("{"):
        continue
    msg = json.loads(line)
    if msg.get("reason") != "compiler-artifact" or not msg.get("profile", {}).get("test"):
        continue
    exe = msg.get("executable")
    if exe:
        print(os.path.dirname(msg["manifest_path"]) + "\t" + exe)
PY
count=$(wc -l <"$work/programs" | tr -d ' ')
echo "parallel-test: $count test programs and the doc tests, $jobs at a time"

# 2. One job per line: "N<TAB>DIR<TAB>PROGRAM", or "N<TAB>DOC" for the doc
# tests. N numbers the output file, so outputs print in job order.
{
  cat "$work/programs"
  echo DOC
} | awk '{ printf "%04d\t%s\n", NR, $0 }' >"$work/jobs"
mkdir "$work/out"

# Runs one job with its whole output in a file, then prints one short line.
# A failure is recorded as a file in $PARALLEL_WORK/failed.
run_one() {
  local line=$1 n rest dir exe name out status=0
  n=${line%%$'\t'*}
  rest=${line#*$'\t'}
  out="$PARALLEL_WORK/out/$n"
  if [ "$rest" = DOC ]; then
    name="doc tests"
    printf '     Running %s\n' "$name" >"$out"
    # shellcheck disable=SC2153 # set by the job's bash -c below
    "$PARALLEL_CARGO" test ${PARALLEL_ARGS[@]+"${PARALLEL_ARGS[@]}"} --doc >>"$out" 2>&1 || status=$?
  else
    dir=${rest%%$'\t'*}
    exe=${rest#*$'\t'}
    name="${dir##*/} $(basename "$exe")"
    printf '     Running %s\n' "$name" >"$out"
    (cd "$dir" && CARGO=$PARALLEL_CARGO CARGO_MANIFEST_DIR=$dir "$exe") >>"$out" 2>&1 || status=$?
  fi
  if [ "$status" -eq 0 ]; then
    printf 'parallel-test: ok     %s\n' "$name"
  else
    printf 'parallel-test: FAILED %s (exit %s)\n' "$name" "$status"
    printf '%s (exit %s)\n' "$name" "$status" >"$PARALLEL_WORK/failed.$n"
  fi
  return 0
}

export PARALLEL_WORK=$work
PARALLEL_CARGO=$(command -v "$cargo")
export PARALLEL_CARGO
# Bash cannot export an array; each job reads the arguments from a file.
: >"$work/args"
if [ "$#" -gt 0 ]; then printf '%s\0' "$@" >"$work/args"; fi
export -f run_one

# shellcheck disable=SC2016 # the script runs in the job's own bash
tr '\n' '\0' <"$work/jobs" | xargs -0 -P "$jobs" -I{} bash -c '
  set -uo pipefail
  PARALLEL_ARGS=()
  while IFS= read -r -d "" a; do PARALLEL_ARGS+=("$a"); done <"$PARALLEL_WORK/args"
  run_one "$1"
' _ {}

# Every program's output, in job order.
echo
cat "$work"/out/*

# 3. Summary.
failed=$(cat "$work"/failed.* 2>/dev/null || true)
if [ -n "$failed" ]; then
  echo
  echo "parallel-test: these test programs failed:"
  printf '%s\n' "$failed" | sed 's/^/  /'
  exit 1
fi
echo
echo "parallel-test: all $count test programs and the doc tests passed"
