#!/usr/bin/env bash
# Assemble a release directory from the binaries CI built.
#
#   scripts/release/assemble.sh <tag> <builds-dir> <out-dir>
#
# <builds-dir> holds one binary per release target, already named for its
# target (see ASSETS). This writes <out-dir>/<tag>/ with those binaries and
# a SHA256SUMS file in `sha256sum` format, ready for sign.sh.
#
# Runs on Linux (it needs GNU coreutils' sha256sum).

set -euo pipefail

# Every release ships exactly these files: oskac-<os>-<arch>[.exe].
ASSETS=(
  oskac-linux-x86_64
  oskac-linux-aarch64
  oskac-macos-x86_64
  oskac-macos-aarch64
  oskac-windows-x86_64.exe
)

die() {
  echo "assemble.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || die "usage: assemble.sh <tag> <builds-dir> <out-dir>"
tag=$1
builds=$2
out=$3

[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "tag '$tag' is not of the form vX.Y.Z or vX.Y.Z-pre"

for asset in "${ASSETS[@]}"; do
  [ -f "$builds/$asset" ] || die "missing $builds/$asset"
done

dir="$out/$tag"
[ ! -e "$dir" ] || die "$dir already exists"
mkdir -p "$dir"

for asset in "${ASSETS[@]}"; do
  cp "$builds/$asset" "$dir/$asset"
  chmod 755 "$dir/$asset"
done

(cd "$dir" && LC_ALL=C sha256sum -- "${ASSETS[@]}" | LC_ALL=C sort -k2) >"$dir/SHA256SUMS"
