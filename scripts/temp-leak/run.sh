#!/usr/bin/env bash
# Runs a command with a private temporary directory and fails if the command
# leaves anything in it. check.sh runs `cargo test --all` through this, so a
# test that leaks a file or directory fails the check at once, naming what it
# left, instead of slowly filling the shared temporary directory.
#
#   scripts/temp-leak/run.sh COMMAND [ARGS...]
#
# Makes an empty directory under the real temporary directory, runs the
# command with TMPDIR pointing at it, then lists what is inside. If it is
# empty, the directory is removed and the command's exit status is returned.
# If not, every path found is printed, the directory is removed, and the exit
# status is 1 even when the command passed. A failing command keeps its own
# status, and its leftovers are still reported. The directory is removed on
# interrupt too.

set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: run.sh COMMAND [ARGS...]" >&2
  exit 2
fi

private=$(mktemp -d "${TMPDIR:-/tmp}/oska-leakcheck.XXXXXX")
child=""

cleanup() {
  # A leaked read-only directory must not stop its removal.
  chmod -R u+rwx "$private" 2>/dev/null || true
  rm -rf "$private"
}

interrupted() {
  trap - EXIT
  if [ -n "$child" ]; then
    kill -TERM "$child" 2>/dev/null || true
    wait "$child" 2>/dev/null || true
  fi
  cleanup
  exit 130
}

trap cleanup EXIT
trap interrupted INT TERM HUP

# In the background so a signal reaches the trap at once, not after the
# command finishes.
set +e
TMPDIR="$private" "$@" &
child=$!
wait "$child"
status=$?
set -e
child=""

leftovers=$(find "$private" -mindepth 1 -maxdepth 1 -print | sort)
if [ -n "$leftovers" ]; then
  {
    echo "temp-leak: the command left these in its temporary directory (removed now):"
    printf '%s\n' "$leftovers" | sed "s|^$private/|  |"
  } >&2
  if [ "$status" -eq 0 ]; then
    status=1
  fi
fi

exit "$status"
