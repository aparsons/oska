#!/bin/sh
# Pin the installers to a release signing key.
#
#   scripts/install/pin.sh <public-key> <out-dir> [--require-index]
#
# Writes <out-dir>/install.sh and <out-dir>/install.ps1: copies of the
# installers next to this script with the placeholder @OSKA_RELEASE_KEY@
# replaced by <public-key>'s key type and material (its comment is dropped).
# An installer that was never pinned refuses to install anything.
#
# The placeholder @OSKA_INDEX_REQUIRED@ becomes `yes` with --require-index
# and `no` without. An installer pinned with `yes` takes the latest release
# only from the signed release index (release-index.json), never from
# latest.json; stage-site.sh passes the flag when it publishes a verified
# index.
#
# <public-key> must be one ed25519 OpenSSH public key, as sign.sh uses.

set -eu

here=$(cd "$(dirname "$0")" && pwd)

die() {
  echo "pin.sh: $*" >&2
  exit 1
}

usage="usage: pin.sh <public-key> <out-dir> [--require-index]"
require=no
case $# in
  2) ;;
  3)
    [ "$3" = --require-index ] || die "$usage"
    require=yes ;;
  *) die "$usage" ;;
esac
public=$1
out=$2

[ -f "$public" ] || die "no public key at $public"
[ "$(wc -l <"$public" | tr -d ' ')" = 1 ] || die "$public must hold exactly one key"
key=$(awk '{print $1, $2}' "$public")
# Base64 has no '|', so the key is safe in the sed replacement below.
printf '%s\n' "$key" | grep -Eqx 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5[A-Za-z0-9+/]+=*' ||
  die "$public is not an ed25519 OpenSSH public key"

mkdir -p "$out"
for f in install.sh install.ps1; do
  grep -q '@OSKA_RELEASE_KEY@' "$here/$f" || die "$here/$f has no key placeholder"
  grep -q '@OSKA_INDEX_REQUIRED@' "$here/$f" || die "$here/$f has no index placeholder"
  sed -e "s|@OSKA_RELEASE_KEY@|$key|" -e "s|@OSKA_INDEX_REQUIRED@|$require|" "$here/$f" >"$out/$f.tmp"
  mv "$out/$f.tmp" "$out/$f"
done
