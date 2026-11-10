#!/usr/bin/env bash
# Verify a signed release directory.
#
#   scripts/release/verify.sh <release-dir> <public-key>
#
# Checks, in order, that:
#   1. SHA256SUMS.sig is a signature of SHA256SUMS by <public-key> in the
#      `oska-release` namespace (`ssh-keygen -Y verify`);
#   2. the first line of SHA256SUMS is `# oska-release <tag>`, where <tag>
#      is the name of <release-dir>, so a signed release moved to another
#      tag's directory is refused, and every other line is a SHA-256 digest
#      and a file name;
#   3. every file SHA256SUMS lists is present with that digest;
#   4. the directory holds nothing else.
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

tag=$(basename "$(cd "$dir" && pwd)")
signed=$(head -n 1 "$dir/SHA256SUMS")
[ "$signed" = "# oska-release $tag" ] ||
  die "$dir/SHA256SUMS is signed for '${signed#\# oska-release }', not for $tag"
if tail -n +2 "$dir/SHA256SUMS" | grep -Evq '^[0-9a-f]{64}  [A-Za-z0-9._-]+$'; then
  die "$dir/SHA256SUMS has a line that is not a SHA-256 digest and a file name"
fi

(cd "$dir" && sha256sum --check --strict --quiet SHA256SUMS) ||
  die "a file in $dir does not match SHA256SUMS"

listed=$(tail -n +2 "$dir/SHA256SUMS" | awk '{print $2}' | LC_ALL=C sort)
present=$(find "$dir" -mindepth 1 -maxdepth 1 ! -name SHA256SUMS ! -name SHA256SUMS.sig \
  -printf '%P\n' | LC_ALL=C sort)
[ "$listed" = "$present" ] ||
  die "$dir holds files that SHA256SUMS does not list"
