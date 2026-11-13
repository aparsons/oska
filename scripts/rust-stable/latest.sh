#!/usr/bin/env bash
# Reads Rust's channel-rust-stable.toml on stdin and prints the stable
# version, the `version` of `[pkg.rustc]` without its commit and date:
#
#   curl -fsSL https://static.rust-lang.org/dist/channel-rust-stable.toml | scripts/rust-stable/latest.sh
#
# Fails when there is no such version in the usual X.Y.Z form.

set -euo pipefail

version=$(awk '
  /^\[/ { in_rustc = ($0 == "[pkg.rustc]") }
  in_rustc && /^version = / { gsub(/^version = "|[ "].*$/, ""); print; exit }
')
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "latest.sh: no [pkg.rustc] version in the channel file" >&2
  exit 1
}
printf '%s\n' "$version"
