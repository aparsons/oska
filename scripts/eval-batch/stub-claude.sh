#!/usr/bin/env bash
# A stand-in for the `claude` CLI, for the driver's tests: test.sh installs
# it on PATH as `claude`. It calls no model. It replays a recorded
# transcript and leaves a recorded final workspace behind, as a real run
# would, so every step after the agent sees real files.
#
#   claude --version         prints a version
#   claude -p ... <prompt    appends its arguments, working directory and
#                            prompt to $STUB_LOG; sleeps $STUB_SLEEP seconds
#                            if set; copies $STUB_SOLUTION/ (if set) into the
#                            working directory; prints $STUB_TRANSCRIPT with
#                            @WORKSPACE@ replaced by the working directory;
#                            exits with $STUB_EXIT (default 0).
set -euo pipefail

if [ "${1:-}" = --version ]; then
  echo "9.9.9 (Claude Code)"
  exit 0
fi

{
  echo "call"
  printf 'cwd %s\n' "$PWD"
  for arg in "$@"; do
    printf 'arg %s\n' "$arg"
  done
  echo "prompt:"
  cat
  echo
} >>"$STUB_LOG"

if [ -n "${STUB_SLEEP:-}" ]; then
  sleep "$STUB_SLEEP"
fi
if [ -n "${STUB_SOLUTION:-}" ]; then
  cp -R "$STUB_SOLUTION/." .
fi
while IFS= read -r line || [ -n "$line" ]; do
  printf '%s\n' "${line//@WORKSPACE@/$PWD}"
done <"$STUB_TRANSCRIPT"
exit "${STUB_EXIT:-0}"
