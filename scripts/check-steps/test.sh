#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for scripts/check.sh's step registry: `--list` prints the steps in
# the order a run prints their headers, so the list cannot drift from the
# steps that run. Each case runs the real check.sh in a scratch tree where
# every script it calls, cargo, shellcheck and claude are stubs that log their name,
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
# (cargo: `cargo SUBCOMMAND`; its whole command line goes to $STUB_LOG.cargo,
# and with a `cargo ` prefix to $STUB_LOG.args), and fails when that is
# STUB_FAIL. The temp-leak wrapper stub then runs the command it wraps.
# `cargo metadata` prints $STUB_METADATA, if set, with @ROOT@ as the
# directory it runs in.
# shellcheck disable=SC2016 # the stub script is written out, not expanded here
stub='#!/usr/bin/env bash
name=$(basename "$(dirname "$0")")/$(basename "$0")
if [ "$name" = bin/cargo ]; then echo "cargo $*" >>"$STUB_LOG.args"; echo "$*" >>"$STUB_LOG.cargo"; name="cargo $1"; fi
echo "$name" >>"$STUB_LOG"
if [ "$name" = "${STUB_FAIL:-}" ]; then exit 1; fi
if [ "$name" = pass-stamp/stamp.sh ] && [ "${1:-}" = tree ]; then echo tree; fi
if [ "$name" = pass-stamp/stamp.sh ] && [ "${1:-}" = write ]; then echo "[stamp write $2 $3]"; fi
if [ "$name" = temp-leak/run.sh ]; then exec "$@"; fi
if [ "$name" = "cargo metadata" ] && [ -n "${STUB_METADATA:-}" ]; then sed "s|@ROOT@|$PWD|g" "$STUB_METADATA"; fi
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
for tool in cargo shellcheck pwsh claude; do
  printf '%s' "$stub" >"$tree/bin/$tool"
  chmod +x "$tree/bin/$tool"
done

# The real docs-test list, which reads the scratch crates.
cp "$repo_root/scripts/docs-tests/list.sh" "$tree/scripts/docs-tests/list.sh"

# rustup reports the Windows target installed, so the Windows step runs its
# (stubbed) cargo check the same on a machine with or without the target.
printf '#!/usr/bin/env bash\necho x86_64-pc-windows-gnu\n' >"$tree/bin/rustup"
chmod +x "$tree/bin/rustup"

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
# The cheap static steps run before the cargo test step, so their mistakes
# fail in seconds and not after the whole test run.
test_line=$(printf '%s\n' "$list" | grep -nxF "cargo test (private temporary directory)" | cut -d: -f1)
for cheap in "SPDX headers and NOTICE" "shellcheck" "workflow validation" "powershell parse" "cargo fmt"; do
  cheap_line=$(printf '%s\n' "$list" | grep -nxF "$cheap" | cut -d: -f1)
  if [ -n "$cheap_line" ] && [ -n "$test_line" ] && [ "$cheap_line" -lt "$test_line" ]; then
    ok "--list puts \"$cheap\" before the cargo test step"
  else
    bad "--list puts \"$cheap\" before the cargo test step" "at $cheap_line, cargo test at $test_line"
  fi
done
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
if [ "$slot_and_neighbours" = "bin/pwsh,build-slots/acquire.sh,cargo fmt," ]; then
  ok "a run takes the build slot once, after the cheap guards and before cargo fmt"
else
  bad "a run takes the build slot once, after the cheap guards and before cargo fmt" "log: $log"
fi

# Timing: each step prints its elapsed seconds, and a run ends with the
# slowest steps, longest first. The cases check the shape, never the values.
n_headers=$(printf '%s\n' "$out" | grep -c '^== ')
n_timers=$(printf '%s\n' "$out" | grep -cE '^\([0-9]+s\)$')
if [ "$n_headers" -gt 0 ] && [ "$n_headers" -eq "$n_timers" ]; then
  ok "each step prints its duration"
else
  bad "each step prints its duration" "$n_headers headers, $n_timers timer lines"
