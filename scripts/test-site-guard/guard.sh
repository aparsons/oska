#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Keeps the `test-site` feature of `oskac` out of releases (spec #230).
#
#   scripts/test-site-guard/guard.sh [--require-binary] [ROOT] [BINARY...]
#
# `test-site` points the update checker at a loopback TLS test server and a
# test key, and reads the site, the root and the key from the environment
# (`crates/oskac/update/test_site.osk`). A release `oskac` must have no such
# way to change what it trusts, so this fails when:
#
#   - `crates/oskac/Cargo.toml` makes `test-site` a default feature, or another
#     feature turn it on;
#   - a tracked file runs `cargo build`, `install`, `publish`, `package` or
#     `rustc` with `--all-features` or with the `test-site` feature (a
#     `cargo check`, `clippy` or `test` may: they ship nothing);
#     (a command continued with a trailing backslash counts as one line);
#   - a BINARY given on the command line holds the feature's marker
#     (`OSKAC_TEST_SITE_`), which only a test build's code mentions.
#
# ROOT is a git work tree (default: the repository this script is in). With
# --require-binary (CI and release runs), having no BINARY to inspect is a
# failure; without it, the guard says plainly that it inspected none.

set -euo pipefail

require=0
if [ "${1:-}" = --require-binary ]; then
  require=1
  shift
fi
root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
[ $# -gt 0 ] && shift
bad=0
cargo_toml=$root/crates/oskac/Cargo.toml

# The features table of oskac, without comments.
features=$(awk '/^\[features\]/ {on = 1; next} /^\[/ {on = 0} on' "$cargo_toml" | sed 's/#.*//')
if ! grep -q '^test-site *= *\[\]' <<<"$features"; then
  printf 'test-site-guard: %s has no test-site = [] feature, or it enables another\n' "$cargo_toml" >&2
  bad=1
fi
# `default` must not name it, and no other feature may enable it.
if grep -E '^default *=' <<<"$features" | grep -q 'test-site'; then
  printf 'test-site-guard: test-site is a default feature of oskac\n' >&2
  bad=1
fi
if grep -v '^test-site' <<<"$features" | grep -q 'test-site'; then
  printf 'test-site-guard: a feature of oskac enables test-site\n' >&2
  bad=1
fi

# Builds that ship something must not turn features on wholesale or this one.
# A command continued with a trailing backslash is one command: its lines are
# joined before they are scanned.
joined() {
  awk '
    { line = $0
      while (line ~ /\\[ \t]*$/ && (getline more) > 0) {
        sub(/\\[ \t]*$/, " ", line)
        line = line more
      }
      print FNR ":" line }
  ' "$1"
}
while IFS= read -r file; do
  case "$file" in
    scripts/test-site-guard/*) continue ;;
  esac
  while IFS= read -r text; do
    text=${text#*:}
    if grep -Eq 'cargo( +\+[^ ]+)? +(build|install|publish|package|rustc|run)( |$)' <<<"$text" &&
      grep -Eq -- '--all-features|test-site|oskac/test' <<<"$text"; then
      printf 'test-site-guard: %s builds with all features or test-site: %s\n' "$file" "$text" >&2
      bad=1
    fi
  done < <(joined "$root/$file")
done < <(git -C "$root" grep -l -I -E -- '--all-features|test-site|oskac/test' -- . || true)

if [ $# -eq 0 ]; then
  if [ "$require" -eq 1 ]; then
    printf 'test-site-guard: no oskac binary to inspect, and this run requires one\n' >&2
    bad=1
  else
    printf 'test-site-guard: NOTE: no oskac binary was inspected (only the sources and build commands)\n' >&2
  fi
fi
for binary in "$@"; do
  if grep -aq 'OSKAC_TEST_SITE_' "$binary"; then
    printf 'test-site-guard: %s was built with the test-site feature\n' "$binary" >&2
    bad=1
  fi
done

[ "$bad" -eq 0 ] || exit 1
printf 'test-site-guard: the test-site feature is kept out of releases\n'
