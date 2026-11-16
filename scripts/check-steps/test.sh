#!/usr/bin/env bash
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
for tool in cargo shellcheck; do
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

# --- the cargo steps skip when no cargo input differs from origin/main ------
# A scratch clone: the scratch tree committed, with origin/main at that
# commit. Each case starts from a fresh copy and changes PATHS.
git_in() {
  git -C "$1" -c user.name=test -c user.email=test@example.com -c commit.gpgsign=false "${@:2}"
}
base="$work/base"
cp -a "$tree" "$base"
mkdir -p "$base/crates/a/src" "$base/docs" "$base/scripts/foo"
for file in CLAUDE.md crates/a/src/lib.rs docs/index.md scripts/foo/run.sh .github/workflows/ci.yml; do
  echo one >"$base/$file"
done
git_in "$base" init -q -b main
git_in "$base" add -A
git_in "$base" commit -q -m base
git_in "$base" update-ref refs/remotes/origin/main HEAD

# fresh_clone: a copy of the base in $repo, as the tree the next run checks.
fresh_clone() {
  repo="$work/repo"
  rm -rf "$repo"
  cp -a "$base" "$repo"
}

# check_repo ARGS...: `check`, in $repo, outside CI.
check_repo() {
  : >"$work/log"
  set +e
  out=$(cd "$repo" && env -u CI -u GITHUB_ACTIONS PATH="$tree/bin:$PATH" STUB_LOG="$work/log" \
    scripts/check.sh "$@" 2>&1)
  status=$?
  set -e
  log=$(cat "$work/log")
}

ran_cargo() { printf '%s\n' "$log" | grep -q '^cargo '; }

fresh_clone
echo two >"$repo/CLAUDE.md"
echo two >"$repo/.github/workflows/ci.yml"
git_in "$repo" commit -qam docs
echo two >"$repo/scripts/foo/run.sh"
check_repo
if [ "$status" -eq 0 ] && ! ran_cargo; then
  ok "a docs, workflow and script change skips the cargo steps"
else
  bad "a docs, workflow and script change skips the cargo steps" "exit $status, log: $log"
fi
skips=$(printf '%s\n' "$out" | grep -c 'no cargo input differs from origin/main' || true)
if [ "$skips" -eq 3 ]; then
  ok "each skipped cargo step says why"
else
  bad "each skipped cargo step says why" "got: $out"
fi
headers=$(printf '%s\n' "$out" | sed -n 's/^== //p')
if [ "$headers" = "$list" ]; then
  ok "a skipping run still prints every step"
else
  bad "a skipping run still prints every step" "got: $headers"
fi
if [ "$(printf '%s\n' "$log" | grep -cx pass-stamp/stamp.sh)" -eq 1 ] &&
  printf '%s\n' "$out" | grep -q 'pass stamp: not written'; then
  ok "a run that skipped the cargo steps writes no pass stamp, and says so"
else
  bad "a run that skipped the cargo steps writes no pass stamp, and says so" "log: $log; out: $out"
fi

# Each case: a change, and whether the cargo steps run.
cargo_case() {
  local name=$1 change=$2
  fresh_clone
  (cd "$repo" && eval "$change")
  check_repo
  if [ "$status" -eq 0 ] && ran_cargo; then
    ok "$name runs the cargo steps"
  else
    bad "$name runs the cargo steps" "exit $status, out: $out"
  fi
}
cargo_case "a committed crate change" 'echo two >crates/a/src/lib.rs && git -c user.name=t -c user.email=t@e -c commit.gpgsign=false commit -qam c'
cargo_case "an uncommitted crate change" 'echo two >crates/a/src/lib.rs'
cargo_case "a new untracked crate file" 'echo new >crates/a/src/new.rs'
cargo_case "a docs/ change (Rust tests read docs/)" 'echo two >docs/index.md'
cargo_case "a Cargo.toml change" 'echo two >Cargo.toml'
cargo_case "a new rust-toolchain file" 'echo two >rust-toolchain.toml'
cargo_case "a deleted script (Rust tests read the tracked files)" 'git rm -q scripts/foo/run.sh'
cargo_case "a change to check.sh" 'echo "# two" >>scripts/check.sh'
cargo_case "a pages.yml change" 'echo two >.github/workflows/pages.yml'
cargo_case "no origin/main" 'echo two >CLAUDE.md && git update-ref -d refs/remotes/origin/main'
cargo_case "a detached HEAD without origin/main" 'git checkout -q --detach && git update-ref -d refs/remotes/origin/main'

# A shallow clone cannot find the merge-base it needs.
fresh_clone
echo two >"$repo/README.md"
git_in "$repo" add README.md
git_in "$repo" commit -qm second
rm -rf "$work/shallow"
git clone -q --depth 1 "file://$repo" "$work/shallow"
repo="$work/shallow"
echo two >"$repo/CLAUDE.md"
check_repo
if ran_cargo; then ok "a shallow clone runs the cargo steps"; else bad "a shallow clone runs the cargo steps" "out: $out"; fi

# CI always runs the full check.
for ci in CI GITHUB_ACTIONS; do
  fresh_clone
  echo two >"$repo/CLAUDE.md"
  : >"$work/log"
  set +e
  out=$(cd "$repo" && env -u CI -u GITHUB_ACTIONS "$ci=true" PATH="$tree/bin:$PATH" STUB_LOG="$work/log" \
    scripts/check.sh 2>&1)
  status=$?
  set -e
  log=$(cat "$work/log")
  if [ "$status" -eq 0 ] && ran_cargo; then
    ok "$ci=true runs the cargo steps"
  else
    bad "$ci=true runs the cargo steps" "exit $status, out: $out"
  fi
done

# --- an unknown argument is a usage error -----------------------------------
check --bogus
if [ "$status" -eq 2 ] && [ -z "$log" ]; then
  ok "an unknown argument exits 2 and runs nothing"
else
  bad "an unknown argument exits 2 and runs nothing" "exit $status, ran: $log"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
