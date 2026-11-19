#!/usr/bin/env bash
# shellcheck disable=SC2016  # single quotes hold scripts that are written out, not expanded here
# Tests for run.sh, the parallel test runner. A stub cargo stands in for the
# real one: `cargo test ... --no-run` prints the JSON messages for stub test
# programs, and `cargo test ... --doc` stands for the doc tests. Each case
# checks only what run.sh prints, its exit status, and what the programs saw.
#
#   scripts/parallel-test/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
runner="$here/run.sh"

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

# The stub cargo. It logs each call's arguments, one call per line, to
# $STUB_DIR/cargo.log. The build lists every program in $STUB_DIR/programs
# ("PACKAGE PROGRAM" per line); it fails when $STUB_DIR/build-fails exists.
# The doc tests fail when $STUB_DIR/doc-fails exists.
mkdir -p "$work/bin"
cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '[%s]' "$@" >>"$STUB_DIR/cargo.log"
echo >>"$STUB_DIR/cargo.log"
for a in "$@"; do
  if [ "$a" = --no-run ]; then
    [ -e "$STUB_DIR/build-fails" ] && { echo "error: does not compile" >&2; exit 101; }
    echo "not JSON: a build script's line"
    while read -r package program; do
      printf '{"reason":"compiler-artifact","manifest_path":"%s/Cargo.toml","profile":{"test":true},"executable":"%s"}\n' \
        "$STUB_DIR/crates/$package" "$STUB_DIR/programs.d/$program"
      # A library that is not a test program, and a test-profile artifact with no executable.
      printf '{"reason":"compiler-artifact","manifest_path":"%s/Cargo.toml","profile":{"test":false},"executable":null}\n' \
        "$STUB_DIR/crates/$package"
    done <"$STUB_DIR/programs"
    printf '{"reason":"build-finished","success":true}\n'
    exit 0
  fi
  if [ "$a" = --doc ]; then
    echo "doc tests ran"
    [ -e "$STUB_DIR/doc-fails" ] && exit 101
    exit 0
  fi
done
echo "stub cargo: unexpected call" >&2
exit 3
EOF
chmod +x "$work/bin/cargo"

# setup NAME: a fresh stub directory for one case, with no programs yet.
setup() {
  STUB_DIR="$work/$1"
  mkdir -p "$STUB_DIR/programs.d" "$STUB_DIR/crates"
  : >"$STUB_DIR/programs"
  export STUB_DIR
}
# program PACKAGE NAME BODY: a stub test program that runs BODY.
program() {
  mkdir -p "$STUB_DIR/crates/$1"
  printf '#!/usr/bin/env bash\n%s\n' "$3" >"$STUB_DIR/programs.d/$2"
  chmod +x "$STUB_DIR/programs.d/$2"
  echo "$1 $2" >>"$STUB_DIR/programs"
}
# run ARGS...: runs run.sh with the stub cargo. Sets `status` and `out`.
run() {
  status=0
  out=$(cd "$STUB_DIR" && PATH="$work/bin:$PATH" CARGO="$work/bin/cargo" "$runner" "$@" 2>&1) || status=$?
}

# --- Every program runs, in its package's directory, and the doc tests run.
setup pass
program alpha alpha_tests 'echo "alpha in $PWD with $CARGO_MANIFEST_DIR"'
program beta beta_tests 'echo "beta ran"'
run --all
if [ "$status" -eq 0 ]; then ok "all passing programs exit 0"; else bad "all passing programs exit 0" "status $status: $out"; fi
case $out in
  *"alpha in $STUB_DIR/crates/alpha with $STUB_DIR/crates/alpha"*) ok "a program runs in its package directory with CARGO_MANIFEST_DIR" ;;
  *) bad "a program runs in its package directory with CARGO_MANIFEST_DIR" "$out" ;;
esac
case $out in
  *"beta ran"*"doc tests ran"*) ok "every program's output and the doc tests' output is printed" ;;
  *) bad "every program's output and the doc tests' output is printed" "$out" ;;
