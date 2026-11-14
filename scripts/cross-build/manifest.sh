#!/usr/bin/env bash
# Seam 3 of spec #153: build every corpus program for every target platform
# with a backend, and print one line per build, the SHA-256 of the
# executable or the exit code that refused it:
#
#   scripts/cross-build/manifest.sh OSKAC > manifest.txt
#
# OSKAC is the `oskac` binary to build with. The corpus is the examples,
# the CLI's fixture programs and packages, and the site generator. CI's
# `cross-build` job runs this on an aarch64 runner and an x86_64 runner and
# requires the two manifests to be identical: the backend's output depends
# only on the program and the target, never on the host.

set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: scripts/cross-build/manifest.sh OSKAC" >&2
  exit 2
fi
oskac=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

targets=(linux-aarch64 linux-x86_64)

# Every corpus program, as a path relative to the repository: a source
# file, or a package directory.
corpus() {
  cd "$root"
  for file in examples/*.osk crates/oskac/tests/fixtures/*.osk site/build.osk; do
    printf '%s\n' "$file"
  done
  for manifest in examples/*/oska.toml crates/oskac/tests/fixtures/packages/*/oska.toml; do
    dirname "$manifest"
  done
}

count=0
while IFS= read -r program; do
  for target in "${targets[@]}"; do
    out="$work/exe"
    rm -f "$out"
    # Built from the repository root, by its relative path, so a panic
    # site's path is the same on every host.
    if (cd "$root" && "$oskac" build "$program" --target "$target" -o "$out" >/dev/null 2>&1); then
      hash=$(sha256sum "$out" | cut -d' ' -f1)
      count=$((count + 1))
    else
      hash="refused:$?"
    fi
    printf '%s %s %s\n' "$target" "$program" "$hash"
  done
done < <(corpus | LC_ALL=C sort)

if [ "$count" -eq 0 ]; then
  echo "manifest.sh: no corpus program built" >&2
  exit 1
fi
