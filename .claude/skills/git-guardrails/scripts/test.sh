#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# shellcheck disable=SC2016  # single quotes hold command text on purpose: it must not expand here
# Tests for block-dangerous-git.sh, the git-guardrails PreToolUse hook.
#
#   test.sh [path-to-guard]
#
# With no argument it tests the guard next to this file. Each case feeds the
# guard the JSON that Claude Code sends for a Bash call and checks the result:
#   allowed: exit 0, no output;
#   blocked: exit 2, a "git-guardrails: blocked" reason on stderr, no stdout.
# It tests copies of the guard with POLICY set to standard and to strict, so
# the policy in the installed file does not change the results. Extra patterns
# in the installed file stay; if one blocks an "allowed" case on purpose,
# that case fails and says so.
# Exit 0 when every case passes; exit 1 on any mismatch.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard=${1:-$here/block-dangerous-git.sh}

if [ ! -f "$guard" ]; then
  printf 'test.sh: no guard script at %s\n' "$guard" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  printf 'test.sh: jq is not installed; the tests need it to make the JSON input\n' >&2
  exit 1
fi

tmp=$(mktemp -d)
chmod 700 "$tmp"
trap 'rm -rf "$tmp"' EXIT

make_copy() {
  local policy=$1 out=$2
  sed "s/^POLICY=.*/POLICY=\"$policy\"/" "$guard" >"$out"
  chmod 600 "$out"
  if ! grep -q "^POLICY=\"$policy\"\$" "$out"; then
    printf 'test.sh: cannot find the POLICY= line in %s\n' "$guard" >&2
    exit 1
  fi
}

standard="$tmp/standard.sh"
strict="$tmp/strict.sh"
bogus="$tmp/bogus.sh"
extra="$tmp/extra.sh"
make_copy standard "$standard"
make_copy strict "$strict"
sed 's/^POLICY=.*/POLICY="lenient"/' "$guard" >"$bogus"
sed "s/^EXTRA_PATTERNS=(\$/EXTRA_PATTERNS=('^git rebase( |\$)'/" "$standard" >"$extra"
if ! grep -q "^EXTRA_PATTERNS=('" "$extra"; then
  printf 'test.sh: cannot find the EXTRA_PATTERNS=( line in %s\n' "$guard" >&2
  exit 1
fi

passed=0
failed=0

# The hook JSON for a Bash command.
bash_json() {
  jq -cn --arg c "$1" '{session_id: "test", hook_event_name: "PreToolUse",
    tool_name: "Bash", tool_input: {command: $c, description: "test"}}'
}

# check NAME WANT GUARD JSON [PATH]  (WANT is 0 for allowed, 2 for blocked)
check() {
  local name=$1 want=$2 g=$3 json=$4 path=${5:-$PATH} out err status=0
  out=$(PATH=$path "$BASH" "$g" <<<"$json" 2>"$tmp/err") || status=$?
  err=$(<"$tmp/err")
  if [ "$want" -eq 0 ]; then
    if [ "$status" -eq 0 ] && [ -z "$out" ] && [ -z "$err" ]; then
      passed=$((passed + 1))
      return 0
    fi
  else
    if [ "$status" -eq 2 ] && [ -z "$out" ] && [[ $err == *'git-guardrails: blocked'* ]]; then
      passed=$((passed + 1))
      return 0
    fi
  fi
  failed=$((failed + 1))
  printf 'FAIL [%s] want exit %s, got %s\n  stdout: %s\n  stderr: %s\n' \
    "$name" "$want" "$status" "$out" "$err" >&2
}

allow() { check "allow: $1" 0 "$standard" "$(bash_json "$1")"; }
deny() { check "block: $1" 2 "$standard" "$(bash_json "$1")"; }