fi
# A timer line comes right before the next header or the summary.
if ! printf '%s\n' "$out" | awk '
  /^== / { if (open) exit 1; open = 1; next }
  /^\([0-9]+s\)$/ { if (!open) exit 1; open = 0 }
  END { exit open }'; then
  bad "a step's duration follows its own output" "out: $out"
else
  ok "a step's duration follows its own output"
fi
slowest=$(printf '%s\n' "$out" | sed -n '/^Slowest steps:$/,/^All checks passed\.$/p' | sed -n 's/^ *\([0-9][0-9]*\)s  .*/\1/p')
if [ -n "$slowest" ] && [ "$slowest" = "$(printf '%s\n' "$slowest" | sort -rn)" ] &&
  [ "$(printf '%s\n' "$slowest" | wc -l)" -le 5 ]; then
  ok "a run ends with the slowest steps, longest first"
else
  bad "a run ends with the slowest steps, longest first" "got: $slowest; out: $out"
fi
for flag in "--group scripts" --fast; do
  # shellcheck disable=SC2086 # the flag is one or two words
  check $flag
  if [ "$status" -eq 0 ] && printf '%s\n' "$out" | grep -q '^Slowest steps:$' &&
    ! printf '%s\n' "$out" | grep -q '^\[stamp write'; then
    ok "$flag prints the slowest steps and writes no stamp"
  else
    bad "$flag prints the slowest steps and writes no stamp" "exit $status: $out"
  fi
done
check --list
if ! printf '%s\n' "$out" | grep -qE '^\([0-9]+s\)$|Slowest'; then
  ok "--list prints no timing"
else
  bad "--list prints no timing" "got: $out"
fi
check

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
mkdir -p "$base/crates/a/src" "$base/crates/a/tests" "$base/docs" "$base/scripts/foo"
for file in CLAUDE.md crates/a/src/lib.rs docs/index.md docs/compiled.md scripts/foo/run.sh .github/workflows/ci.yml; do
  echo one >"$base/$file"
done
# Crate a's package is pkg-a. One test reads docs/, one does not, and the
# crate itself compiles docs/compiled.md in.
printf '[package]\nname = "pkg-a"\n' >"$base/crates/a/Cargo.toml"
echo 'const D: &str = include_str!("../../../docs/index.md");' >"$base/crates/a/tests/reads_docs.rs"
echo 'fn plain() {}' >"$base/crates/a/tests/plain.rs"
echo 'const H: &str = include_str!("../../../docs/compiled.md");' >"$base/crates/a/src/help.rs"
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
  : >"$work/log.cargo"
  set +e
  out=$(cd "$repo" && env -u CI -u GITHUB_ACTIONS PATH="$tree/bin:$PATH" STUB_LOG="$work/log" \
    STUB_METADATA="$work/metadata.json" \
    scripts/check.sh "$@" 2>&1)
  status=$?
  set -e
  log=$(cat "$work/log")
  cargo_log=$(cat "$work/log.cargo")
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
if [ "$skips" -eq 5 ]; then
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
# The skip is decided from the tree the stamp names, so the stamp vouches
# for it (#360).
if printf '%s\n' "$out" | grep -qx '\[stamp write full tree\]' &&
  ! printf '%s\n' "$out" | grep -q 'pass stamp: not written'; then
  ok "a run that skipped the cargo steps because no cargo input changed writes the full stamp"
else
  bad "a run that skipped the cargo steps because no cargo input changed writes the full stamp" "out: $out"
fi
# The first step after the cargo steps takes the slot in `step`, before its
# block runs, so this holds whether or not that step's tool (jq) is here.
after_slot=$(printf '%s\n' "$out" | grep -x -A2 '\[build slot\]' | sed '/^$/d' | sed -n 2p)
if [ "$(printf '%s\n' "$out" | grep -cx '\[build slot\]')" -eq 1 ] && [ "$after_slot" = "== eval batch driver tests" ]; then
  ok "skipped cargo steps wait for no build slot; the next step takes it"
else
  bad "skipped cargo steps wait for no build slot; the next step takes it" "out: $out"
fi

