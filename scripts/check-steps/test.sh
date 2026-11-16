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
# (cargo: `cargo SUBCOMMAND`, and its whole command line to $STUB_LOG.args),
# and fails when that is STUB_FAIL. The temp-leak wrapper stub then runs the
# command it wraps.
stub='#!/usr/bin/env bash
name=$(basename "$(dirname "$0")")/$(basename "$0")
if [ "$name" = bin/cargo ]; then echo "cargo $*" >>"$STUB_LOG.args"; name="cargo $1"; fi
echo "$name" >>"$STUB_LOG"
if [ "$name" = "${STUB_FAIL:-}" ]; then exit 1; fi
if [ "$name" = pass-stamp/stamp.sh ] && [ "${1:-}" = tree ]; then echo tree; fi
if [ "$name" = pass-stamp/stamp.sh ] && [ "${1:-}" = write ]; then echo "[stamp write $2 $3]"; fi
if [ "$name" = temp-leak/run.sh ]; then exec "$@"; fi
exit 0
'

# The scratch tree: the real check.sh, a stub for every script it names, a
# build-slot stub that logs being sourced and prints "[build slot]", a Cargo.lock with no outside crate
# and no workflows.
tree="$work/tree"
mkdir -p "$tree/scripts/build-slots" "$tree/.github/workflows" "$tree/bin"
cp "$repo_root/scripts/check.sh" "$tree/scripts/check.sh"
: >"$tree/Cargo.lock"
# shellcheck disable=SC2016 # expands when check.sh sources it
printf '%s\n' 'echo build-slots/acquire.sh >>"$STUB_LOG"' 'echo "[build slot]"' >"$tree/scripts/build-slots/acquire.sh"
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
if [ "$(printf '%s\n' "$out" | grep -c '^\[stamp write')" -eq 1 ] &&
  printf '%s\n' "$out" | grep -qx '\[stamp write full tree\]'; then
  ok "a full run stamps the full tier and the tree it checked"
else
  bad "a full run stamps the full tier and the tree it checked" "out: $out"
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
  : >"$work/log.args"
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
if printf '%s\n' "$out" | grep -q '^\[stamp write'; then
  bad "a run that skipped the cargo steps never calls stamp.sh write" "out: $out"
else
  ok "a run that skipped the cargo steps never calls stamp.sh write"
fi
after_slot=$(printf '%s\n' "$out" | grep -x -A2 '\[build slot\]' | sed '/^$/d' | sed -n 2p)
if [ "$(printf '%s\n' "$out" | grep -cx '\[build slot\]')" -eq 1 ] && [ "$after_slot" = "== installer tests" ]; then
  ok "skipped cargo steps wait for no build slot; the next step takes it"
else
  bad "skipped cargo steps wait for no build slot; the next step takes it" "out: $out"
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
cargo_case "a scripts/install/ change (Rust tests read it)" 'echo "# two" >>scripts/install/test.sh'
cargo_case "a scripts/temp-leak/ change (it wraps cargo test)" 'echo "# two" >>scripts/temp-leak/run.sh'
cargo_case "an index-only deletion (git rm --cached; Rust tests read the index)" 'git rm -q --cached scripts/foo/run.sh'
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

# --- --fast: fmt, clippy and the tests of what changed, never a stamp -----
# fast_case NAME CHANGE: runs `check.sh --fast` in a fresh clone after CHANGE.
fast_case() {
  fresh_clone
  (cd "$repo" && eval "$2")
  check_repo --fast
  headers=$(printf '%s\n' "$out" | sed -n 's/^== //p' | tr '\n' ,)
  cargo_test=$(grep '^cargo test' "$work/log.args" || true)
}
# no_stamp NAME: the run never called stamp.sh write, and said it is not the
# full check, last.
no_stamp() {
  if printf '%s\n' "$out" | grep -q '^\[stamp write'; then
    bad "$1 writes no pass stamp" "out: $out"
  else
    ok "$1 writes no pass stamp"
  fi
  if printf '%s\n' "$out" | tail -n 1 | grep -q 'NOT the full check'; then
    ok "$1 ends saying it is NOT the full check"
  else
    bad "$1 ends saying it is NOT the full check" "out: $out"
  fi
}

fast_case "a one-crate change" 'echo two >crates/a/src/lib.rs'
if [ "$status" -eq 0 ] && [ "$headers" = "cargo fmt,cargo clippy,cargo test (private temporary directory)," ]; then
  ok "--fast on a crate change runs fmt, clippy and cargo test only"
else
  bad "--fast on a crate change runs fmt, clippy and cargo test only" "exit $status, headers: $headers"
fi
if [ "$cargo_test" = "cargo test -p a" ]; then
  ok "--fast tests only the changed crate"
else
  bad "--fast tests only the changed crate" "got: $cargo_test"
fi
no_stamp "--fast"
if printf '%s\n' "$log" | grep -qx pass-stamp/stamp.sh; then
  bad "--fast never calls stamp.sh" "log: $log"
else
  ok "--fast never calls stamp.sh"
fi

fast_case "two crates" 'mkdir -p crates/b && echo b >crates/b/lib.rs && echo two >crates/a/src/lib.rs && git add -A && git -c user.name=t -c user.email=t@e -c commit.gpgsign=false commit -qm c'
if [ "$cargo_test" = "cargo test -p a -p b" ]; then
  ok "--fast tests each changed crate, committed or not"
else
  bad "--fast tests each changed crate, committed or not" "got: $cargo_test"
fi

fast_case "a docs change" 'echo two >docs/index.md'
if [ "$cargo_test" = "cargo test --all" ]; then
  ok "--fast tests every crate for a cargo input outside crates/"