esac
case $out in
  *"all 2 test programs and the doc tests passed"*) ok "a passing run says how many programs passed" ;;
  *) bad "a passing run says how many programs passed" "$out" ;;
esac
if grep -qx '\[test\]\[--all\]\[--no-run\]\[--message-format=json-render-diagnostics\]' "$STUB_DIR/cargo.log" &&
  grep -qx '\[test\]\[--all\]\[--doc\]' "$STUB_DIR/cargo.log"; then
  ok "the arguments reach the build and the doc tests"
else
  bad "the arguments reach the build and the doc tests" "$(cat "$STUB_DIR/cargo.log")"
fi

# --- No arguments: cargo gets none either, not an empty one.
setup no-args
program alpha alpha_tests 'exit 0'
run
if [ "$status" -eq 0 ] && grep -qx '\[test\]\[--doc\]' "$STUB_DIR/cargo.log"; then
  ok "with no arguments, cargo gets no empty argument"
else
  bad "with no arguments, cargo gets no empty argument" "status $status: $(cat "$STUB_DIR/cargo.log")"
fi

# --- A failing program fails the run, and the others still run.
setup one-fails
program alpha alpha_tests 'echo "alpha broke"; exit 101'
program beta beta_tests 'touch "$STUB_DIR/beta-ran"'
OSKA_TEST_JOBS=1 run --all
if [ "$status" -eq 1 ]; then ok "a failing program exits 1"; else bad "a failing program exits 1" "status $status: $out"; fi
if [ -e "$STUB_DIR/beta-ran" ]; then ok "the other programs still run after a failure"; else bad "the other programs still run after a failure" "$out"; fi
case $out in
  *"alpha broke"*"these test programs failed:"*"alpha alpha_tests (exit 101)"*) ok "a failure shows its output and is named at the end" ;;
  *) bad "a failure shows its output and is named at the end" "$out" ;;
esac

# --- Failing doc tests fail the run.
setup doc-fails
program alpha alpha_tests 'exit 0'
touch "$STUB_DIR/doc-fails"
run --all
if [ "$status" -eq 1 ] && [[ $out == *"doc tests (exit 101)"* ]]; then
  ok "failing doc tests exit 1 and are named"
else
  bad "failing doc tests exit 1 and are named" "status $status: $out"
fi

# --- A failed build runs nothing.
setup build-fails
program alpha alpha_tests 'touch "$STUB_DIR/alpha-ran"'
touch "$STUB_DIR/build-fails"
run --all
if [ "$status" -eq 1 ] && [ ! -e "$STUB_DIR/alpha-ran" ] && [[ $out == *"does not compile"* ]]; then
  ok "a failed build exits 1, shows cargo's error and runs no program"
else
  bad "a failed build exits 1, shows cargo's error and runs no program" "status $status: $out"
fi

# --- Programs run at the same time: each waits for the other to start.
setup parallel
wait_for='touch "$STUB_DIR/$1"; for _ in $(seq 100); do [ -e "$STUB_DIR/$2" ] && exit 0; sleep 0.05; done; echo "$2 never started"; exit 1'
program alpha alpha_tests "set -- alpha beta; $wait_for"
program beta beta_tests "set -- beta alpha; $wait_for"
OSKA_TEST_JOBS=2 run --all
if [ "$status" -eq 0 ]; then ok "OSKA_TEST_JOBS=2 runs two programs at the same time"; else bad "OSKA_TEST_JOBS=2 runs two programs at the same time" "status $status: $out"; fi

# --- A bad job count is a usage error.
setup bad-jobs
for jobs in 0 x -1; do
  OSKA_TEST_JOBS=$jobs run --all
  if [ "$status" -eq 2 ] && [ ! -e "$STUB_DIR/cargo.log" ]; then
    ok "OSKA_TEST_JOBS=$jobs exits 2 and runs nothing"
  else
    bad "OSKA_TEST_JOBS=$jobs exits 2 and runs nothing" "status $status: $out"
  fi
done

echo
echo "$passed passed, $failures failed"
[ "$failures" -eq 0 ]
