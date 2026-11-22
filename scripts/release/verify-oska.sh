#!/usr/bin/env bash
# Run the Oska release verifier (verify.osk) on a release directory.
#
#   OSKA_VERIFY=<built verify.osk> scripts/release/verify-oska.sh <release-dir> [--format=json]
#
# OSKA_VERIFY is the program `oskac build scripts/release/verify.osk -o <file>`
# made. It pins the release key in its source, so there is no key argument.
# The program reads its directory relative to its working directory, and
# refuses an absolute path, so this changes into the directory's parent and
# passes the directory's name, which the program takes as the tag, as
# verify.sh takes the tag from the name too. Output and exit code are the
# program's: 0 verified, 1 refused. A missing verifier, or a directory that
# cannot be entered, refuses (exit 1); it never skips. A release directory
# that is a symlink is refused by the program's directory capability, which
# follows no symlink (verify.sh would follow it).

set -euo pipefail

die() {
  echo "verify-oska.sh: $*" >&2
  exit 1
}

[ $# -ge 1 ] || die "usage: verify-oska.sh <release-dir> [--format=json]"
dir=$1
shift
[ -n "${OSKA_VERIFY:-}" ] || die "OSKA_VERIFY is not set: build scripts/release/verify.osk with oskac build and point it at the result"
[ -x "$OSKA_VERIFY" ] || die "OSKA_VERIFY=$OSKA_VERIFY is not an executable file"
[ -d "$dir" ] || die "no release directory $dir"

name=$(basename "$(cd "$dir" && pwd)")
parent=$(cd "$dir/.." && pwd)
verifier=$(cd "$(dirname "$OSKA_VERIFY")" && pwd)/$(basename "$OSKA_VERIFY")
cd "$parent"
exec "$verifier" "$name" "$@"
