#!/usr/bin/env bash
# Put signed releases into a built site.
#
#   scripts/release/stage-site.sh <releases-dir> <site-dir> <public-key>
#
# <releases-dir> holds one directory per release tag (vX.Y.Z), each as
# sign.sh left it. This:
#   - verifies every release against <public-key> with verify.sh, and stops
#     if any fails;
#   - copies each to <site-dir>/releases/<tag>/;
#   - publishes <public-key> as <site-dir>/oska-release.pub;
#   - publishes the installers as <site-dir>/install.sh and install.ps1,
#     pinned to <public-key> (scripts/install/pin.sh);
#   - writes <site-dir>/latest.json for the newest release that is not a
#     pre-release, merging into any latest.json the generator wrote (the
#     release fields win).
#
# URLs in latest.json start with $OSKA_SITE_URL (default
# https://oskalang.dev), so tests can stage a site served elsewhere.
#
# With no releases it only publishes the key and the installers, if there
# is a key. Without a key it publishes no installers, as they could verify
# nothing.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

die() {
  echo "stage-site.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || die "usage: stage-site.sh <releases-dir> <site-dir> <public-key>"
releases=$1
site=$2
public=$3
base=${OSKA_SITE_URL:-https://oskalang.dev}
base=${base%/}

stable='^v[0-9]+\.[0-9]+\.[0-9]+$'
any='^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$'

[ -d "$site" ] || die "no site directory $site"

tags=()
if [ -d "$releases" ]; then
  while IFS= read -r tag; do
    tags+=("$tag")
  done < <(find "$releases" -mindepth 1 -maxdepth 1 -type d -printf '%P\n' | LC_ALL=C sort)
fi

if [ ! -f "$public" ]; then
  [ ${#tags[@]} -eq 0 ] || die "no public key at $public to verify releases with"
  echo "stage-site.sh: no releases and no public key; nothing to stage"
  exit 0
fi
cp "$public" "$site/oska-release.pub"
"$here/../install/pin.sh" "$public" "$site"

for tag in "${tags[@]}"; do
  [[ $tag =~ $any ]] || die "$releases/$tag is not named for a release tag"
  "$here/verify.sh" "$releases/$tag" "$public"
  [ ! -e "$site/releases/$tag" ] || die "$site/releases/$tag already exists"
  mkdir -p "$site/releases"
  cp -R "$releases/$tag" "$site/releases/$tag"
done

latest=$(printf '%s\n' "${tags[@]}" | grep -E "$stable" | sort -V | tail -n 1 || true)
if [ -z "$latest" ]; then
  echo "stage-site.sh: no stable release; latest.json not written"
  exit 0
fi

url="$base/releases/$latest"
release_json=$(
  awk '{print $2}' "$releases/$latest/SHA256SUMS" |
    jq -R -n --arg tag "$latest" --arg url "$url" --arg base "$base" '{
      version: ($tag | ltrimstr("v")),
      tag: $tag,
      release_url: "\($url)/",
      checksums_url: "\($url)/SHA256SUMS",
      signature_url: "\($url)/SHA256SUMS.sig",
      signing_key_url: "\($base)/oska-release.pub",
      signing_namespace: "oska-release",
      installers: {sh: "\($base)/install.sh", ps1: "\($base)/install.ps1"},
      binaries: ([inputs
        | {key: (ltrimstr("oskac-") | rtrimstr(".exe")), value: "\($url)/\(.)"}]
        | from_entries)
    }'
)

if [ -f "$site/latest.json" ]; then
  merged=$(jq -n --argjson release "$release_json" --slurpfile site "$site/latest.json" \
    '$site[0] * $release')
  printf '%s\n' "$merged" >"$site/latest.json"
else
  printf '%s\n' "$release_json" >"$site/latest.json"
fi