# --- allowed in the standard policy -----------------------------------------
allow 'git status'
allow 'git push origin feature/x'
allow 'git push -u origin feature/x'
allow 'git push origin HEAD:feature/x'
allow 'git push --force-if-includes origin feature/x'
allow 'git push origin docs-v2'
allow 'git push origin feature/v1-notes'
# Quoted text is one word, not a command: these messages are allowed.
allow 'git commit -m "reset --hard docs"'
allow 'git commit -m "docs: never use git push --force"'
allow "git commit -m 'git clean -fdx is dangerous'"
allow 'git commit -mnote'
allow 'echo "git reset --hard"'
allow "echo 'a \$(git reset --hard) b'"
allow $'git commit -m "$(cat <<\'EOF\'\nDon\'t reset --hard here; git push --force is bad.\nEOF\n)"'
allow 'git log --oneline | head -5'
allow 'git -C repo status'
allow 'git --no-pager diff'
allow 'git checkout -b feature/y'
allow 'git checkout src/main.rs'
allow 'git checkout -- src/main.rs'
allow 'git restore src/main.rs'
allow 'git restore --staged .'
allow 'git branch -d old'
allow 'git clean -n'
allow 'git stash'
allow 'git stash pop'
allow 'git reflog'
allow 'git gc'
allow 'git reset --soft HEAD~1'
allow 'grep -rn -- --no-verify docs'
allow 'ls | grep git'
allow 'cargo test # git reset --hard'
allow ''

# --- blocked in every policy -------------------------------------------------
deny 'git push --force'
deny 'git push --force origin feature/x'
deny 'git push -f'
deny 'git -C repo push -f'
deny 'git push -fu origin feature/x'
deny 'git push --force-with-lease'
deny 'git push --force-with-lease=feature/x origin feature/x'
deny 'git push --mirror'
deny 'git push --delete origin feature/x'
deny 'git push origin -d feature/x'
deny 'git push origin +feature/x'
deny 'git push origin :feature/x'
deny 'git push origin main'
deny 'git push origin master'
deny 'git push origin HEAD:main'
deny 'git push origin feature/x:refs/heads/main'
deny 'git push origin "$BRANCH"'
# Release tags are protected by default.
deny 'git push origin v0.4.0'
deny 'git push origin refs/tags/v1.0.0'
deny 'git push origin tag v1.0.0'
deny 'git push origin HEAD:refs/tags/v2.0.0'
deny 'git push --tags'
deny 'git push --follow-tags origin feature/x'
deny "git push origin 'refs/tags/*:refs/tags/*'"
deny 'FOO=1 git reset --hard'
deny 'git reset --hard HEAD~1'
deny 'sudo -u root git reset --hard'
deny 'env FOO=1 BAR=2 git reset --hard'
deny 'command git reset --hard'
deny 'exec git reset --hard'
deny 'timeout 10 git reset --hard'
deny 'git --no-pager reset --hard'
deny 'git -c user.name=x reset --hard'
deny 'git --git-dir=.git --work-tree=. reset --hard'
deny '/usr/bin/git clean -fdx'
deny 'git clean -f'
deny 'git clean --force -d'
deny 'echo ok && git branch -D x'
deny 'git branch --delete --force x'
deny 'git branch -df x'
deny 'git checkout -- .'
deny 'git checkout .'
deny 'git checkout HEAD -- .'
deny 'git checkout -f'
deny 'git restore .'
deny 'git restore --staged --worktree .'
deny 'git restore -SW :/'
deny 'git stash drop'
deny 'git stash clear'
deny 'git reflog expire --expire=now --all'
deny 'git gc --prune=now'
deny 'git update-ref -d refs/heads/x'
deny 'git filter-branch --tree-filter "rm x" HEAD'
deny 'git commit --no-verify -m x'
deny 'git commit -n -m x'
deny 'git commit -anm x'
deny 'git push --no-verify origin feature/x'
deny 'git -c core.hooksPath=/dev/null commit -m x'
deny "git -c alias.nuke='reset --hard' nuke"
# Splitting the command line.
deny 'true; git reset --hard'
deny 'false || git clean -fdx'
deny 'git status | git reset --hard'
deny $'cd repo\ngit reset --hard'
deny 'make & git reset --hard'
deny '(git reset --hard)'
deny '{ git reset --hard; }'
deny 'if true; then git reset --hard; fi'
deny 'echo $(git reset --hard)'
deny 'echo "$(git reset --hard)"'
deny 'echo "`git reset --hard`"'
deny 'git commit -m "$(git push --force)"'
deny 'git   reset    --hard'
deny $'git reset \\\n --hard'
# Nested shells.
deny 'bash -c "git reset --hard"'
deny "sh -c 'git push -f'"
deny "bash -lc 'cd x && git clean -fdx'"
deny 'eval "git reset --hard"'
deny "env -S 'git reset --hard'"
deny $'bash <<\'EOF\'\ngit reset --hard\nEOF'
deny $'cat <<EOF\n$(git reset --hard)\nEOF'
deny 'bash -c "bash -c \"bash -c \\\"bash -c true\\\"\""'

