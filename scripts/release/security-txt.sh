#!/usr/bin/env bash
# Write the site's security.txt (RFC 9116).
#
#   scripts/release/security-txt.sh <site-dir>
#
# Writes <site-dir>/.well-known/security.txt, which tells a security
# researcher, or their tooling, where to report a vulnerability:
#   - Contact: GitHub private vulnerability reporting, the only channel
#     (SECURITY.md);
#   - Expires: one year from now, so the file never goes stale while the
#     site is deployed at least once a year;
#   - Policy: the security page the site generator publishes from
#     SECURITY.md;
#   - Canonical: where this file is served;
#   - Preferred-Languages: en.
#
# The Pages workflow runs this after the generator, which has no clock and
# cannot write a hidden directory. URLs start with $OSKA_SITE_URL (default
# https://oskalang.dev), so tests can write a site served elsewhere. There
# is no PGP signature: releases are signed with an SSH key, and RFC 9116
# makes the signature optional.
#
# check-security-txt.sh is the check the deploy runs on the result.

set -euo pipefail

die() {
  echo "security-txt.sh: $*" >&2
  exit 1
}

[ $# -eq 1 ] || die "usage: security-txt.sh <site-dir>"
site=$1
base=${OSKA_SITE_URL:-https://oskalang.dev}
base=${base%/}

[ -d "$site" ] || die "no site directory $site"

expires=$(date -u -d '+1 year' +%Y-%m-%dT%H:%M:%SZ) || die "cannot work out a date a year from now"

mkdir -p "$site/.well-known"
cat >"$site/.well-known/security.txt" <<TXT
Contact: https://github.com/aparsons/oska/security/advisories/new
Expires: $expires
Policy: $base/security.html
Canonical: $base/.well-known/security.txt
Preferred-Languages: en
TXT
