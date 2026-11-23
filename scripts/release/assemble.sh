#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Assemble a release directory from the binaries CI built.
#
#   SOURCE_DATE_EPOCH=<seconds> \
#     scripts/release/assemble.sh <tag> <builds-dir> <out-dir> [<image-dir>]
#   scripts/release/assemble.sh --list
#
# --list prints the assets a release ships, one per line, and does
# nothing else (scripts/release/test.sh holds release.yml's build matrix
# to it).
#
# <builds-dir> holds one binary per release target, already named for its
# target (see ASSETS). This writes <out-dir>/<tag>/ with those binaries and
# a SHA256SUMS file, ready for sign.sh. SHA256SUMS starts with the line
#
#   # oska-release <tag>
#
# and then has one `sha256sum` line per file. The first line binds the
# tag into what sign.sh signs, so a signed release cannot be passed off as
# another version: verify.sh and the installers require it to name the tag
# they asked for. `sha256sum --check` skips it as a comment.
#
# It also builds the container image ghcr.io/aparsons/oskac from the two
# musl builds with image.sh (ADR 0022), and writes the digest of its image
# index, `sha256:<hex>` on one line, to <out-dir>/<tag>/oskac-image-digest,
# which SHA256SUMS lists. So the release signature covers the image, and
# the image is built from exactly the musl builds the release ships. The
# OCI layout goes to <image-dir> (which must not exist, or be empty), or is
# thrown away if none is named. SOURCE_DATE_EPOCH, the time stamped into
# the image, must be set: the release run takes the tag's commit time.
#
# It also writes the SPDX SBOM, <out-dir>/<tag>/oskac.spdx.json, from
# Cargo.lock at the root of this checkout (scripts/release/sbom.sh), and
# SHA256SUMS lists it too, so the release signature covers it.
#
# Runs on Linux (it needs GNU coreutils' sha256sum).

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

# Every release ships exactly these files: oskac-<os>-<arch>[.exe], and on
# Linux two builds per processor (ADR 0022): oskac-linux-<arch>-gnu, the
# glibc build, and oskac-linux-<arch>-musl, the static musl build.
ASSETS=(
  oskac-linux-x86_64-gnu
  oskac-linux-x86_64-musl
  oskac-linux-aarch64-gnu
  oskac-linux-aarch64-musl
  oskac-macos-x86_64
  oskac-macos-aarch64
  oskac-windows-x86_64.exe
)
# Not a binary: the digest of the image built from the musl builds.
IMAGE_DIGEST=oskac-image-digest
# Not a binary either: the SPDX SBOM of the release (sbom.sh), from the
# Cargo.lock of the commit this runs in, the one the builds used.
SBOM=oskac.spdx.json

die() {
  echo "assemble.sh: $*" >&2
  exit 1
}

if [ $# -eq 1 ] && [ "$1" = --list ]; then
  printf '%s\n' "${ASSETS[@]}"
  exit 0
fi

[ $# -eq 3 ] || [ $# -eq 4 ] ||
  die "usage: assemble.sh <tag> <builds-dir> <out-dir> [<image-dir>], or assemble.sh --list"
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

# The image first, so a release that cannot have one leaves nothing behind.
if [ $# -eq 4 ]; then
  image=$4
else
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT
  image=$scratch/image
fi
digest=$("$here/image.sh" "$builds/oskac-linux-x86_64-musl" "$builds/oskac-linux-aarch64-musl" "$image") ||
  die "could not build the image"
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || die "image.sh printed '$digest', not a digest"

# Before the release folder exists, so a lock that gives no SBOM leaves nothing.
sbom=$("$here/sbom.sh" "$tag" "$here/../../Cargo.lock") || die "could not write the SBOM"

mkdir -p "$dir"
for asset in "${ASSETS[@]}"; do
  cp "$builds/$asset" "$dir/$asset"
  chmod 755 "$dir/$asset"
done
printf '%s\n' "$digest" >"$dir/$IMAGE_DIGEST"
printf '%s\n' "$sbom" >"$dir/$SBOM"

{
  printf '# oska-release %s\n' "$tag"
  (cd "$dir" && LC_ALL=C sha256sum -- "${ASSETS[@]}" "$IMAGE_DIGEST" "$SBOM" | LC_ALL=C sort -k2)
} >"$dir/SHA256SUMS"
