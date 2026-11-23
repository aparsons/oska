#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Makes a `target` directory restored from a CI cache usable by cargo,
# without ever letting it use a stale build.
#
#   scripts/cargo-cache/mtimes.sh [TARGET-DIR]
#   scripts/cargo-cache/mtimes.sh verify [TARGET-DIR]
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
# The list stands for the build only if the job builds every unit with the
# tree as checked out: CI runs every cargo step, and changes no tracked
# file. `verify` checks the second before the cache is saved:
#
#   scripts/cargo-cache/mtimes.sh verify [TARGET-DIR]
#
# exits 1, saying why, if a tracked file changed since the checkout or the
# list does not name the current tree.

set -euo pipefail

mode=stamp
if [ "${1:-}" = verify ]; then
  mode=verify
  shift
fi
target=${1:-target}
list="$target/.cargo-cache-sources"

if [ "$mode" = verify ]; then
  if ! git diff --quiet HEAD --; then
    printf 'cargo-cache: a tracked file changed during the job; do not save the cache\n' >&2
    exit 1
  fi
  if ! git -c core.quotePath=false ls-files -s | LC_ALL=C sort | cmp -s - "$list"; then
    printf 'cargo-cache: %s does not name the current tree; do not save the cache\n' "$list" >&2
    exit 1
  fi
  printf 'cargo-cache: the tree is as checked out; the cache may be saved\n'
  exit 0
fi

current=$(mktemp)
trap 'rm -f "$current" "$current.old"' EXIT

git -c core.quotePath=false ls-files -s | LC_ALL=C sort >"$current"

# A symbolic link that changed (or appeared, or went) could point a build
# at another file with the old time; cargo follows links. Then no file
# gets the old time.
links_changed() {
  ! cmp -s <(grep -E '^120000 ' "$current.old" || true) <(grep -E '^120000 ' "$current" || true)
}

if [ -f "$list" ]; then
  LC_ALL=C sort "$list" >"$current.old"
fi

if [ -f "$list" ] && links_changed; then
  printf 'cargo-cache: a symbolic link changed; no file times changed\n'
elif [ -f "$list" ]; then
  # Lines in both lists: same mode, blob ID and path. Only regular files:
  # `touch` follows a symbolic link, and would give its target, which may
  # have changed, the old time. `cut` keeps the path after the tab; a
  # quoted path starts with `"` and is left alone.
  LC_ALL=C comm -12 "$current.old" "$current" |
    { grep -E '^100(644|755) ' || true; } |
    cut -f2- |
    { grep -v '^"' || true; } |
    tr '\n' '\0' |
    xargs -0 sh -c '[ "$#" -eq 0 ] || touch -t 200001010000 -- "$@"' sh
  kept=$(LC_ALL=C comm -12 "$current.old" "$current" | wc -l | tr -d ' ')
  total=$(wc -l <"$current" | tr -d ' ')
  printf 'cargo-cache: %s of %s tracked files unchanged since the cached build\n' "$kept" "$total"
else
  printf 'cargo-cache: no cached build; no file times changed\n'
fi

mkdir -p "$target"
cp "$current" "$list"
