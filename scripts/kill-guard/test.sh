#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# shellcheck disable=SC2016  # single quotes hold text on purpose: patterns, commands and scripts that must not expand here
# Tests for guard.sh, the Claude Code PreToolUse hook that refuses broad kills.
# Each case feeds the hook the JSON Claude Code sends for a Bash call and
# expects exit 2 with the safe alternative on stderr (refused), or exit 0 with
# no output (allowed).
#
#   scripts/kill-guard/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"

passed=0
failures=0

# The tool-input JSON for a Bash command, escaped the way Claude Code does.
bash_input() {
  local escaped=${1//\\/\\\\}
  escaped=${escaped//\"/\\\"}
  escaped=${escaped//$'\n'/\\n}
  printf '{"session_id":"s1","cwd":"/work","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"%s","description":"a description","run_in_background":false},"tool_use_id":"toolu_1"}' "$escaped"
}

# Expect the hook to refuse the JSON in `input`, naming the safe form.
expect_refused_json() {
  local name=$1 input=$2 out status=0
  out=$("$guard" <<<"$input" 2>&1) || status=$?
  if [ "$status" -ne 2 ]; then
    printf 'FAIL %s: exit %s, expected 2\n' "$name" "$status" >&2
    failures=$((failures + 1))
  elif ! grep -qF -- 'kill <pid>' <<<"$out" || ! grep -qF -- 'pkill -P $$' <<<"$out"; then
    printf 'FAIL %s: the message does not name the safe form:\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

expect_allowed_json() {
  local name=$1 input=$2 out status=0
  out=$("$guard" <<<"$input" 2>&1) || status=$?
  if [ "$status" -ne 0 ] || [ -n "$out" ]; then
    printf 'FAIL %s: exit %s, output "%s"; expected exit 0 and no output\n' "$name" "$status" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

refused() { expect_refused_json "refuse: $1" "$(bash_input "$1")"; }
allowed() { expect_allowed_json "allow: $1" "$(bash_input "$1")"; }

# Kill by pattern, name or session: the exit-144 cause.
refused 'pkill -f "scripts/check.sh"'
refused 'pkill -f temp-leak/run.sh'
refused 'pkill -9 -f cargo'
refused 'pkill --full cargo'
refused 'pkill -TERM -s 4242'
refused 'pkill --session 4242'
refused 'pkill cargo'
refused 'killall cargo'
refused 'killall -9 rustc'
# A refused kill anywhere in a compound command, or behind a wrapper.
refused 'cargo fmt; pkill -f "scripts/check.sh"; true'
refused 'cargo fmt && pkill -f check.sh'
refused 'sleep 1 || pkill cargo'
refused $'echo done\npkill -f check.sh'
refused 'sudo killall cargo'
refused 'timeout 5 pkill -f check.sh'
refused 'FOO=1 pkill -f check.sh'
refused '/usr/bin/pkill -f check.sh'
refused 'bash -c "pkill -f check.sh"'
refused 'echo $(pkill -f check.sh)'
# Scoped to a parent that is not the caller's own shell.
refused 'pkill -P 1 -f check.sh'
# Kill of whatever a name search finds.
refused 'kill $(pgrep -f check.sh)'
refused 'kill -9 `pgrep -f check.sh`'
refused 'pgrep -f check.sh | xargs kill'
refused 'kill $(pidof cargo)'

# The safe forms.
allowed 'kill 1234'
allowed 'kill -TERM 1234'
allowed 'kill -9 4321 4322'
allowed 'kill $!'
allowed 'pkill -P $$'
allowed 'pkill -TERM -P $$'
allowed 'pkill -P "$$" sleep'
allowed 'pkill -P $BASHPID'
allowed 'kill $(pgrep -P $$)'
allowed 'sleep 100 & pid=$!; kill "$pid"'
# Reading, or only mentioning, is not killing.
allowed 'pgrep -f check.sh'
allowed 'cargo test --all'
allowed 'grep -rn pkill docs'
allowed 'git commit -m "Guard: refuse pkill -f and killall"'
allowed 'echo "never run pkill -f"'
allowed 'ls -la'

# Input that is not a Bash command is none of the guard's business.
expect_allowed_json "allow: an Edit call" \
  '{"tool_name":"Edit","tool_input":{"file_path":"/a","old_string":"pkill -f x","new_string":"y"}}'
expect_allowed_json "allow: no input" ''
# A pattern in another field is not a command.
expect_allowed_json "allow: pkill only in the description" \
  '{"tool_name":"Bash","tool_input":{"command":"ls","description":"pkill -f x"}}'
# A Bash call whose command cannot be read is refused if it looks like a kill.
expect_refused_json "refuse: unreadable input with a broad kill" \
  'tool_name Bash: pkill -f check.sh'

if [ "$failures" -ne 0 ]; then
  printf 'kill-guard tests: %s failed, %s passed\n' "$failures" "$passed" >&2
  exit 1
fi
printf 'kill-guard tests: %s passed\n' "$passed"
