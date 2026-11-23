# SPDX-License-Identifier: Apache-2.0
# shellcheck shell=bash
# Takes one of OSKA_BUILD_SLOTS machine-wide build slots. check.sh sources
# this file (`. scripts/build-slots/acquire.sh`) right before its build and
# test steps, so the slot is held by check.sh's own process for the rest of
# the run, and the kernel releases it when that process ends, however it ends.
#
# OSKA_BUILD_SLOTS unset or empty: no limit, no lock files; nothing changes
# (CI and other clones). A positive integer N: N lock files under
# oska-build-slots-UID in $XDG_RUNTIME_DIR (else $TMPDIR, else /tmp), shared
# by every clone and project on the machine that sets the variable. The run
# tries each slot with a non-blocking `flock`, prints one line if all are
# taken, and retries every OSKA_BUILD_SLOTS_POLL seconds (default 5). Without
# a `flock` binary it runs unlimited and says so once. Any other value is an
# error (exit 2). Overloading the machine makes every check slower, not
# faster; this keeps each one near its idle time.

_slots=${OSKA_BUILD_SLOTS:-}
if [ -n "$_slots" ]; then
  case $_slots in
    0* | *[!0-9]*)
      echo "check.sh: OSKA_BUILD_SLOTS must be a positive integer, got '$_slots'" >&2
      exit 2
      ;;
  esac
  if ! command -v flock >/dev/null 2>&1; then
    echo "check.sh: flock not found; running without a build-slot limit" >&2
  else
    _dir=${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/oska-build-slots-$(id -u)
    (umask 077 && mkdir -p "$_dir")
    _said=
    while :; do
      _i=1
      while [ "$_i" -le "$_slots" ]; do
        exec {_fd}>>"$_dir/slot-$_i"
        if flock -n "$_fd"; then break 2; fi
        exec {_fd}>&-
        _i=$((_i + 1))
      done
      if [ -z "$_said" ]; then
        echo "check.sh: waiting for a free build slot (OSKA_BUILD_SLOTS=$_slots)" >&2
        _said=1
      fi
      sleep "${OSKA_BUILD_SLOTS_POLL:-5}"
    done
    unset _dir _said _i
  fi
fi
unset _slots
