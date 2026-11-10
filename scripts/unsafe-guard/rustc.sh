#!/usr/bin/env bash
# The rustc wrapper behind guard.sh's compile check. Cargo runs it, as
# RUSTC_WORKSPACE_WRAPPER, for every workspace crate:
#
#   rustc.sh <rustc> <rustc arguments...>
#
# It compiles every crate but oska-sys with `-F unsafe_code` last on the
# command line, after any lint flag that a manifest's `[lints]` table,
# RUSTFLAGS or a cargo config added. A later `-A unsafe_code` cannot
# follow it, and a `#![allow(unsafe_code)]` in source is an error under
# `forbid`, so the crate compiles only if it has no `unsafe` code.
# `--cap-lints` would cap that `forbid` at a warning, so it is refused.

set -euo pipefail

rustc=$1
shift

crate=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--crate-name" ]; then
    crate=$arg
  fi
  previous=$arg
done

case "$crate" in
  # Cargo's own probes of the compiler (`___`, or no crate at all), and
  # the one crate that may use `unsafe` (ADR 0009).
  "" | ___ | oska_sys) exec "$rustc" "$@" ;;
esac

for arg in "$@"; do
  case "$arg" in
    --cap-lints*)
      printf 'unsafe guard: %s is compiled with %s, which would let unsafe code through\n' \
        "$crate" "$arg" >&2
      exit 1
      ;;
  esac
done

exec "$rustc" "$@" -F unsafe_code