# --- strict policy -----------------------------------------------------------
check 'strict: git push origin feature/x' 2 "$strict" "$(bash_json 'git push origin feature/x')"
check 'strict: git push' 2 "$strict" "$(bash_json 'git push')"
check 'strict: git status' 0 "$strict" "$(bash_json 'git status')"
check 'strict: git reset --hard' 2 "$strict" "$(bash_json 'git reset --hard')"

# --- no protected tags --------------------------------------------------------
notags="$tmp/notags.sh"
sed "s/^PROTECTED_TAGS=.*/PROTECTED_TAGS=()/" "$standard" >"$notags"
if ! grep -q '^PROTECTED_TAGS=()$' "$notags"; then
  printf 'test.sh: cannot find the PROTECTED_TAGS= line in %s\n' "$guard" >&2
  exit 1
fi
check 'no tags: git push origin v0.4.0' 0 "$notags" "$(bash_json 'git push origin v0.4.0')"
check 'no tags: git push --tags' 0 "$notags" "$(bash_json 'git push --tags')"

# --- extra patterns ----------------------------------------------------------
check 'extra: git rebase -i main' 2 "$extra" "$(bash_json 'git rebase -i main')"
check 'extra: git -C x rebase main' 2 "$extra" "$(bash_json 'git -C x rebase main')"
check 'extra: git status' 0 "$extra" "$(bash_json 'git status')"

# --- fail closed -------------------------------------------------------------
check 'fail closed: invalid JSON' 2 "$standard" '{not json'
check 'fail closed: empty input' 2 "$standard" ''
check 'fail closed: missing command field' 2 "$standard" '{"tool_name":"Bash","tool_input":{}}'
check 'fail closed: missing tool_input' 2 "$standard" '{"tool_name":"Bash"}'
check 'fail closed: null command' 2 "$standard" '{"tool_name":"Bash","tool_input":{"command":null}}'
check 'fail closed: number command' 2 "$standard" '{"tool_name":"Bash","tool_input":{"command":42}}'
check 'fail closed: unclosed double quote' 2 "$standard" "$(bash_json 'git commit -m "oops')"
check 'fail closed: unclosed single quote' 2 "$standard" "$(bash_json "echo 'oops")"
check 'fail closed: unclosed substitution' 2 "$standard" "$(bash_json 'echo $(git status')"
check 'fail closed: unknown POLICY' 2 "$bogus" "$(bash_json 'git status')"
mkdir "$tmp/empty-path"
check 'fail closed: jq missing' 2 "$standard" "$(bash_json 'git status')" "$tmp/empty-path"

total=$((passed + failed))
if [ "$failed" -ne 0 ]; then
  printf '%s of %s cases failed\n' "$failed" "$total" >&2
  exit 1
fi
printf 'all %s cases passed\n' "$total"
