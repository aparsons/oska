#!/usr/bin/env bash
# Push a signed release's oskac container image to the registry (ADR 0022).
#
#   scripts/release/push-image.sh digest|tags <release-dir> <public-key> <image-layout> <repository>
#
# The release runs it twice: `digest` before the GitHub Release is created
# (steps 1 to 3), and `tags` only after it exists (steps 1 to 5; step 3
# again pushes the same bytes). So a release that fails before it is
# published leaves at most an untagged image, and no public tag names it.
#
# <release-dir> is a release as the sign job left it (vX.Y.Z/, with the
# signed release index beside it), <image-layout> the OCI layout assemble.sh
# wrote for it, and <repository> a registry repository such as
# ghcr.io/aparsons/oskac, with skopeo already logged in to it. This:
#   1. verifies the release with verify.sh against <public-key>, so the
#      signature exists and covers oskac-image-digest;
#   2. checks that <image-layout> is that signed image: its index.json
#      names exactly the signed digest, and every blob matches its digest;
#   3. pushes those bytes by digest only, <repository>@<signed digest>,
#      with `skopeo copy --all --preserve-digests`, reads it back
#      (`skopeo inspect --raw`), and refuses unless the registry serves the
#      signed digest, so no public tag ever names other bytes;
#   4. tags it <repository>:vX.Y.Z, and checks the tag the same way;
#   5. tags it <repository>:latest too, checked the same way, only when the
#      signed release index beside the release (verify-index.sh) names this
#      tag as the latest, so latest never moves back to an older release or
#      to a pre-release, as the index never does.
# `digest` stops after step 3. It prints `<repository>@sha256:<hex>`, the reference users pull by. Any
# failure exits non-zero, and a step that fails stops every later one.
#
# Needs bash 4.4 or newer, skopeo, jq, sha256sum and ssh-keygen.

set -euo pipefail
# Steps run inside `$(...)`: without this a failure there would be ignored.
shopt -s inherit_errexit

here=$(cd "$(dirname "$0")" && pwd)

die() {
  echo "push-image.sh: $*" >&2
  exit 1
}

usage="usage: push-image.sh digest|tags <release-dir> <public-key> <image-layout> <repository>"
[ $# -eq 5 ] || die "$usage"
case $1 in
  digest | tags) step=$1 ;;
  *) die "'$1' is not a step: $usage" ;;
esac
release=$2
public=$3
layout=$4
repo=$5

# A repository and nothing else: no tag, no digest, no option.
[[ $repo =~ ^[a-z0-9]+([._-][a-z0-9]+)*(\.[a-z0-9]+([._-][a-z0-9]+)*)*(/[a-z0-9]+([._-][a-z0-9]+)*)+$ ]] ||
  die "'$repo' is not a registry repository such as ghcr.io/owner/name"
command -v skopeo >/dev/null 2>&1 || die "skopeo is not installed (the runner's own is used)"
command -v jq >/dev/null 2>&1 || die "jq is not installed"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is not installed"
[ -d "$release" ] || die "no release directory $release"
[ -f "$layout/index.json" ] || die "no OCI image layout at $layout"

tag=$(basename "$(cd "$release" && pwd)")
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "$release is not named for a release tag"

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-push-image.XXXXXXXX")
trap 'rm -rf "$work"' EXIT

# 1. The release, without the index that travels beside it, as verify.sh
#    wants it. Only the verified copy is read from here on.
mkdir "$work/release" "$work/index"
cp -R "$release" "$work/release/$tag"
signed_dir=$work/release/$tag
for f in release-index.json release-index.json.sig; do
  if [ -e "$signed_dir/$f" ]; then
    mv "$signed_dir/$f" "$work/index/$f"
  fi
done
"$here/verify.sh" "$signed_dir" "$public" ||
  die "the release in $release does not verify; nothing was pushed"
[ -f "$signed_dir/oskac-image-digest" ] ||
  die "the release has no signed oskac-image-digest; nothing was pushed"
# verify.sh checked its format: one line sha256:<64 hex>.
signed=$(head -n 1 "$signed_dir/oskac-image-digest")
[[ $signed =~ ^sha256:[0-9a-f]{64}$ ]] || die "oskac-image-digest holds '$signed'"

# Whether latest moves: only if the signed index names this release.
move_latest=no
if [ -f "$work/index/release-index.json" ]; then
  fields=$("$here/verify-index.sh" "$work/index" "$public") ||
    die "the release index beside $release does not verify; nothing was pushed"
  read -r _ _ named <<<"$fields"
  if [ "$named" = "$tag" ]; then
    move_latest=yes
  fi
fi

# 2. The layout is the signed image, byte for byte.
entries=$(jq -r '.manifests | length' "$layout/index.json") ||
  die "$layout/index.json is not an OCI index"
[ "$entries" = 1 ] || die "$layout/index.json names $entries images, not one"
named=$(jq -r '.manifests[0].digest' "$layout/index.json")
[ "$named" = "$signed" ] ||
  die "$layout holds the image $named, not the signed $signed; nothing was pushed"
for blob in "$layout"/blobs/sha256/*; do
  [ -f "$blob" ] && [ ! -L "$blob" ] || die "$blob is not a regular file"
  sum=$(sha256sum <"$blob")
  [ "${sum%% *}" = "$(basename "$blob")" ] ||
    die "$blob does not match its digest; nothing was pushed"
done
[ -f "$layout/blobs/sha256/${signed#sha256:}" ] || die "$layout has no blob for $signed"

# The digest the registry serves for reference $1.
served_digest() {
  local raw sum
  raw=$(mktemp "$work/raw.XXXXXXXX")
  skopeo inspect --raw "docker://$1" >"$raw" || return 1
  sum=$(sha256sum <"$raw") || return 1
  echo "sha256:${sum%% *}"
}

# Pushes the layout to reference $1 and checks that it serves the signed
# digest.
push_to() {
  skopeo copy --all --preserve-digests "oci:$layout" "docker://$1" >&2 ||
    die "pushing $1 failed"
  local served
  served=$(served_digest "$1") || die "cannot read $1 back after pushing it"
  [ "$served" = "$signed" ] ||
    die "$1 is $served, not the signed $signed; the release must not go on"
  echo "push-image.sh: $1 is the signed $signed" >&2
}

# 3, then, for the tags step only, 4 and 5.
push_to "$repo@$signed"
if [ "$step" = digest ]; then
  echo "$repo@$signed"
  exit 0
fi
push_to "$repo:$tag"
if [ "$move_latest" = yes ]; then
  push_to "$repo:latest"
else
  echo "push-image.sh: latest stays: the signed release index does not name $tag" >&2
fi

echo "$repo@$signed"
