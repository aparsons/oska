#!/usr/bin/env bash
# Tests for the fast pre-commit hook (pre-commit.sh in the setup-pre-commit
# skill, installed as .githooks/pre-commit).
#
#   test.sh [path-to-hook]
#
# With no argument it tests pre-commit.sh next to this file if there is one,
# else .githooks/pre-commit of the repo that holds this file. Each case runs
# the hook in a new temporary git repo, with stub `cargo` and `shellcheck`
# commands on a PATH that holds only them and git, so nothing builds.
# Exit 0 when every case passes; exit 1 on any failure.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
if [ $# -gt 0 ]; then
  hook=$1
elif [ -f "$here/pre-commit.sh" ]; then
  hook="$here/pre-commit.sh"
else
  hook="$(git -C "$here" rev-parse --show-toplevel)/.githooks/pre-commit"
fi
if [ ! -f "$hook" ]; then
  printf 'test.sh: no hook at %s\n' "$hook" >&2
  exit 1
fi
hook=$(cd "$(dirname "$hook")" && pwd)/$(basename "$hook")

tmp=$(mktemp -d)
chmod 700 "$tmp"
trap 'rm -rf "$tmp"' EXIT

# PATH parts: git alone, a stub cargo, a stub shellcheck. Each stub appends
# its arguments to $STUB_LOG and exits with the code its variable names.
mkdir "$tmp/git" "$tmp/cargo" "$tmp/shellcheck"
ln -s "$(command -v git)" "$tmp/git/git"
# shellcheck disable=SC2016  # the stubs expand these when they run
printf '#!%s\nprintf "cargo %%s\\n" "$*" >>"$STUB_LOG"\nexit "${STUB_CARGO_EXIT:-0}"\n' "$BASH" >"$tmp/cargo/cargo"
# shellcheck disable=SC2016  # the stubs expand these when they run
printf '#!%s\nprintf "shellcheck %%s\\n" "$*" >>"$STUB_LOG"\nexit "${STUB_SC_EXIT:-0}"\n' "$BASH" >"$tmp/shellcheck/shellcheck"
chmod 700 "$tmp/cargo/cargo" "$tmp/shellcheck/shellcheck"
all_path="$tmp/git:$tmp/cargo:$tmp/shellcheck"

passed=0
failed=0

fail() {
  failed=$((failed + 1))
  printf 'FAIL [%s] %s\n' "$1" "$2" >&2
}

# new_repo: make a fresh repo with one committed file; print its path.
new_repo() {
  local r
  r=$(mktemp -d "$tmp/repo.XXXXXX")
  git -C "$r" init -q
  git -C "$r" config user.email test@example.invalid
  git -C "$r" config user.name test
  git -C "$r" config core.hooksPath /dev/null
  printf 'old\n' >"$r/old.sh"
  git -C "$r" add old.sh
  git -C "$r" commit -qm init >/dev/null
  printf '%s\n' "$r"
}

# run_hook REPO PATH: run the hook; sets status, out (stdout+stderr), log.
run_hook() {
  local r=$1 path=$2
  : >"$tmp/log"
  status=0
  out=$(cd "$r" && PATH=$path STUB_LOG="$tmp/log" OSKA_TEST_SECRET=hunter2-planted "$BASH" "$hook" 2>&1) || status=$?
  log=$(<"$tmp/log")
}

expect() {
  local name=$1 want=$2
  if [ "$status" -ne "$want" ]; then
    fail "$name" "exit $status, want $want; output: $out"
    return 1
  fi
  if [[ $out == *hunter2-planted* ]]; then
    fail "$name" "the hook printed an environment variable"
    return 1
  fi
  passed=$((passed + 1))
}

# 1. Nothing staged, format clean: passes, runs only cargo fmt --check.
r=$(new_repo)
STUB_CARGO_EXIT=0 run_hook "$r" "$all_path"
if expect 'clean tree passes' 0; then
  if [ "$log" != 'cargo fmt --all --check' ]; then
    passed=$((passed - 1))
    fail 'clean tree passes' "want only 'cargo fmt --all --check', got: $log"
  fi
fi

# 2. Unformatted code: refused.
r=$(new_repo)
export STUB_CARGO_EXIT=1
run_hook "$r" "$all_path"
expect 'cargo fmt failure refuses' 1 || true
export STUB_CARGO_EXIT=0

# 3. No cargo: refused (fail closed), nothing else runs.
r=$(new_repo)
printf 'x\n' >"$r/new.sh"
git -C "$r" add new.sh
run_hook "$r" "$tmp/git:$tmp/shellcheck"
if expect 'missing cargo refuses' 1 && [ -n "$log" ]; then
  passed=$((passed - 1))
  fail 'missing cargo refuses' "a tool ran: $log"
fi

# 4. Staged scripts go to shellcheck, names with spaces too; a staged
#    deletion and an unstaged script do not.
r=$(new_repo)
printf 'x\n' >"$r/a b.sh"
printf 'x\n' >"$r/c.sh"
printf 'x\n' >"$r/unstaged.sh"
printf 'x\n' >"$r/notes.txt"
git -C "$r" add 'a b.sh' c.sh notes.txt
git -C "$r" rm -q old.sh
run_hook "$r" "$all_path"
if expect 'staged scripts checked' 0; then
  if [ "$log" != $'cargo fmt --all --check\nshellcheck -- a b.sh c.sh' ]; then
    passed=$((passed - 1))
    fail 'staged scripts checked' "unexpected tool calls: $log"
  fi
fi

# 5. A shellcheck finding: refused.
r=$(new_repo)
printf 'x\n' >"$r/new.sh"
git -C "$r" add new.sh
export STUB_SC_EXIT=1
run_hook "$r" "$all_path"
expect 'shellcheck finding refuses' 1 || true
export STUB_SC_EXIT=0

# 6. No shellcheck: passes with a note, and the format check still ran.
r=$(new_repo)
printf 'x\n' >"$r/new.sh"
git -C "$r" add new.sh
run_hook "$r" "$tmp/git:$tmp/cargo"
if expect 'missing shellcheck is a note' 0; then
  if [[ $out != *'shellcheck is not installed'* ]] || [ "$log" != 'cargo fmt --all --check' ]; then
    passed=$((passed - 1))
    fail 'missing shellcheck is a note' "output: $out; log: $log"
  fi
fi

# 7. Not in a git repo: refused.
mkdir "$tmp/norepo"
run_hook "$tmp/norepo" "$all_path"
if [ "$status" -eq 0 ]; then
  fail 'outside a repo refuses' 'exit 0'
else
  passed=$((passed + 1))
fi

total=$((passed + failed))
if [ "$failed" -ne 0 ]; then
  printf '%s of %s cases failed\n' "$failed" "$total" >&2
  exit 1
fi
printf 'all %s cases passed\n' "$total"
