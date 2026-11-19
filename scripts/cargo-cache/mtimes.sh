#!/usr/bin/env bash
# Makes a `target` directory restored from a CI cache usable by cargo,
# without ever letting it use a stale build.
#
#   scripts/cargo-cache/mtimes.sh [TARGET-DIR]
#
# Run it in the repository root after the cache is restored and before the
# first cargo command. TARGET-DIR defaults to `target`.
#
# Cargo decides whether a workspace crate is fresh from file times: a source
# newer than the crate's last build makes it build again. A checkout gives
# every file the current time, so a restored `target` would be rebuilt in
# full. Cargo's content-based check (`-Z checksum-freshness`) is unstable.
#
# So the script compares the tracked files with the ones the cached build
# came from, by git blob ID. It writes that list as
# TARGET-DIR/.cargo-cache-sources, which is cached with the build. A file
# whose path and blob ID are both in that list gets an old fixed time
# (2000-01-01), older than any cached output, so cargo can reuse what was
# built from it. Every other file keeps the checkout's time, newer than any
# cached output, so cargo builds again what reads it. A symbolic link, or
# a path git quotes (one with a tab, a newline or a quote), never gets the
# old time. With no list (no cache, or a cache from before this script),
# it changes no time.
#
# Then it writes the list for the current tree, for the next run's cache.

set -euo pipefail

target=${1:-target}
list="$target/.cargo-cache-sources"

current=$(mktemp)
trap 'rm -f "$current" "$current.old"' EXIT

git -c core.quotePath=false ls-files -s | LC_ALL=C sort >"$current"

if [ -f "$list" ]; then
  LC_ALL=C sort "$list" >"$current.old"
  # Lines in both lists: same mode, blob ID and path. Only regular files:
  # `touch` follows a symbolic link, and would give its target, which may
  # have changed, the old time. `cut` keeps the path after the tab; a
  # quoted path starts with `"` and is left alone.
  LC_ALL=C comm -12 "$current.old" "$current" |
    { grep -E '^100(644|755) ' || true; } |
    cut -f2- |
    { grep -v '^"' || true; } |
    tr '\n' '\0' |
    xargs -0 sh -c '[ "$#" -eq 0 ] || touch -t 200001010000 "$@"' sh
  kept=$(LC_ALL=C comm -12 "$current.old" "$current" | wc -l | tr -d ' ')
  total=$(wc -l <"$current" | tr -d ' ')
  printf 'cargo-cache: %s of %s tracked files unchanged since the cached build\n' "$kept" "$total"
else
  printf 'cargo-cache: no cached build; no file times changed\n'
fi

mkdir -p "$target"
cp "$current" "$list"
