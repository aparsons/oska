#!/usr/bin/env bash
# Seam 3 of spec #153: build every corpus program for every target platform
# with a backend, and print one line per build, the SHA-256 of the
# executable or the exit code that refused it:
#
#   scripts/cross-build/manifest.sh OSKAC [repo-root] > manifest.txt
#
# OSKAC is the `oskac` binary to build with. The corpus is the examples,
# the CLI's fixture programs and packages, and the site generator. CI's
# `cross-build` job runs this on an aarch64 runner and an x86_64 runner and
# requires the two manifests to be identical: the backend's output depends
# only on the program and the target, never on the host.
#
# Two hosts that refused the same program would agree, so a refusal fails
# the script unless refused.txt, beside this script, lists the program
# with the code it is refused with: a fixture rejected on purpose. So
# does a listed program that builds, or that is not in the corpus. The
# script prints the whole manifest first, then each failure, with
# oskac's error for a refusal.

set -euo pipefail
shopt -s nullglob

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  echo "usage: scripts/cross-build/manifest.sh OSKAC [repo-root]" >&2
  exit 2
fi
oskac=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
root=$(cd "${2:-$(dirname "$0")/../..}" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

targets=(linux-aarch64 linux-x86_64)

list=scripts/cross-build/refused.txt
if [ ! -f "$root/$list" ]; then
  echo "manifest.sh: $list is missing: it lists the corpus programs meant to be refused" >&2
  exit 1
fi
# The programs meant to be refused, one `PROGRAM CODE` per line; `#`
# starts a comment.
expected=$(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$root/$list")

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
programs=$(corpus | LC_ALL=C sort)

errors=()
count=0
while IFS= read -r program; do
  # The code `program` is meant to be refused with, if it is.
  code=$(awk -v p="$program" '$1 == p { print $2 }' <<<"$expected")
  for target in "${targets[@]}"; do
    out="$work/exe"
    rm -f "$out"
    # Built from the repository root, by its relative path, so a panic
    # site's path is the same on every host.
    if (cd "$root" && "$oskac" build "$program" --target "$target" -o "$out" >/dev/null 2>"$work/err"); then
      hash=$(sha256sum "$out" | cut -d' ' -f1)
      count=$((count + 1))
      if [ -n "$code" ]; then
        errors+=("$target $program built, but $list lists it as refused with $code: take it off the list")
      fi
    else
      hash="refused:$?"
      if [ -z "$code" ]; then
        errors+=("$target $program was refused, and $list does not expect it:
$(cat "$work/err")")
      elif ! grep -qF "error[$code]" "$work/err"; then
        errors+=("$target $program was refused without the $code that $list expects:
$(cat "$work/err")")
      fi
    fi
    printf '%s %s %s\n' "$target" "$program" "$hash"
  done
done <<<"$programs"

while read -r program _; do
  if ! grep -qxF -- "$program" <<<"$programs"; then
    errors+=("$list lists $program, which is not a corpus program")
  fi
done < <(printf '%s\n' "$expected" | sed '/^$/d')

if [ "$count" -eq 0 ]; then
  errors+=("no corpus program built")
fi

if [ "${#errors[@]}" -gt 0 ]; then
  printf 'manifest.sh: %s\n' "${errors[@]}" >&2
  exit 1
fi
