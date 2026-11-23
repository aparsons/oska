#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
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
# An actionlint on PATH is used only if it reports the pinned version.
# Otherwise the pinned linux release (amd64 or arm64) is downloaded once into
# the cache, and its binary is re-hashed before every run. actionlint's own
# shell and Python lint passes are off: this checks the workflow's structure
# and expressions, and the shellcheck step covers scripts.

set -euo pipefail

version=1.7.12
sha_amd64=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
sha_arm64=325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6

cd "$(dirname "$0")/../.."

unavailable() {
  printf 'actionlint: %s\n' "$1" >&2
  exit 3
}

# find_tool: sets $tool to a usable actionlint of the pinned version, or exits
# 3. It sets a variable rather than printing, so a failure inside it cannot be
# lost in a command substitution.
tool=
find_tool() {
  local found
  if found=$(command -v actionlint 2>/dev/null) &&
    [ "$("$found" -version 2>/dev/null | head -n 1)" = "$version" ]; then
    tool=$found
    return
  fi
  local arch want
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) arch=amd64 want=$sha_amd64 ;;
    Linux/aarch64 | Linux/arm64) arch=arm64 want=$sha_arm64 ;;
    *) unavailable "no pinned release for $(uname -s)/$(uname -m)" ;;
  esac
  local dir="${XDG_CACHE_HOME:-$HOME/.cache}/oska/actionlint-$version-$arch"
  # The cached binary is trusted only while it hashes to what was recorded
  # when it was extracted from the verified tarball.
  if [ -x "$dir/actionlint" ] && [ -f "$dir/actionlint.sha256" ] &&
    [ "$(sha256sum "$dir/actionlint" | cut -d' ' -f1)" = "$(cat "$dir/actionlint.sha256")" ]; then
    tool=$dir/actionlint
    return
  fi
  command -v curl >/dev/null 2>&1 || unavailable "curl not installed"
  local tmp tgz got
  tmp=$(mktemp -d) || unavailable "mktemp failed"
  tgz="$tmp/actionlint.tar.gz"
  if ! curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$tgz" \
    "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_linux_$arch.tar.gz"; then
    rm -rf "$tmp"
    unavailable "download failed"
  fi
  got=$(sha256sum "$tgz" | cut -d' ' -f1)
  if [ "$got" != "$want" ]; then
    rm -rf "$tmp"
    unavailable "checksum mismatch for the $arch release (got $got)"
  fi
  if ! tar -xzf "$tgz" -C "$tmp" actionlint ||
    ! mkdir -p "$dir" ||
    ! mv -f "$tmp/actionlint" "$dir/actionlint" ||
    ! sha256sum "$dir/actionlint" | cut -d' ' -f1 >"$dir/actionlint.sha256"; then
    rm -rf "$tmp"
    unavailable "could not install the release into $dir"
  fi
  rm -rf "$tmp"
  tool=$dir/actionlint
}

find_tool

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
