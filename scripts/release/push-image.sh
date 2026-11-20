#!/usr/bin/env bash
# Push a signed release's oskac container image to the registry (ADR 0022).
#
#   SOURCE_DATE_EPOCH=<tag's commit time> \
#     scripts/release/push-image.sh <release-dir> <public-key> <repository>
#
# <release-dir> is a release as the sign job left it (vX.Y.Z/, maybe with
# the release index beside it), <repository> a registry repository such as
# ghcr.io/aparsons/oskac, with skopeo already logged in to it. This:
#   1. verifies the release with verify.sh against <public-key>, so the
#      signature exists and covers oskac-image-digest and the musl builds;
#   2. rebuilds the image from the release's own two musl builds with
#      image.sh, and refuses unless its digest is the signed one, so no
#      byte comes from anywhere but the signed release and this repo;
#   3. pushes those OCI bytes as <repository>:vX.Y.Z with
#      `skopeo copy --all --preserve-digests`, and then reads the tag back
#      (`skopeo inspect --raw`) and refuses unless the registry serves the
#      signed digest;
#   4. for a stable tag only, does the same for <repository>:latest.
# It prints `<repository>@sha256:<hex>`, the reference users pull by. Any
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

[ $# -eq 3 ] || die "usage: push-image.sh <release-dir> <public-key> <repository>"
release=$1
public=$2
repo=$3

# A repository and nothing else: no tag, no digest, no option.
[[ $repo =~ ^[a-z0-9]+([._-][a-z0-9]+)*(\.[a-z0-9]+([._-][a-z0-9]+)*)*(/[a-z0-9]+([._-][a-z0-9]+)*)+$ ]] ||
  die "'$repo' is not a registry repository such as ghcr.io/owner/name"
command -v skopeo >/dev/null 2>&1 || die "skopeo is not installed"
command -v jq >/dev/null 2>&1 || die "jq is not installed"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is not installed"
[ -d "$release" ] || die "no release directory $release"

tag=$(basename "$(cd "$release" && pwd)")
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "$release is not named for a release tag"

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-push-image.XXXXXXXX")
trap 'rm -rf "$work"' EXIT

# 1. The release, without the index that travels beside it, as verify.sh
#    wants it. Only the verified copy is read from here on.
mkdir "$work/release"
cp -R "$release" "$work/release/$tag"
rm -f "$work/release/$tag/release-index.json" "$work/release/$tag/release-index.json.sig"
signed_dir=$work/release/$tag
"$here/verify.sh" "$signed_dir" "$public" ||
  die "the release in $release does not verify; nothing was pushed"
[ -f "$signed_dir/oskac-image-digest" ] ||
  die "the release has no signed oskac-image-digest; nothing was pushed"
# verify.sh checked its format: one line sha256:<64 hex>.
signed=$(head -n 1 "$signed_dir/oskac-image-digest")
[[ $signed =~ ^sha256:[0-9a-f]{64}$ ]] || die "oskac-image-digest holds '$signed'"

# 2. Rebuild from the signed musl builds.
layout=$work/image
built=$("$here/image.sh" "$signed_dir/oskac-linux-x86_64-musl" \
  "$signed_dir/oskac-linux-aarch64-musl" "$layout")
[ "$built" = "$signed" ] ||
  die "the signed musl builds rebuild to $built, not the signed $signed (is SOURCE_DATE_EPOCH the tag's commit time?); nothing was pushed"

# The digest the registry serves for <repository>:$1.
served_digest() {
  local raw
  raw=$(mktemp "$work/raw.XXXXXXXX")
  skopeo inspect --raw "docker://$repo:$1" >"$raw" || return 1
  local sum
  sum=$(sha256sum <"$raw") || return 1
  echo "sha256:${sum%% *}"
}

# 3 and 4. Push to each tag, and read it back.
names=("$tag")
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] && names+=(latest)
for name in "${names[@]}"; do
  skopeo copy --all --preserve-digests "oci:$layout" "docker://$repo:$name" >&2 ||
    die "pushing $repo:$name failed"
  served=$(served_digest "$name") || die "cannot read $repo:$name back"
  [ "$served" = "$signed" ] ||
    die "$repo:$name is $served, not the signed $signed; the release must not go on"
  echo "push-image.sh: $repo:$name is the signed $signed" >&2
done

echo "$repo@$signed"
