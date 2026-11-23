#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Add a build-provenance attestation bundle to an assembled release.
#
#   scripts/release/add-bundle.sh <release-dir> <bundle>
#
# <bundle> is the file GitHub's attest action wrote for the release's
# binaries. This copies it into <release-dir> as oskac.intoto.jsonl (the
# `.intoto.jsonl` ending is what OpenSSF Scorecard counts as provenance)
# and adds its digest to SHA256SUMS, so sign.sh's signature covers it and
# verify.sh finds nothing unlisted. Run it after assemble.sh and before
# sign.sh: signing rewrites nothing, so a bundle added after signing
# would make verify.sh refuse the release.
#
# Refuses an empty bundle, a release that has a bundle already, and a
# release with no SHA256SUMS. The list keeps its order: the tag line
# first, then one line per file, sorted by name as assemble.sh sorts it.

set -euo pipefail

BUNDLE=oskac.intoto.jsonl

die() {
  echo "add-bundle.sh: $*" >&2
  exit 1
}

[ $# -eq 2 ] || die "usage: add-bundle.sh <release-dir> <bundle>"
dir=$1
bundle=$2

[ -f "$dir/SHA256SUMS" ] || die "no $dir/SHA256SUMS"
[ ! -e "$dir/SHA256SUMS.sig" ] || die "$dir is already signed; add the bundle before sign.sh"
[ -f "$bundle" ] && [ ! -L "$bundle" ] || die "$bundle is not a regular file"
[ -s "$bundle" ] || die "$bundle is empty"
[ ! -e "$dir/$BUNDLE" ] && [ ! -L "$dir/$BUNDLE" ] || die "$dir already holds $BUNDLE"
if tail -n +2 "$dir/SHA256SUMS" | awk '{print $2}' | grep -qx "$BUNDLE"; then
  die "SHA256SUMS already lists $BUNDLE"
fi

cp "$bundle" "$dir/$BUNDLE"
{
  head -n 1 "$dir/SHA256SUMS"
  {
    tail -n +2 "$dir/SHA256SUMS"
    (cd "$dir" && sha256sum -- "$BUNDLE")
  } | LC_ALL=C sort -k2
} >"$dir/SHA256SUMS.new"
mv "$dir/SHA256SUMS.new" "$dir/SHA256SUMS"
