#!/usr/bin/env bash
# Validates GitHub workflow files the way GitHub reads them. A workflow that
# GitHub cannot parse produces no jobs and no checks, so the PR looks green
# with the workflow silently missing (PR #329, an unquoted ": " in an input
# description).
#
#   scripts/actionlint/run.sh [FILE...]   default: .github/workflows/*.y*ml
#
# Exit 0: every file is valid. Exit 1: actionlint rejected one. Exit 3: no
# actionlint could be had (none on PATH, and the pinned release could not be
# downloaded or does not match its checksum); scripts/check.sh turns that
# into a skip locally and a failure in CI.
#
# actionlint is pinned by version and by the SHA-256 of each release tarball.
# An actionlint already on PATH is used as is. Otherwise the pinned linux
# release (amd64 or arm64) is downloaded once into the cache. Its own
# own lint passes for shell and Python are off: this checks the workflow's
# structure and expressions, and the shellcheck step covers scripts.

set -euo pipefail

version=1.7.12
sha_amd64=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
sha_arm64=325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6

cd "$(dirname "$0")/../.."

unavailable() {
  printf 'actionlint: %s\n' "$1" >&2
  exit 3
}

find_tool() {
  if command -v actionlint >/dev/null 2>&1; then
    command -v actionlint
    return
  fi
  local arch want
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) arch=amd64 want=$sha_amd64 ;;
    Linux/aarch64 | Linux/arm64) arch=arm64 want=$sha_arm64 ;;
    *) unavailable "no pinned release for $(uname -s)/$(uname -m)" ;;
  esac
  local dir="${XDG_CACHE_HOME:-$HOME/.cache}/oska/actionlint-$version-$arch"
  if [ ! -x "$dir/actionlint" ]; then
    command -v curl >/dev/null 2>&1 || unavailable "curl not installed"
    local tmp tar got
    tmp=$(mktemp -d)
    tar="$tmp/actionlint.tar.gz"
    if ! curl -fsSL --retry 3 -o "$tar" \
      "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_linux_$arch.tar.gz"; then
      rm -rf "$tmp"
      unavailable "download failed"
    fi
    got=$(sha256sum "$tar" | cut -d' ' -f1)
    if [ "$got" != "$want" ]; then
      rm -rf "$tmp"
      unavailable "checksum mismatch for the $arch release (got $got)"
    fi
    mkdir -p "$dir"
    tar -xzf "$tar" -C "$tmp" actionlint
    mv "$tmp/actionlint" "$dir/actionlint"
    rm -rf "$tmp"
  fi
  printf '%s\n' "$dir/actionlint"
}

tool=$(find_tool)

if [ $# -eq 0 ]; then
  for f in .github/workflows/*.yml .github/workflows/*.yaml; do
    [ -f "$f" ] && set -- "$@" "$f"
  done
fi
if [ $# -eq 0 ]; then
  echo "actionlint: no workflow files"
  exit 0
fi

"$tool" -shellcheck= -pyflakes= "$@"
