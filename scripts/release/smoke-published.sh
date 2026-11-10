#!/usr/bin/env bash
# Check that the published one-line install works end to end.
#
#   scripts/release/smoke-published.sh <site-url> [<expected-version>]
#
# Runs what the install guide tells a user to run,
#
#   curl -fsSL <site-url>/install.sh | sh -s -- --json
#
# into a scratch OSKA_HOME, and checks that:
#   - the installer printed one line of JSON with ok, checksum_verified and
#     signature_verified all true;
#   - the version it installed is <expected-version>, or, if none is given,
#     the version latest.json names;
#   - the installed binary runs and reports that version, in plain text and
#     with --format=json.
#
# With no release published (latest.json has no "tag"), it says so and
# succeeds when no <expected-version> is given, and fails when one is.
#
# <site-url> is https://oskalang.dev for the live site; it may be a file://
# URL for tests. Downloads are retried, as a fresh deploy takes a moment to
# reach every edge. Needs curl and jq.

set -euo pipefail

die() {
  echo "smoke-published.sh: $*" >&2
  exit 1
}

[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: smoke-published.sh <site-url> [<expected-version>]"
site=${1%/}
expected=${2:-}
expected=${expected#v}

fetch() {
  curl --silent --show-error --fail --location \
    --retry 6 --retry-delay 10 --retry-all-errors "$1"
}

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

latest=$(fetch "$site/latest.json") || die "cannot fetch $site/latest.json"
tag=$(printf '%s' "$latest" | jq -r '.tag // empty') || die "$site/latest.json is not JSON"
if [ -z "$tag" ]; then
  [ -z "$expected" ] || die "latest.json names no release, but $expected was expected"
  echo "no release is published at $site; nothing to install"
  exit 0
fi
published=$(printf '%s' "$latest" | jq -r '.version // empty')
[ -n "$published" ] || die "latest.json has a tag, $tag, but no version"
[ -n "$expected" ] || expected=$published
[ "$published" = "$expected" ] ||
  die "latest.json names version $published, not $expected"

# The same pipe a user types. The installer's own output is the JSON.
installer=$(fetch "$site/install.sh") || die "cannot fetch $site/install.sh"
status=0
result=$(printf '%s\n' "$installer" |
  OSKA_HOME="$scratch/oska" OSKA_SITE_URL="$site" sh -s -- --json) || status=$?
printf '%s\n' "$result"
[ "$status" -eq 0 ] || die "the installer exited with status $status"

[ "$(printf '%s\n' "$result" | wc -l | tr -d ' ')" = 1 ] ||
  die "the installer printed more than one line"
field() { printf '%s' "$result" | jq -r "$1" 2>/dev/null || die "not JSON: $result"; }
[ "$(field .ok)" = true ] || die "ok is not true"
[ "$(field .checksum_verified)" = true ] || die "the checksum was not verified"
[ "$(field .signature_verified)" = true ] || die "the signature was not verified"
[ "$(field .version)" = "$expected" ] || die "installed $(field .version), not $expected"
[ "$(field .tag)" = "v$expected" ] || die "installed tag $(field .tag), not v$expected"

binary=$(field .path)
[ -x "$binary" ] || die "$binary is not an executable file"
version=$("$binary" --version) || die "$binary --version failed"
[ "$version" = "oskac $expected" ] || die "--version printed '$version'"
json=$("$binary" --version --format=json) || die "$binary --version --format=json failed"
[ "$(printf '%s' "$json" | jq -r .version)" = "$expected" ] ||
  die "--version --format=json printed '$json'"
echo "ok: $site installs oskac $expected, and it runs"
