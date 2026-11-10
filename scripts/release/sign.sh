#!/usr/bin/env bash
# Sign a release directory's SHA256SUMS with an SSH key.
#
#   scripts/release/sign.sh <release-dir> <private-key> <public-key>
#
# Writes <release-dir>/SHA256SUMS.sig with `ssh-keygen -Y sign` in the
# `oska-release` namespace, then checks the whole release with verify.sh.
# <public-key> is the key the site publishes: signing refuses a private key
# whose public half is not that key, so a release can never be signed by a
# key users cannot verify.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

die() {
  echo "sign.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || die "usage: sign.sh <release-dir> <private-key> <public-key>"
dir=$1
private=$2
public=$3

[ -f "$dir/SHA256SUMS" ] || die "no $dir/SHA256SUMS"
[ -f "$public" ] || die "no public key at $public"

# Compare key type and key material; the comment may differ.
derived=$(ssh-keygen -y -f "$private" | awk '{print $1, $2}')
published=$(awk '{print $1, $2}' "$public")
[ "$derived" = "$published" ] ||
  die "the signing key does not match the published key $public"

rm -f "$dir/SHA256SUMS.sig"
ssh-keygen -q -Y sign -f "$private" -n oska-release "$dir/SHA256SUMS"
"$here/verify.sh" "$dir" "$public"
