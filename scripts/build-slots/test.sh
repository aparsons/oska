#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for acquire.sh, the build-slot limiter that check.sh sources. Each
# case starts fake check runs (a few lines that source acquire.sh, log when
# they start and end, and wait for a go-ahead file) and asserts only on the
# order of events in a log and on what the runs print. Nothing depends on
# elapsed time: waits poll for a condition and give up only to fail the test.
#
#   scripts/build-slots/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

work=$(mktemp -d)
pids=()
cleanup() {
  # Only the processes this test started.
  if [ "${#pids[@]}" -gt 0 ]; then
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  fi
  rm -rf "$work"
}
trap cleanup EXIT

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

# The fake check run: take a slot, log "start-NAME", hold it until
# $GO/NAME exists, log "end-NAME".
cat >"$work/fake.sh" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=/dev/null
. "$ACQUIRE"
echo "start-$NAME" >>"$LOG"
while [ ! -e "$GO/$NAME" ]; do sleep 0.05; done
echo "end-$NAME" >>"$LOG"
FAKE

# A new scratch case: sets case_dir, rt (the runtime directory), LOG and GO.
new_case() {
  case_dir=$(mktemp -d "$work/case.XXXXXX")
  rt="$case_dir/rt"
  GO="$case_dir/go"
  LOG="$case_dir/log"
  mkdir "$rt" "$GO"
  : >"$LOG"
}

# start NAME SLOTS: runs a fake check in the background; sets `pid`. SLOTS
# "unset" removes OSKA_BUILD_SLOTS. Output goes to $case_dir/out-NAME.
start() {
  local name=$1 slots=$2
  local -a slot_env=(-u OSKA_BUILD_SLOTS)
  [ "$slots" = unset ] || slot_env=("OSKA_BUILD_SLOTS=$slots")
  env "${slot_env[@]}" PATH="${START_PATH:-$PATH}" XDG_RUNTIME_DIR="$rt" TMPDIR="$rt" OSKA_BUILD_SLOTS_POLL=0.05 \
    ACQUIRE="$here/acquire.sh" NAME="$name" LOG="$LOG" GO="$GO" \
    bash "$work/fake.sh" >"$case_dir/out-$name" 2>&1 &
  pid=$!
  pids+=("$pid")
}

# until_true DESCRIPTION COMMAND...: polls until the command succeeds.
until_true() {
  local what=$1 n=0
  shift
  until "$@"; do
    n=$((n + 1))
    if [ "$n" -gt 600 ]; then
      bad "$what" "gave up waiting"
      return 1
    fi
    sleep 0.05
  done
}
logged() { grep -qx "$1" "$LOG"; }
not_logged() { ! grep -qx "$1" "$LOG"; }
both_logged() { logged start-A && logged start-B; }
said_waiting() { grep -q 'waiting for a free build slot' "$case_dir/out-$1" 2>/dev/null; }
release() { : >"$GO/$1"; }
events() { tr '\n' ' ' <"$LOG"; }

expect_events() { # name expected
  if [ "$(events)" = "$2 " ]; then ok "$1"; else bad "$1" "events '$(events)', expected '$2'"; fi
}

# With one slot, two concurrent runs go one after the other.
new_case
start A 1
pid_a=$pid
until_true "A starts" logged start-A
start B 1
pid_b=$pid
until_true "B says it is waiting" said_waiting B
if not_logged start-B; then ok "B has not started while A holds the only slot"; else bad "B waits for A" "B started"; fi
release A
until_true "B starts after A ends" logged start-B
release B
wait "$pid_a" "$pid_b"
expect_events "one slot serializes two runs" "start-A end-A start-B end-B"
if said_waiting A; then bad "A does not wait" "A printed a waiting line"; else ok "the run that got the slot printed nothing about waiting"; fi
if [ "$(grep -c 'waiting for a free build slot' "$case_dir/out-B")" -eq 1 ]; then ok "the waiting line is printed once"; else bad "waiting line printed once" "$(cat "$case_dir/out-B")"; fi

# With two slots, two runs go at once and a third waits.
new_case
start A 2
pid_a=$pid
start B 2
pid_b=$pid
until_true "A and B start" both_logged
start C 2
pid_c=$pid
until_true "C says it is waiting" said_waiting C
if not_logged start-C; then ok "a third run waits when both slots are taken"; else bad "third run waits" "C started"; fi
release A
until_true "C starts when a slot frees" logged start-C
release B
release C
wait "$pid_a" "$pid_b" "$pid_c"
ok "all three runs finished"

# A run killed mid-way releases its slot.
new_case
start A 1
pid_a=$pid
until_true "A starts" logged start-A
start B 1
pid_b=$pid
until_true "B says it is waiting" said_waiting B
kill -9 "$pid_a"
wait "$pid_a" 2>/dev/null || true
until_true "B starts after A is killed" logged start-B
release B
wait "$pid_b"
ok "a killed run releases its slot"

# Unset or empty means no limit and no lock files.
for setting in unset ""; do
  label=${setting:-empty}
  new_case
  start A "$setting"
  pid_a=$pid
  start B "$setting"
  pid_b=$pid
  until_true "A and B run together when OSKA_BUILD_SLOTS is $label" both_logged
  release A
  release B
  wait "$pid_a" "$pid_b"
  if [ ! -s "$case_dir/out-A" ] && [ ! -s "$case_dir/out-B" ]; then ok "no output and no waiting when OSKA_BUILD_SLOTS is $label"; else bad "no output when $label" "$(cat "$case_dir"/out-*)"; fi
  if [ -z "$(ls -A "$rt")" ]; then ok "no lock files when OSKA_BUILD_SLOTS is $label"; else bad "no lock files when $label" "found $(ls -A "$rt")"; fi
done

# Without flock, the run goes ahead unlimited and says so once.
new_case
nobin="$work/nobin"
mkdir "$nobin"
for tool in bash mkdir sleep; do ln -s "$(command -v "$tool")" "$nobin/$tool"; done
START_PATH="$nobin" start A 1
pid_a=$pid
START_PATH="$nobin" start B 1
pid_b=$pid
until_true "both start without flock" both_logged
release A
release B
wait "$pid_a" "$pid_b"
if [ "$(grep -c 'flock not found' "$case_dir/out-A")" -eq 1 ]; then ok "a missing flock is reported once"; else bad "missing flock reported once" "$(cat "$case_dir/out-A")"; fi
if [ -z "$(ls -A "$rt")" ]; then ok "no lock files without flock"; else bad "no lock files without flock" "found $(ls -A "$rt")"; fi

# A bad value fails the run instead of silently running unlimited.
for bad_value in 0 two -1 1.5; do
  new_case
  release A
  start A "$bad_value"
  status=0
  wait "$pid" || status=$?
  if [ "$status" -eq 2 ] && grep -q 'positive integer' "$case_dir/out-A" && not_logged start-A; then ok "OSKA_BUILD_SLOTS=$bad_value is refused"; else bad "OSKA_BUILD_SLOTS=$bad_value refused" "status $status, output $(cat "$case_dir/out-A")"; fi
done

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