else
  bad "--fast tests every crate for a cargo input outside crates/" "got: $cargo_test"
fi

fast_case "a script change" 'echo "# two" >>scripts/kill-guard/test.sh'
if [ "$status" -eq 0 ] && [ "$headers" = "kill guard tests,shellcheck," ] &&
  ! ran_cargo; then
  ok "--fast on a script change runs that script's tests and shellcheck only"
else
  bad "--fast on a script change runs that script's tests and shellcheck only" "exit $status, headers: $headers, log: $log"
fi
no_stamp "--fast on a script change"

fast_case "a check.sh change" 'echo "# two" >>scripts/check.sh'
if printf '%s\n' "$headers" | grep -q 'check step registry tests'; then
  ok "--fast on a check.sh change runs the step registry tests"
else
  bad "--fast on a check.sh change runs the step registry tests" "headers: $headers"
fi

fast_case "a hook change" 'mkdir -p .githooks && echo "# two" >.githooks/pre-push'
if [ "$headers" = "pass stamp tests,shellcheck," ]; then
  ok "--fast on a pre-push hook change runs the pass stamp tests and shellcheck"
else
  bad "--fast on a pre-push hook change runs the pass stamp tests and shellcheck" "headers: $headers"
fi

fast_case "no change" ':'
if [ "$status" -eq 0 ] && [ -z "$headers" ]; then
  ok "--fast with nothing changed runs no step"
else
  bad "--fast with nothing changed runs no step" "exit $status, headers: $headers"
fi
no_stamp "--fast with nothing changed"

STUB_FAIL=kill-guard/test.sh fast_case "a failing script test" 'echo "# two" >>scripts/kill-guard/test.sh'
if [ "$status" -eq 1 ] && ! printf '%s\n' "$out" | grep -q 'NOT the full check'; then
  ok "a failing --fast step exits 1"
else
  bad "a failing --fast step exits 1" "exit $status, out: $out"
fi

# Drift guard: every scripts/*/test.sh runs in --fast when its directory
# changes. A new suite with no --fast-when fails here.
missing=
for suite in "$repo_root"/scripts/*/test.sh; do
  dir=$(basename "$(dirname "$suite")")
  fast_case "a $dir change" "echo '# two' >>scripts/$dir/test.sh"
  printf '%s\n' "$log" | grep -qxF "$dir/test.sh" || missing="$missing $dir"
done
if [ -z "$missing" ]; then
  ok "--fast runs each scripts/*/test.sh when its directory changes"
else
  bad "--fast runs each scripts/*/test.sh when its directory changes" "not run:$missing"
fi

# Cannot tell what changed: every step runs, still with no stamp.
fast_case "no origin/main" 'echo two >CLAUDE.md && git update-ref -d refs/remotes/origin/main'
headers=$(printf '%s\n' "$out" | sed -n 's/^== //p')
if [ "$status" -eq 0 ] && [ "$headers" = "$list" ] && [ "$cargo_test" = "cargo test --all" ] &&
  printf '%s\n' "$out" | grep -q 'cannot tell what changed'; then
  ok "--fast that cannot tell what changed runs the full check, and says why"
else
  bad "--fast that cannot tell what changed runs the full check, and says why" "exit $status, out: $out"
fi
no_stamp "--fast falling back to the full check"
if [ "$(printf '%s\n' "$log" | grep -cx pass-stamp/stamp.sh)" -eq 0 ]; then
  ok "--fast falling back never calls stamp.sh"
else
  bad "--fast falling back never calls stamp.sh" "log: $log"
fi

# --- drift guard: crates/ reads no repo file cargo_input calls a non-input --
# Covers the paths written as literals: `include_str!`/`include_bytes!` (from
# the file's directory) and `Path::new(env!("CARGO_MANIFEST_DIR")).join("..")`
# on one line (from the crate's directory). A path built at run time (a root
# joined to a variable, `git ls-files`) is not seen; cargo_input's comment
# lists those.
eval "$(sed -n '/^cargo_input() {/,/^}/p' "$repo_root/scripts/check.sh")"
seen=0
outside=
while IFS=: read -r file kind path; do
  case $kind in
    include) from=$(dirname "$file") ;;
    manifest) from=${file%%/src/*}; from=${from%%/tests/*}; from=${from%%/benches/*} ;;
  esac
  resolved=$(realpath -m --relative-to="$repo_root" "$repo_root/$from/$path")
  seen=$((seen + 1))
  case $resolved in
    ..*) outside="$outside $file:$path(outside the repository)"; continue ;;
  esac
  # A directory read covers what is inside it.
  if [ -d "$repo_root/$resolved" ]; then resolved="$resolved/x"; fi
  cargo_input "$resolved" || outside="$outside $file:$resolved"
done < <(
  cd "$repo_root" &&
    grep -rhoE --include='*.rs' -H 'include_(str|bytes)!\("[^"]+"\)' crates |
    sed -E 's/^([^:]+):include_(str|bytes)!\("([^"]+)"\)$/\1:include:\3/'
  cd "$repo_root" &&
    grep -roE --include='*.rs' -H 'env!\("CARGO_MANIFEST_DIR"\)\)\.join\("[^"]+"\)' crates |
    sed -E 's/^([^:]+):.*join\("([^"]+)"\)$/\1:manifest:\2/'
)
if [ "$seen" -lt 20 ]; then
  bad "the drift guard finds the crates' repo-file reads" "found only $seen"
elif [ -z "$outside" ]; then
  ok "every literal repo-file read in crates/ ($seen) is a cargo input"
else
  bad "every literal repo-file read in crates/ is a cargo input" "not inputs:$outside"
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
