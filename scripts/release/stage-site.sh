#!/usr/bin/env bash
# Put signed releases into a built site.
#
#   scripts/release/stage-site.sh <releases-dir> <site-dir> <public-key>
#
# <releases-dir> holds one directory per release tag (vX.Y.Z), each as
# sign.sh left it. This:
#   - verifies every release against <public-key> with verify.sh, and stops
#     if any fails (verify.sh also requires the tag signed inside each
#     release's SHA256SUMS to be the name of its directory);
#   - copies each to <site-dir>/releases/<tag>/;
#   - publishes <public-key> as <site-dir>/oska-release.pub;
#   - publishes the signed release index (see below);
#   - publishes the installers as <site-dir>/install.sh and install.ps1,
#     pinned to <public-key> (scripts/install/pin.sh), and pinned to
#     require the index when it published one;
#   - writes <site-dir>/latest.json for the newest release that is not a
#     pre-release, merging into any latest.json the generator wrote (the
#     release fields win).
#
# URLs in latest.json start with $OSKA_SITE_URL (default
# https://oskalang.dev), so tests can stage a site served elsewhere.
#
# A release directory may also hold a signed release index,
# release-index.json and release-index.json.sig, which the release run (or
# the index refresh) attached to that GitHub Release. Each is taken out of
# its release before the release is verified, so a release directory still
# holds nothing SHA256SUMS does not list, and is verified with
# verify-index.sh; one that does not verify stops the build, as a release
# does. Of those, the index with the highest serial is published as
# <site-dir>/release-index.json and .sig, if:
#   - it has not expired (an older, expired index is simply superseded);
#   - no other index has the same serial;
#   - the release it names is one published here.
# Otherwise the build stops. When the published index expires within 30
# days, it warns with a GitHub Actions `::warning::` annotation.
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

# The page at releases/<tag>/, which release notes and latest.json's
# release_url link to: each file with its SHA-256, and how to check the
# signature. Tags and file names are checked or come from the signed
# SHA256SUMS, so they need no escaping.
release_page() {
  local tag=$1 version=${1#v} sum file
  cat <<HTML
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>oskac $version</title>
</head>
<body>
<h1>oskac $version</h1>
<p>Install or update with the <a href="$base/guide/install.html">installer</a>, which checks the signature and the checksum for you.</p>
<ul>
HTML
  while read -r sum file; do
    printf '<li><a href="%s">%s</a><br><code>%s</code></li>\n' "$file" "$file" "$sum"
  done < <(tail -n +2 "$releases/$tag/SHA256SUMS")
  cat <<HTML
</ul>
<p><a href="SHA256SUMS">SHA256SUMS</a> lists the SHA-256 of each file, and <a href="SHA256SUMS.sig">SHA256SUMS.sig</a> is its signature by the <a href="$base/oska-release.pub">release key</a>. To check it:</p>
<pre>printf 'oska-release %s\\n' "\$(curl -fsS $base/oska-release.pub)" &gt; allowed
ssh-keygen -Y verify -f allowed -I oska-release -n oska-release -s SHA256SUMS.sig &lt; SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS</pre>
</body>
</html>
HTML
}

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

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-stage-site.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/releases" "$work/indexes"

for tag in "${tags[@]}"; do
  [[ $tag =~ $any ]] || die "$releases/$tag is not named for a release tag"
  [ ! -e "$site/releases/$tag" ] || die "$site/releases/$tag already exists"
  # Keep an index apart from the release files it travelled with.
  cp -R "$releases/$tag" "$work/releases/$tag"
  for f in release-index.json release-index.json.sig; do
    if [ -e "$work/releases/$tag/$f" ]; then
      mkdir -p "$work/indexes/$tag"
      mv "$work/releases/$tag/$f" "$work/indexes/$tag/$f"
    fi
  done
  "$here/verify.sh" "$work/releases/$tag" "$public"
  mkdir -p "$site/releases"
  mv "$work/releases/$tag" "$site/releases/$tag"
  release_page "$tag" >"$site/releases/$tag/index.html"
done

# The signed release index with the highest serial.
best=
best_serial=0
for dir in "$work"/indexes/*; do
  [ -d "$dir" ] || continue
  from=$releases/$(basename "$dir")
  fields=$("$here/verify-index.sh" "$dir" "$public") ||
    die "the release index in $from does not verify"
  read -r serial expires named <<<"$fields"
  if [ "$serial" -gt "$best_serial" ]; then
    best=$dir best_serial=$serial best_expires=$expires best_tag=$named best_from=$from
  elif [ "$serial" -eq "$best_serial" ] && ! cmp -s "$dir/release-index.json" "$best/release-index.json"; then
    die "the release indexes in $best_from and $from differ but both have serial $serial; sign a new one"
  fi
done
require=()
if [ -n "$best" ]; then
  now=$(date -u +%s)
  left=$(($(date -u -d "$best_expires" +%s) - now))
  [ "$left" -gt 0 ] ||
    die "the release index in $best_from (serial $best_serial) expired at $best_expires; re-sign it (docs/agents/release.md)"
  [ -d "$site/releases/$best_tag" ] ||
    die "the release index in $best_from names $best_tag, which is not a published release"
  if [ "$left" -lt $((30 * 86400)) ]; then
    echo "::warning::the release index (serial $best_serial) expires at $best_expires, in under 30 days; installers refuse it after that, so re-sign it (docs/agents/release.md)"
  fi
  cp "$best/release-index.json" "$best/release-index.json.sig" "$site/"
  require=(--require-index)
  echo "stage-site.sh: published release index serial $best_serial, naming $best_tag, expiring $best_expires"
fi
"$here/../install/pin.sh" "$public" "$site" "${require[@]}"

latest=$(printf '%s\n' "${tags[@]}" | grep -E "$stable" | sort -V | tail -n 1 || true)
if [ -z "$latest" ]; then
  echo "stage-site.sh: no stable release; latest.json not written"
  exit 0
fi

url="$base/releases/$latest"
release_json=$(
  tail -n +2 "$releases/$latest/SHA256SUMS" | awk '{print $2}' |
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