# --- only docs/ changed: just the tests that read docs/ run -----------------
full_cargo='fmt --all --check
clippy --all-targets -- -D warnings
check -p oskac --tests --target x86_64-pc-windows-gnu
test --all --no-fail-fast'
for change in 'echo two >docs/index.md' 'echo new >docs/new.md' \
  'echo two >docs/index.md && git -c user.name=t -c user.email=t@e -c commit.gpgsign=false commit -qam d'; do
  fresh_clone
  (cd "$repo" && eval "$change")
  check_repo
  name="only docs/ changed ($change)"
  if [ "$status" -eq 0 ] && [ "$cargo_log" = "test -p pkg-a --test reads_docs" ]; then
    ok "$name: runs just the tests that read docs/"
  else
    bad "$name: runs just the tests that read docs/" "exit $status, cargo: $cargo_log; out: $out"
  fi
done
if printf '%s\n' "$out" | grep -qx '  pkg-a reads_docs' &&
  [ "$(printf '%s\n' "$out" | grep -cE '^(skipped|narrowed): only docs/ differs from origin/main')" -eq 5 ] &&
  printf '%s\n' "$out" | grep -x -A1 '== oskac type-checks for Windows' | grep -q '^skipped: only docs/ differs'; then
  ok "each cargo step of a docs-only run says why it skipped or narrowed, and which tests run"
else
  bad "each cargo step of a docs-only run says why it skipped or narrowed, and which tests run" "out: $out"
fi
if [ "$(printf '%s\n' "$log" | grep -cx pass-stamp/stamp.sh)" -eq 1 ] &&
  printf '%s\n' "$out" | grep -q 'pass stamp: not written'; then
  ok "a docs-only run writes no pass stamp"
else
  bad "a docs-only run writes no pass stamp" "log: $log; out: $out"
fi
why=$(printf '%s\n' "$out" | sed -n '/^pass stamp: not written/,/^$/p')
if printf '%s\n' "$why" | grep -qx '  cargo fmt: skipped: only docs/ differs.*' &&
  printf '%s\n' "$why" | grep -qx '  oskac type-checks for Windows: skipped: only docs/ differs.*' &&
  printf '%s\n' "$why" | grep -qx '  cargo test (private temporary directory): narrowed to the tests that read docs/'; then
  ok "a docs-only run names each step the stamp cannot vouch for, and why"
else
  bad "a docs-only run names each step the stamp cannot vouch for, and why" "out: $out"
fi
headers=$(printf '%s\n' "$out" | sed -n 's/^== //p')
if [ "$headers" = "$list" ]; then
  ok "a docs-only run still prints every step"
else
  bad "a docs-only run still prints every step" "got: $headers"
fi

# Each case: a change, and whether all of the cargo steps run.
cargo_case() {
  local name=$1 change=$2
  fresh_clone
  (cd "$repo" && eval "$change")
  check_repo
  if [ "$status" -eq 0 ] && [ "$cargo_log" = "$full_cargo" ]; then
    ok "$name runs the cargo steps"
  else
    bad "$name runs the cargo steps" "exit $status, out: $out"
  fi
}
cargo_case "a committed crate change" 'echo two >crates/a/src/lib.rs && git -c user.name=t -c user.email=t@e -c commit.gpgsign=false commit -qam c'
cargo_case "an uncommitted crate change" 'echo two >crates/a/src/lib.rs'
cargo_case "a new untracked crate file" 'echo new >crates/a/src/new.rs'
cargo_case "a docs/ change with a crate change" 'echo two >docs/index.md && echo two >crates/a/src/lib.rs'
cargo_case "a change to a docs/ file a crate compiles in" 'echo two >docs/compiled.md'
cargo_case "a deleted docs/ file" 'git rm -q docs/index.md'
cargo_case "a docs-test list change" 'echo "# two" >>scripts/docs-tests/list.sh'
# A docs/ reader the list cannot see is in the base: the drift guard fails,
# so a docs-only change runs all of cargo and says why.
cargo_case "a docs/ change with an unclassified docs/ reader" \
  'echo "fn f() { std::fs::read_to_string(format!(\"{}/docs/x\", other::root())); }" >crates/a/tests/hidden.rs &&
   git add -A && git -c user.name=t -c user.email=t@e -c commit.gpgsign=false commit -qm hidden &&
   git update-ref refs/remotes/origin/main HEAD && echo two >docs/index.md'
