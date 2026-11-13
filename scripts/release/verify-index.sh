#!/usr/bin/env bash
# Verify a signed release index.
#
#   scripts/release/verify-index.sh <dir> <public-key>
#
# Checks that <dir>/release-index.json.sig is a signature of
# <dir>/release-index.json by <public-key> in the `oska-index` namespace
# (`ssh-keygen -Y verify`), and that the index has the schema-1 format:
#
#   {"schema": 1, "serial": <positive integer>,
#    "issued": "<UTC RFC 3339>", "expires": "<UTC RFC 3339, after issued>",
#    "latest": {"tag": "vX.Y.Z", "version": "X.Y.Z"}}
#
# with no other field. On success it prints one line,
#
#   <serial> <expires> <tag>
#
# for the caller. It does not check whether the index has expired: that
# depends on when it is used (stage-site.sh and the installers do).
# Exits non-zero, naming the problem, if any check fails.

set -euo pipefail

die() {
  echo "verify-index.sh: $*" >&2
  exit 1
}

[ $# -eq 2 ] || die "usage: verify-index.sh <dir> <public-key>"
dir=$1
public=$2
index=$dir/release-index.json

for f in "$index" "$index.sig"; do
  [ -f "$f" ] || die "no $f"
done
[ -f "$public" ] || die "no public key at $public"

allowed=$(mktemp "${TMPDIR:-/tmp}/oska-index-allowed.XXXXXXXX")
trap 'rm -f "$allowed"' EXIT
printf 'oska-index namespaces="oska-index" %s\n' "$(cat "$public")" >"$allowed"

ssh-keygen -Y verify -f "$allowed" -I oska-index -n oska-index \
  -s "$index.sig" <"$index" >/dev/null 2>&1 ||
  die "$index.sig is not a valid oska-index signature by $public"

time_re='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$'
jq -e --arg t "$time_re" '
  type == "object"
  and (keys == ["expires", "issued", "latest", "schema", "serial"])
  and .schema == 1
  and (.serial | type == "number" and . >= 1 and . == floor and . < 9007199254740992)
  and (.issued | type == "string" and test($t))
  and (.expires | type == "string" and test($t))
  and .expires > .issued
  and (.latest | type == "object" and keys == ["tag", "version"])
  and (.latest.tag | type == "string" and test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))
  and .latest.version == (.latest.tag | ltrimstr("v"))
' "$index" >/dev/null 2>&1 || die "$index is not a schema-1 release index"

jq -r '"\(.serial) \(.expires) \(.latest.tag)"' "$index"
