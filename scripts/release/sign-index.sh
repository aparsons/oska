#!/usr/bin/env bash
# Sign the next release index.
#
#   scripts/release/sign-index.sh [--tag <tag>] [--previous-key <public-key>] \
#     <private-key> <public-key> <site-url> <out-dir>
#
# The release index (release-index.json, beside latest.json at the site
# root) names the newest stable release, and installers take the latest
# release from it once its signature checks. This writes the next one,
# <out-dir>/release-index.json, and its signature,
# <out-dir>/release-index.json.sig (`ssh-keygen -Y sign -n oska-index`):
#
#   - it reads the index published at <site-url>/release-index.json and
#     verifies it (verify-index.sh) against <public-key>, or against
#     --previous-key after a key rotation; one that does not verify stops
#     the signing, as signing over it could lower the serial;
#   - the new serial is the published serial + 1, or 1 if none is
#     published (a 404 at an https:// URL, or no file at a file:// URL);
#   - `latest` is the published index's, or --tag if that is a stable
#     release (vX.Y.Z) newer than it, so it never moves back;
#   - `issued` is now and `expires` is 90 days later.
#
# <public-key> is the key the site publishes: signing refuses a private key
# whose public half is not that key, as sign.sh does. With no stable
# release to name (none published, and no stable --tag), it signs nothing
# and says so.
#
# Runs on Linux (it needs GNU date). Needs curl, jq and ssh-keygen.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

die() {
  echo "sign-index.sh: $*" >&2
  exit 1
}

usage="usage: sign-index.sh [--tag <tag>] [--previous-key <public-key>] <private-key> <public-key> <site-url> <out-dir>"
tag=
previous_key=
while [ $# -gt 0 ]; do
  case $1 in
    --tag)
      [ $# -ge 2 ] || die "$usage"
      tag=$2
      shift 2 ;;
    --previous-key)
      [ $# -ge 2 ] || die "$usage"
      previous_key=$2
      shift 2 ;;
    -*) die "$usage" ;;
    *) break ;;
  esac
done
[ $# -eq 4 ] || die "$usage"
private=$1
public=$2
site=${3%/}
out=$4
[ -n "$previous_key" ] || previous_key=$public

stable='^v[0-9]+\.[0-9]+\.[0-9]+$'
[ -z "$tag" ] || [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "--tag '$tag' is not of the form vX.Y.Z or vX.Y.Z-pre"
[ -f "$public" ] || die "no public key at $public"
[ -f "$previous_key" ] || die "no public key at $previous_key"
[ -d "$out" ] || die "no directory $out"

# Compare key type and key material; the comment may differ.
derived=$(ssh-keygen -y -f "$private" | awk '{print $1, $2}')
published=$(awk '{print $1, $2}' "$public")
[ "$derived" = "$published" ] ||
  die "the signing key does not match the published key $public"

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-sign-index.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/published"

# fetch <url> <file>: 0 if fetched, 3 if nothing is published there, 1 on
# any other failure, so a network error is never taken for "no index".
fetch() {
  local code
  case $1 in
    file://*)
      [ -e "${1#file://}" ] || return 3
      cp "${1#file://}" "$2" ;;
    https://*)
      code=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 --silent --show-error \
        --location --retry 3 --output "$2" --write-out '%{http_code}' "$1") || return 1
      case $code in
        200) ;;
        404) return 3 ;;
        *) echo "sign-index.sh: $1 answered HTTP $code" >&2; return 1 ;;
      esac ;;
    *) die "<site-url> must be an https:// or file:// URL, not $1" ;;
  esac
}

serial=0
latest=
status=0
fetch "$site/release-index.json" "$work/published/release-index.json" || status=$?
case $status in
  0)
    fetch "$site/release-index.json.sig" "$work/published/release-index.json.sig" ||
      die "cannot fetch $site/release-index.json.sig"
    read -r serial _ latest < <("$here/verify-index.sh" "$work/published" "$previous_key") ||
      die "the index published at $site does not verify; refusing to sign over it" ;;
  3) echo "sign-index.sh: no index is published at $site; signing serial 1" ;;
  *) die "cannot fetch $site/release-index.json" ;;
esac

# The newer of the published latest and a stable --tag, by version order.
if [[ $tag =~ $stable ]]; then
  if [ -z "$latest" ] || [ "$(printf '%s\n%s\n' "$latest" "$tag" | sort -V | tail -n 1)" = "$tag" ]; then
    latest=$tag
  fi
fi
if [ -z "$latest" ]; then
  echo "sign-index.sh: no stable release to name; no index signed"
  exit 0
fi

now=$(date -u +%s)
jq -n --argjson serial $((serial + 1)) \
  --arg issued "$(date -u -d "@$now" +%Y-%m-%dT%H:%M:%SZ)" \
  --arg expires "$(date -u -d "@$((now + 90 * 86400))" +%Y-%m-%dT%H:%M:%SZ)" \
  --arg tag "$latest" '{
    schema: 1,
    serial: $serial,
    issued: $issued,
    expires: $expires,
    latest: {tag: $tag, version: ($tag | ltrimstr("v"))}
  }' >"$out/release-index.json"
rm -f "$out/release-index.json.sig"
ssh-keygen -q -Y sign -f "$private" -n oska-index "$out/release-index.json"
"$here/verify-index.sh" "$out" "$public" >/dev/null
echo "sign-index.sh: signed serial $((serial + 1)), latest $latest, in $out/release-index.json"