if printf '%s\n' "$out" | grep -q 'crates/a/tests/hidden.rs'; then
  ok "an unclassified docs/ reader is named"
else
  bad "an unclassified docs/ reader is named" "out: $out"
fi
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
  echo two >"$repo/docs/index.md"
  : >"$work/log"
  : >"$work/log.cargo"
  set +e
  out=$(cd "$repo" && env -u CI -u GITHUB_ACTIONS "$ci=true" PATH="$tree/bin:$PATH" STUB_LOG="$work/log" \
    scripts/check.sh 2>&1)
  status=$?
  set -e
  cargo_log=$(cat "$work/log.cargo")
  if [ "$status" -eq 0 ] && [ "$cargo_log" = "$full_cargo" ]; then
    ok "$ci=true runs all of the cargo steps, even for a docs/ change"
  else
    bad "$ci=true runs all of the cargo steps, even for a docs/ change" "exit $status, cargo: $cargo_log"
  fi
done

# --- --fast: fmt, clippy and the tests of what changed, never a stamp -----
# The scratch workspace, as `cargo metadata --no-deps` describes it: a and b
# stand alone; app (in crates/app-dir) depends on lib and core, top on app,
# and c, d and e on core. So a change to core reaches six of the nine.
pkg() { # NAME DIR DEPS...
  local name=$1 dir=$2 deps='' dep
  shift 2
  for dep in "$@"; do
    deps="$deps${deps:+,}{\"name\":\"$dep\",\"kind\":null,\"path\":\"@ROOT@/crates/$dep\"}"
  done
  printf '{"name":"%s","manifest_path":"@ROOT@/%s/Cargo.toml","dependencies":[%s]}' "$name" "$dir" "$deps"
}
printf '{"packages":[%s,%s,%s,%s,%s,%s,%s,%s,%s],"workspace_root":"@ROOT@"}\n' \
  "$(pkg a crates/a)" "$(pkg b crates/b)" "$(pkg c crates/c core)" "$(pkg d crates/d core)" \
  "$(pkg e crates/e core)" "$(pkg app crates/app-dir lib core)" "$(pkg lib crates/lib)" \
  "$(pkg top crates/top app)" "$(pkg core crates/core)" >"$work/metadata.json"

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
if [ "$cargo_test" = "cargo test --no-fail-fast -p a" ]; then
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
if [ "$cargo_test" = "cargo test --no-fail-fast -p a -p b" ]; then
  ok "--fast tests each changed crate, committed or not"
else
  bad "--fast tests each changed crate, committed or not" "got: $cargo_test"
fi

fast_case "a library crate change" 'mkdir -p crates/lib && echo two >crates/lib/lib.rs'
if [ "$cargo_test" = "cargo test --no-fail-fast -p app -p lib -p top" ]; then
  ok "--fast tests a changed crate's dependents, direct and indirect"
else
  bad "--fast tests a changed crate's dependents, direct and indirect" "got: $cargo_test"
fi

fast_case "a crate whose directory is not its name" 'mkdir -p crates/app-dir && echo two >crates/app-dir/main.rs'
if [ "$cargo_test" = "cargo test --no-fail-fast -p app -p top" ]; then
  ok "--fast names a crate by its package name, not its directory"
else
  bad "--fast names a crate by its package name, not its directory" "got: $cargo_test"
fi

fast_case "a core crate change" 'mkdir -p crates/core && echo two >crates/core/lib.rs'
if [ "$cargo_test" = "cargo test --no-fail-fast --all" ]; then
  ok "--fast runs the full test command when half the workspace or more would be tested"
else
  bad "--fast runs the full test command when half the workspace or more would be tested" "got: $cargo_test"
fi
if [ "$(grep -c '^cargo test' "$work/log.args")" -eq 1 ]; then
  ok "--fast runs cargo test once"
