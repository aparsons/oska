#!/usr/bin/env bash
# Verify a signed release directory.
#
#   scripts/release/verify.sh <release-dir> <public-key>
#
# Checks, in order, that:
#   1. SHA256SUMS.sig is a signature of SHA256SUMS by <public-key> in the
#      `oska-release` namespace (`ssh-keygen -Y verify`);
#   2. every file SHA256SUMS lists is present with that digest;
#   3. the directory holds nothing else.
# Exits non-zero, naming the problem, if any check fails.

set -euo pipefail

die() {
  echo "verify.sh: $*" >&2
  exit 1
}

[ $# -eq 2 ] || die "usage: verify.sh <release-dir> <public-key>"
dir=$1
public=$2

for f in SHA256SUMS SHA256SUMS.sig; do
  [ -f "$dir/$f" ] || die "no $dir/$f"
done
[ -f "$public" ] || die "no public key at $public"

allowed=$(mktemp)
trap 'rm -f "$allowed"' EXIT
printf 'oska-release namespaces="oska-release" %s\n' "$(cat "$public")" >"$allowed"

ssh-keygen -Y verify -f "$allowed" -I oska-release -n oska-release \
  -s "$dir/SHA256SUMS.sig" <"$dir/SHA256SUMS" >/dev/null ||
  die "$dir/SHA256SUMS.sig is not a valid signature by $public"

(cd "$dir" && sha256sum --check --strict --quiet SHA256SUMS) ||
  die "a file in $dir does not match SHA256SUMS"

listed=$(awk '{print $2}' "$dir/SHA256SUMS" | LC_ALL=C sort)
present=$(find "$dir" -mindepth 1 -maxdepth 1 ! -name SHA256SUMS ! -name SHA256SUMS.sig \
  -printf '%P\n' | LC_ALL=C sort)
[ "$listed" = "$present" ] ||
  die "$dir holds files that SHA256SUMS does not list"
