#!/usr/bin/env bash
# Check the security.txt a deployed site serves.
#
#   scripts/release/check-security-txt.sh <site-url>
#
# Fetches <site-url>/.well-known/security.txt and fails, naming the
# problem, if it cannot be fetched, if it lacks any of the fields
# security-txt.sh writes (Contact, Expires, Policy, Canonical,
# Preferred-Languages), or if its Expires cannot be read as a time or is
# already past. A stale security.txt is a defect, so the post-deploy check
# in the Pages workflow runs this.
#
# <site-url> is https://oskalang.dev for the live site; it may be a file://
# URL for tests. The download is retried $OSKA_FETCH_RETRIES times (default
# 6), as a fresh deploy takes a moment to reach every edge. Needs curl and
# GNU date.

set -euo pipefail

die() {
  echo "check-security-txt.sh: $*" >&2
  exit 1
}

[ $# -eq 1 ] || die "usage: check-security-txt.sh <site-url>"
url="${1%/}/.well-known/security.txt"
retries=${OSKA_FETCH_RETRIES:-6}

text=$(curl --silent --show-error --fail --location \
  --retry "$retries" --retry-delay 10 --retry-all-errors "$url") ||
  die "cannot fetch $url"

field() {
  printf '%s\n' "$text" | sed -n "s/^$1: //p"
}

for name in Contact Expires Policy Canonical Preferred-Languages; do
  [ -n "$(field "$name")" ] || die "$url has no $name field"
done

expires=$(field Expires)
[ "$(printf '%s\n' "$expires" | wc -l)" = 1 ] || die "$url has more than one Expires field"
# RFC 9116 dates are RFC 3339 internet date-times. GNU date alone would
# also read "next year".
rfc3339='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})$'
[[ $expires =~ $rfc3339 ]] || die "$url has an Expires that is not an RFC 3339 time: $expires"
expires_at=$(date -u -d "$expires" +%s 2>/dev/null) ||
  die "$url has an Expires that is not a time: $expires"
[ "$expires_at" -gt "$(date -u +%s)" ] || die "$url expired at $expires"

echo "ok: $url expires $expires"