else
  bad "--fast runs cargo test once" "got: $(cat "$work/log.args")"
fi

fast_case "a file in no package" 'mkdir -p crates/loose && echo two >crates/loose/x.rs'
if [ "$cargo_test" = "cargo test --no-fail-fast --all" ]; then
  ok "--fast tests every crate when a changed file is in no package"
else
  bad "--fast tests every crate when a changed file is in no package" "got: $cargo_test"
fi

STUB_FAIL="cargo metadata" fast_case "no cargo metadata" 'echo two >crates/a/src/lib.rs'
if [ "$status" -eq 0 ] && [ "$cargo_test" = "cargo test --no-fail-fast --all" ]; then
  ok "--fast tests every crate when cargo metadata fails"
else
  bad "--fast tests every crate when cargo metadata fails" "exit $status, got: $cargo_test"
fi

fast_case "a Cargo.toml change" 'echo two >Cargo.toml'
if [ "$cargo_test" = "cargo test --no-fail-fast --all" ]; then
  ok "--fast tests every crate for a cargo input outside crates/"
else
  bad "--fast tests every crate for a cargo input outside crates/" "got: $cargo_test"
fi

# Fast and docs mode compose: a docs-only change runs just the docs tests.
fast_case "a docs change" 'echo two >docs/index.md'
if [ "$status" -eq 0 ] && [ "$headers" = "cargo test (private temporary directory)," ] &&
  [ "$cargo_test" = "cargo test -p pkg-a --test reads_docs" ]; then
  ok "--fast on a docs-only change runs just the tests that read docs/"
else
  bad "--fast on a docs-only change runs just the tests that read docs/" "exit $status, headers: $headers, cargo: $cargo_test"
fi
no_stamp "--fast on a docs-only change"

fast_case "an eval task change" 'mkdir -p eval/tasks && echo two >eval/tasks/t.toml'
if [ "$status" -eq 0 ] && printf '%s\n' "$headers" | tr , '\n' | grep -qx 'eval task set'; then
  ok "--fast on an eval task change validates the task set"
else
  bad "--fast on an eval task change validates the task set" "exit $status, headers: $headers"
fi

fast_case "an eval-tasks script change" 'mkdir -p scripts/eval-tasks && echo "# two" >>scripts/eval-tasks/run.sh'
if [ "$status" -eq 0 ] && printf '%s\n' "$headers" | tr , '\n' | grep -qx 'eval task set'; then
  ok "--fast on a change to the eval-tasks script validates the task set"
else
  bad "--fast on a change to the eval-tasks script validates the task set" "exit $status, headers: $headers"
fi

fast_case "a script change" 'echo "# two" >>scripts/kill-guard/test.sh'
if [ "$status" -eq 0 ] && [ "$headers" = "shellcheck,kill guard tests," ] &&
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
if [ "$headers" = "shellcheck,pass stamp tests," ]; then
  ok "--fast on a pre-push hook change runs the pass stamp tests and shellcheck"
else
  bad "--fast on a pre-push hook change runs the pass stamp tests and shellcheck" "headers: $headers"
fi

fast_case "no change" ':'
if [ "$status" -eq 0 ] && [ -z "$headers" ] && printf '%s\n' "$out" | grep -q 'no steps ran' &&
  ! printf '%s\n' "$out" | grep -q 'passed'; then
  ok "--fast with nothing changed runs no step, and says so"
else
  bad "--fast with nothing changed runs no step, and says so" "exit $status, out: $out"
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
if [ "$status" -eq 0 ] && [ "$headers" = "$list" ] && [ "$cargo_test" = "cargo test --all --no-fail-fast" ] &&
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

