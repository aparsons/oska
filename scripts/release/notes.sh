#!/usr/bin/env bash
# Print a GitHub release's body: its version's CHANGELOG.md section, then
# a footer.
#
#   scripts/release/notes.sh <tag> <changelog> <public-key> [<image-digest-file>]
#
# The section is the one under the heading `## vX.Y.Z` for the tag's
# version; a pre-release tag vX.Y.Z-rc.N uses the vX.Y.Z section. Its
# relative links, which point into the repository, are made absolute
# (https://github.com/aparsons/oska/blob/<tag>/...), so they work on the
# release page. The footer names, in this order:
#
#   - the download page, https://oskalang.dev/releases/<tag>/;
#   - the release key's SHA256 fingerprint, computed from <public-key>
#     (site/oska-release.pub) with `ssh-keygen -l`, never typed by hand;
#   - the container image as ghcr.io/aparsons/oskac@sha256:<hex>, from
#     <image-digest-file> (oskac-image-digest), left out when none is
#     named, as for releases before the image;
#   - the version's anchor on https://oskalang.dev/changelog.html.
#
# It fails closed, printing nothing: a tag that is not a version, an
# unreadable file, no section for the version, two of them, a section with
# nothing in it but its date, or a digest file that is not one line
# `sha256:<64 hex digits>` each stop it with a message.
#
# release.yml runs it in `assemble`, before anything is signed, and
# `publish` hands its output to `gh release create --notes-file`, so the
# notes never pass through a shell command line.

set -euo pipefail

site=https://oskalang.dev
repo=https://github.com/aparsons/oska
image=ghcr.io/aparsons/oskac

die() {
  echo "notes.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || [ $# -eq 4 ] ||
  die "usage: notes.sh <tag> <changelog> <public-key> [<image-digest-file>]"
tag=$1
changelog=$2
public=$3
digest_file=${4:-}

[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "tag '$tag' is not of the form vX.Y.Z or vX.Y.Z-pre"
version=${tag%%-*}
# The site's GitHub-style anchor for the heading `## vX.Y.Z`.
anchor=${version//./}

[ -f "$changelog" ] && [ -r "$changelog" ] || die "cannot read the changelog $changelog"
[ -f "$public" ] && [ -r "$public" ] || die "cannot read the public key $public"

fingerprint=$(ssh-keygen -l -E sha256 -f "$public" | awk '{print $2}') ||
  die "cannot read the fingerprint of $public"
[[ $fingerprint =~ ^SHA256:[A-Za-z0-9+/]{43}$ ]] ||
  die "$public has no SHA256 fingerprint: '$fingerprint'"

digest=
if [ -n "$digest_file" ]; then
  [ -f "$digest_file" ] && [ -r "$digest_file" ] || die "cannot read the image digest $digest_file"
  digest=$(head -c 72 "$digest_file")
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] && [ "$(wc -c <"$digest_file")" -eq 72 ] &&
    [ "$(tail -c 1 "$digest_file" | od -An -c | tr -d ' ')" = '\n' ] ||
    die "$digest_file is not one line 'sha256:<64 hex digits>'"
fi

# The lines under `## <version>` up to the next `## ` heading outside a
# code block, without the blank lines around them. Exit 3: no such
# heading; exit 4: more than one.
status=0
section=$(awk -v want="## $version" '
  /^(```|~~~)/ { fence = !fence }
  !fence && /^## / {
    sub(/[[:space:]]+$/, "")
    inside = ($0 == want)
    if (inside) found++
    next
  }
  inside { lines[n++] = $0 }
  END {
    if (found == 0) exit 3
    if (found > 1) exit 4
    first = 0; while (first < n && lines[first] ~ /^[[:space:]]*$/) first++
    last = n - 1; while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
    for (i = first; i <= last; i++) print lines[i]
  }
' "$changelog") || status=$?
case $status in
  0) ;;
  3) die "$changelog has no section '## $version' for $tag" ;;
  4) die "$changelog has more than one section '## $version'" ;;
  *) die "cannot read the section '## $version' of $changelog" ;;
esac
grep -Evq '^[[:space:]]*$|^Released [0-9]{4}-[0-9]{2}-[0-9]{2}\.$' <<<"$section" ||
  die "the section '## $version' of $changelog is empty"

# A link into the repository, such as [x](docs/guide/install.md#a), is
# relative to CHANGELOG.md at the root: point it at the tag's files. A link
# with a scheme, an anchor or an absolute path is left as it is.
section=$(sed -E "s|\]\(([^)#:/][^):]*)\)|]($repo/blob/$tag/\1)|g" <<<"$section")

printf '%s\n' "$section"
printf '\n---\n\n'
printf -- '- Download and verify: %s/releases/%s/\n' "$site" "$tag"
# shellcheck disable=SC2016 # Markdown code spans, not command substitutions
printf -- '- Release key fingerprint: `%s`. Check it as [Verify a release by hand](%s/guide/install.html#verify-a-release-by-hand) says.\n' \
  "$fingerprint" "$site"
if [ -n "$digest" ]; then
  # shellcheck disable=SC2016 # a Markdown code span
  printf -- '- Container image: `%s@%s`\n' "$image" "$digest"
fi
printf -- '- These notes on oskalang.dev: %s/changelog.html#%s\n' "$site" "$anchor"
