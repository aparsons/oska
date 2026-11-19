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
#    as cargo runs them. The doc tests run as one more job, for the chosen
#    packages that have a library (`cargo test --doc` refuses a package
#    without one): ARGS without the package choice, then `-p` per package.
# 3. Prints one line as each program ends, then every program's output in
#    job order, so outputs do not mix. Every program runs even when one
#    fails; the end names each failure, and the exit status is 1 if any
#    failed.
#
# A test program must not depend on another one running before or after it:
# cargo gives no order either. ARGS must not hold `--`: arguments for the
# test programs themselves are not supported. A build that yields no test
# program fails the run, so a change in cargo's messages cannot pass as an
# empty run.

set -euo pipefail

cargo=${CARGO:-cargo}
jobs=${OSKA_TEST_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}
case $jobs in
  '' | *[!0-9]* | 0)
    echo "parallel-test: OSKA_TEST_JOBS must be a positive number, not '$jobs'" >&2
    exit 2
    ;;
esac
for arg in "$@"; do
  if [ "$arg" = -- ]; then
    echo "parallel-test: arguments for the test programs (after --) are not supported" >&2
    exit 2
  fi
done

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-parallel-test.XXXXXX")
trap 'rm -rf "$work"' EXIT

# 1. Build. Cargo's own messages (compiling, errors) still go to stderr.
if ! "$cargo" test "$@" --no-run --message-format=json-render-diagnostics >"$work/build.json"; then
  echo "parallel-test: cargo test --no-run failed" >&2
  exit 1
fi
# "PROG<TAB>DIR<TAB>PROGRAM" per test program, then "DOC<TAB>PACKAGE" per
# chosen package with a library.
python3 -I - "$work/build.json" >"$work/found" <<'PY'
import json, os, sys
programs, docs = [], []
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line.startswith("{"):
        continue
    msg = json.loads(line)
    if msg.get("reason") != "compiler-artifact" or not msg.get("profile", {}).get("test"):
        continue
    exe = msg.get("executable")
    if not exe:
        continue
    programs.append(os.path.dirname(msg["manifest_path"]) + "\t" + exe)
    if set(msg.get("target", {}).get("kind", [])) & {"lib", "rlib", "dylib", "proc-macro"}:
        # path+file:///a/b/name#1.0.0, or path+file:///a/b/dir#name@1.0.0
        url, _, fragment = msg["package_id"].partition("#")
        name = fragment.split("@")[0] if "@" in fragment else url.rstrip("/").rsplit("/", 1)[-1]
        if name not in docs:
            docs.append(name)
for p in programs:
    print("PROG\t" + p)
for d in docs:
    print("DOC\t" + d)
PY
grep '^PROG' "$work/found" | cut -f2- >"$work/programs" || true
count=$(wc -l <"$work/programs" | tr -d ' ')
if [ "$count" -eq 0 ]; then
  echo "parallel-test: cargo built no test program; nothing was tested" >&2
  exit 1
fi

# The doc tests' arguments, NUL-separated: ARGS without the package choice,
# --doc, then -p for each package with a library.
: >"$work/doc-args"
doc_job=
if grep -q '^DOC' "$work/found"; then
  doc_job=1
  {
    skip=
    for arg in "$@"; do
      if [ -n "$skip" ]; then
        skip=
        continue
      fi
      case $arg in
        -p | --package) skip=1 ;;
        --package=* | --all | --workspace) ;;
        *) printf '%s\0' "$arg" ;;
      esac
    done
    printf '%s\0' --doc
    grep '^DOC' "$work/found" | cut -f2 | while IFS= read -r package; do
      printf '%s\0' -p "$package"
    done
  } >"$work/doc-args"
  echo "parallel-test: $count test programs and the doc tests, $jobs at a time"
else
  echo "parallel-test: $count test programs (no library, so no doc tests), $jobs at a time"
fi

# 2. One job per line: "N<TAB>DIR<TAB>PROGRAM", or "N<TAB>DOC" for the doc
# tests. N numbers the output file, so outputs print in job order.
{
  cat "$work/programs"
  if [ -n "$doc_job" ]; then echo DOC; fi
} | awk '{ printf "%04d\t%s\n", NR, $0 }' >"$work/jobs"
mkdir "$work/out"

# run_one N: runs job N with its whole output in a file, then prints one
# short line. A failure is recorded as a file in $PARALLEL_WORK/failed.N.
# xargs passes only N: BSD xargs limits a replaced argument to 255 bytes.
run_one() {
  local n=$1 rest dir exe name out status=0 doc_args=()
  rest=$(grep "^$n"$'\t' "$PARALLEL_WORK/jobs" | cut -f2-)
  out="$PARALLEL_WORK/out/$n"
  if [ "$rest" = DOC ]; then
    name="doc tests"
    while IFS= read -r -d '' a; do doc_args+=("$a"); done <"$PARALLEL_WORK/doc-args"
    printf '     Running %s\n' "$name" >"$out"
    "$PARALLEL_CARGO" test "${doc_args[@]}" >>"$out" 2>&1 || status=$?
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
export -f run_one

# shellcheck disable=SC2016 # the script runs in the job's own bash
cut -f1 "$work/jobs" | xargs -P "$jobs" -I{} bash -c 'run_one "$1"' _ {}

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
if [ -n "$doc_job" ]; then
  echo "parallel-test: all $count test programs and the doc tests passed"
else
  echo "parallel-test: all $count test programs passed"
fi