# --- drift guard: the real workspace's crates and their dependents ----------
# fast_packages and fast_crates on this repository, with real cargo
# metadata: each crate's changed file maps to the package its own Cargo.toml
# names; a change to oska-syntax reaches every crate that depends on it
# (known from the manifests), so it runs the full test command; a change to
# oskac, which nothing depends on, tests oskac alone.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi
if command -v cargo >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
  real() { # FUNCTION PATH: FUNCTION's output when only PATH changed
    (
      cd "$repo_root"
      eval "$(sed -n '/^cargo_input() {/,/^}/p; /^changes() {/,/^}/p; /^fast_crates() {/,/^}/p; /^fast_packages() {/,/^}/p' scripts/check.sh)"
      # shellcheck disable=SC2034 # read by the functions eval'd above
      changes_why=known deleted='' cargo_changed=$2
      "$1"
    )
  }
  sorted() { tr ' ' '\n' | grep -v '^-p$' | LC_ALL=C sort | tr '\n' ' '; }
  unmapped=
  for manifest in "$repo_root"/crates/*/Cargo.toml; do
    dir=$(dirname "$manifest")
    name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest" | head -n 1)
    got=$(real fast_packages "crates/$(basename "$dir")/src/x.rs")
    case " $got " in
      *" -p $name "*) ;;
      *) unmapped="$unmapped $(basename "$dir")->'$got'" ;;
    esac
  done
  if [ -z "$unmapped" ]; then
    ok "--fast maps each crates/ directory to its manifest's package name"
  else
    bad "--fast maps each crates/ directory to its manifest's package name" "got:$unmapped"
  fi
  got=$(real fast_packages crates/oska-syntax/src/lib.rs | sorted)
  if [ "$got" = "oska-check oska-eval oska-native oska-run oska-syntax oskac " ]; then
    ok "--fast on an oska-syntax change reaches every crate that depends on it"
  else
    bad "--fast on an oska-syntax change reaches every crate that depends on it" "got: $got"
  fi
  got=$(real fast_crates crates/oska-check/src/lib.rs)
  if [ "$got" = --all ]; then
    ok "--fast on an oska-check change runs the full test command"
  else
    bad "--fast on an oska-check change runs the full test command" "got: $got"
  fi
  got=$(real fast_crates crates/oskac/src/main.rs)
  if [ "$got" = "-p oskac" ]; then
    ok "--fast on an oskac change tests oskac alone"
  else
    bad "--fast on an oskac change tests oskac alone" "got: $got"
  fi
else
  ok "the real workspace's crate graph: skipped (no cargo or jq here; CI runs it)"
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

# --- --docs-tests needs --cargo -----------------------------------------------
cp "$tree/scripts/check.sh" "$work/check.sh.orig"
sed -i 's/^if step "installer tests" /if step "installer tests" --docs-tests /' "$tree/scripts/check.sh"
check --list
cp "$work/check.sh.orig" "$tree/scripts/check.sh"
if [ "$status" -eq 2 ] && grep -q -- '--docs-tests needs --cargo' <<<"$out"; then
  ok "a step tagged --docs-tests without --cargo is a usage error"
else
  bad "a step tagged --docs-tests without --cargo is a usage error" "exit $status: $out"
fi

# --- an unknown argument is a usage error -----------------------------------
check --bogus
if [ "$status" -eq 2 ] && [ -z "$log" ]; then
  ok "an unknown argument exits 2 and runs nothing"
else
  bad "an unknown argument exits 2 and runs nothing" "exit $status, ran: $log"
fi

# --- --group splits the steps in four, with none lost -----------------------
check --list
all_steps=$(printf '%s\n' "$out" | sort)
group_steps() { # GROUP: the steps of that group, in order
  check --list --group "$1"
  printf '%s\n' "$out"
}
tests_steps=$(group_steps tests)
formal_steps=$(group_steps formal)
lint_steps=$(group_steps lint)
scripts_steps=$(group_steps scripts)
checks_steps=$(group_steps checks)
both=$(printf '%s\n%s\n%s\n%s\n' "$tests_steps" "$formal_steps" "$lint_steps" "$scripts_steps" | sed '/^$/d' | sort)
if [ "$both" = "$all_steps" ]; then
  ok "--group tests, formal, lint and scripts together list every step, each once"
else
  bad "--group tests, formal, lint and scripts together list every step, each once" "all: $all_steps; groups: $both"
fi
if [ "$tests_steps" = "cargo test (private temporary directory)" ]; then
  ok "--group tests is the cargo test step"
else
  bad "--group tests is the cargo test step" "got: $tests_steps"
fi
if [ "$formal_steps" = $'formal\nformal step tests' ]; then
  ok "--group formal is the formal step and its tests"
else
  bad "--group formal is the formal step and its tests" "got: $formal_steps"
fi
for step_name in "cargo fmt" "cargo clippy" "oskac type-checks for Windows" "shellcheck" "claude plugin validate" "workflow validation"; do
  if printf '%s\n' "$lint_steps" | grep -qxF "$step_name"; then
    ok "--group lint has the $step_name step"
  else
    bad "--group lint has the $step_name step" "got: $lint_steps"
  fi
done
for step_name in "installer tests" "release script tests" "test shard tests" "eval task set"; do
  if printf '%s\n' "$scripts_steps" | grep -qxF "$step_name"; then
    ok "--group scripts has the $step_name step"
  else
    bad "--group scripts has the $step_name step" "got: $scripts_steps"
  fi
done
union=$(printf '%s\n%s\n%s\n' "$formal_steps" "$lint_steps" "$scripts_steps" | sed '/^$/d' | sort)
if [ "$(printf '%s\n' "$checks_steps" | sort)" = "$union" ]; then
  ok "--group checks is formal, lint and scripts together"
else
  bad "--group checks is formal, lint and scripts together" "got: $checks_steps"
fi
# A group run is not the full check: it writes no pass stamp.
for group in formal lint scripts checks; do
  check --group "$group"
  if [ "$status" -eq 0 ] && ! printf '%s\n' "$out" | grep -q '^\[stamp write' &&
    printf '%s\n' "$out" | grep -q "Group $group passed. This was NOT the full check"; then
    ok "--group $group writes no pass stamp"
  else
    bad "--group $group writes no pass stamp" "exit $status: $out"
  fi
done
# The formal step may skip a missing Lean or TLC (run.sh exits 3) outside CI,
# and must fail in CI, also through --group formal (ADRs 0018, 0019).
formal_run=$tree/scripts/formal/run.sh
cp "$formal_run" "$work/formal-run.sh.orig"
printf '#!/usr/bin/env bash\nexit 3\n' >"$formal_run"
set +e
out=$(cd "$tree" && env -u CI -u GITHUB_ACTIONS PATH="$tree/bin:$PATH" STUB_LOG="$work/log" scripts/check.sh --group formal 2>&1)
status=$?
set -e
if [ "$status" -eq 0 ]; then
  ok "--group formal skips a missing Lean or TLC outside CI"
else
  bad "--group formal skips a missing Lean or TLC outside CI" "exit $status: $out"
fi
set +e
out=$(cd "$tree" && env -u CI -u GITHUB_ACTIONS GITHUB_ACTIONS=true PATH="$tree/bin:$PATH" STUB_LOG="$work/log" scripts/check.sh --group formal 2>&1)
status=$?
set -e
if [ "$status" -ne 0 ] && printf '%s\n' "$out" | grep -q 'CI must run the formal step'; then
  ok "--group formal fails, never skips, a missing Lean or TLC in CI"
else
  bad "--group formal fails, never skips, a missing Lean or TLC in CI" "exit $status: $out"
fi
cp "$work/formal-run.sh.orig" "$formal_run"
check --group nonsense
if [ "$status" -eq 2 ] && [ -z "$log" ]; then
  ok "an unknown group exits 2 and runs nothing"
else
  bad "an unknown group exits 2 and runs nothing" "exit $status, ran: $log"
fi

# ci.yml's test matrix names exactly the groups check.sh has, so a row cannot
# name a group that does not exist and no group goes unrun in CI (spec #712).
have=$(sed -n 's/^ *- {os: [^,]*, group: \([a-z]*\),.*/\1/p' "$repo_root/.github/workflows/ci.yml" | sort -u | tr '\n' ' ')
if [ "$have" = "formal lint scripts tests " ]; then
  ok "ci.yml's test matrix runs the groups formal, lint, scripts and tests"
else
  bad "ci.yml's test matrix runs the groups formal, lint, scripts and tests" "matrix groups: $have"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
