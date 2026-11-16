#!/usr/bin/env bash
# Installs the pinned ShellCheck (scripts/shellcheck/version) into DIR, for CI
# jobs: runners ship different ShellCheck releases, which report different
# findings. Linux x86_64 and aarch64 only; the tarball's SHA-256 is pinned here.
#
#   scripts/shellcheck/install.sh DIR
set -euo pipefail

[ $# -eq 1 ] || {
  echo "usage: install.sh DIR" >&2
  exit 2
}
dir=$1
here=$(cd "$(dirname "$0")" && pwd)
version=$(tr -d '[:space:]' <"$here/version")

case "$version/$(uname -m)" in
  0.11.0/x86_64) sum=8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198 ;;
  0.11.0/aarch64 | 0.11.0/arm64) sum=12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588 ;;
  *)
    echo "install.sh: no pinned checksum for ShellCheck $version on $(uname -m)" >&2
    exit 1
    ;;
esac
arch=$(uname -m)
[ "$arch" = arm64 ] && arch=aarch64

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tarball="shellcheck-v$version.linux.$arch.tar.xz"
curl -fsSL -o "$work/$tarball" "https://github.com/koalaman/shellcheck/releases/download/v$version/$tarball"
echo "$sum  $work/$tarball" | sha256sum -c - >/dev/null || {
  echo "install.sh: $tarball does not match its pinned SHA-256" >&2
  exit 1
}
tar -xJf "$work/$tarball" -C "$work"
mkdir -p "$dir"
install -m 755 "$work/shellcheck-v$version/shellcheck" "$dir/shellcheck"
"$dir/shellcheck" --version | sed -n 2p
