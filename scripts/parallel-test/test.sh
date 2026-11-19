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
# ("DIR PROGRAM KIND [PACKAGE]" per line; PACKAGE defaults to DIR); it fails
# when $STUB_DIR/build-fails exists. The doc tests fail when
# $STUB_DIR/doc-fails exists.
mkdir -p "$work/bin"
cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '[%s]' "$@" >>"$STUB_DIR/cargo.log"
echo >>"$STUB_DIR/cargo.log"
for a in "$@"; do
  if [ "$a" = --no-run ]; then
    [ -e "$STUB_DIR/build-fails" ] && { echo "error: does not compile" >&2; exit 101; }
    echo "not JSON: a build script's line"
    while read -r dir program kind package; do
      if [ -n "$package" ]; then id="$STUB_DIR/crates/$dir#$package@0.1.0"; else id="$STUB_DIR/crates/$dir#0.1.0"; fi
      printf '{"reason":"compiler-artifact","package_id":"path+file://%s","manifest_path":"%s/Cargo.toml","target":{"kind":["%s"]},"profile":{"test":true},"executable":"%s"}\n' \
        "$id" "$STUB_DIR/crates/$dir" "$kind" "$STUB_DIR/programs.d/$program"
      # A library that is not a test program, and a test-profile artifact with no executable.
      printf '{"reason":"compiler-artifact","package_id":"path+file://%s","manifest_path":"%s/Cargo.toml","target":{"kind":["lib"]},"profile":{"test":false},"executable":null}\n' \
        "$id" "$STUB_DIR/crates/$dir"
      printf '{"reason":"compiler-artifact","package_id":"path+file://%s","manifest_path":"%s/Cargo.toml","target":{"kind":["test"]},"profile":{"test":true},"executable":null}\n' \
        "$id" "$STUB_DIR/crates/$dir"
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
# program DIR NAME BODY [KIND [PACKAGE]]: a stub test program that runs
# BODY. KIND is its target kind (default lib: a library's unit tests).
program() {
  mkdir -p "$STUB_DIR/crates/$1"
  printf '#!/usr/bin/env bash\n%s\n' "$3" >"$STUB_DIR/programs.d/$2"
  chmod +x "$STUB_DIR/programs.d/$2"
  echo "$1 $2 ${4:-lib} ${5:-}" >>"$STUB_DIR/programs"
}
# run ARGS...: runs run.sh with the stub cargo. Sets `status` and `out`.
run() {
  status=0
  out=$(cd "$STUB_DIR" && PATH="$work/bin:$PATH" CARGO="$work/bin/cargo" "$runner" "$@" 2>&1) || status=$?
}

# --- Every program runs, in its package's directory, and the doc tests run.
setup pass
program alpha alpha_tests 'echo "alpha in $PWD with $CARGO_MANIFEST_DIR"'
program beta beta_tests 'echo "beta ran"' test
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
  grep -qx '\[test\]\[--doc\]\[-p\]\[alpha\]' "$STUB_DIR/cargo.log"; then
  ok "the build gets the arguments; the doc tests name each package with a library"
else
  bad "the build gets the arguments; the doc tests name each package with a library" "$(cat "$STUB_DIR/cargo.log")"
fi

# --- No arguments: cargo gets none either, not an empty one.
setup no-args
program alpha alpha_tests 'exit 0'
run
if [ "$status" -eq 0 ] && grep -qx '\[test\]\[--no-run\]\[--message-format=json-render-diagnostics\]' "$STUB_DIR/cargo.log"; then
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

# --- The doc tests keep other arguments, drop the package choice, and use
# each package's name, not its directory.
setup doc-args
program alpha alpha_tests 'exit 0'
program gamma-dir gamma_tests 'exit 0' lib gamma
program beta beta_tests 'exit 0' bin
run -p alpha --package gamma --package=beta --release
if [ "$status" -eq 0 ] && grep -qx '\[test\]\[--release\]\[--doc\]\[-p\]\[alpha\]\[-p\]\[gamma\]' "$STUB_DIR/cargo.log"; then
  ok "the doc tests keep other arguments and name each library package"
else
  bad "the doc tests keep other arguments and name each library package" "status $status: $(cat "$STUB_DIR/cargo.log")"
fi

# --- No package with a library: no doc tests (cargo would refuse them).
setup no-lib
program oskac main_tests 'exit 0' bin
program oskac cli_tests 'exit 0' test
run -p oskac
if [ "$status" -eq 0 ] && ! grep -q -- '--doc' "$STUB_DIR/cargo.log" && [[ $out == *"all 2 test programs passed"* ]]; then
  ok "without a library package, the doc tests are not run"
else
  bad "without a library package, the doc tests are not run" "status $status: $out"
fi

# --- A build with no test program fails: nothing was tested.
setup none
run --all
if [ "$status" -eq 1 ] && [[ $out == *"no test program"* ]] && ! grep -q -- '--doc' "$STUB_DIR/cargo.log"; then
  ok "a build with no test program exits 1"
else
  bad "a build with no test program exits 1" "status $status: $out"
fi

# --- A program killed by a signal fails the run.
setup killed
program alpha alpha_tests 'kill -9 $$'
run --all
if [ "$status" -eq 1 ] && [[ $out == *"alpha alpha_tests (exit 137)"* ]]; then
  ok "a program killed by a signal fails the run"
else
  bad "a program killed by a signal fails the run" "status $status: $out"
fi

# --- Arguments for the test programs are refused.
setup dash-dash
program alpha alpha_tests 'exit 0'
run --all -- --nocapture
if [ "$status" -eq 2 ] && [ ! -e "$STUB_DIR/cargo.log" ]; then
  ok "-- in the arguments exits 2 and runs nothing"
else
  bad "-- in the arguments exits 2 and runs nothing" "status $status: $out"
fi

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
