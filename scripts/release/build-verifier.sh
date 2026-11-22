#!/usr/bin/env bash
# Builds the Oska verifier (verify.osk) for a public key other than the
# release key: build-verifier.sh OSKAC PUBKEY_FILE OUT.
#
# verify.osk pins the real release key as a literal. This copies it to a
# temporary file with the one pinned key line replaced by PUBKEY_FILE's
# line, and builds that copy with OSKAC. The repository file is not
# touched. Fails when the pinned line is not found exactly once, when the
# key is not replaced, or when the build fails. Used by the release dry run
# (throwaway key) and by test.sh.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

[ "$#" -eq 3 ] || { echo "usage: build-verifier.sh OSKAC PUBKEY_FILE OUT" >&2; exit 2; }
oskac=$1 pub=$2 out=$3

[ -x "$oskac" ] || { echo "build-verifier.sh: no oskac at $oskac" >&2; exit 1; }
[ -f "$pub" ] || { echo "build-verifier.sh: no public key file $pub" >&2; exit 1; }

pinned=$(sed -n 's/^    "\(ssh-ed25519 [^"]*\)"$/\1/p' "$here/verify.osk")
if [ "$(printf '%s\n' "$pinned" | grep -c .)" -ne 1 ]; then
  echo "build-verifier.sh: verify.osk does not pin exactly one key line" >&2
  exit 1
fi
new=$(tr -d '\n' <"$pub")

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
src=$(<"$here/verify.osk")
printf '%s\n' "${src/"$pinned"/"$new"}" >"$tmp/verify.osk"
if [ "$pinned" != "$new" ] && cmp -s "$here/verify.osk" "$tmp/verify.osk"; then
  echo "build-verifier.sh: the key was not replaced" >&2
  exit 1
fi
"$oskac" build "$tmp/verify.osk" -o "$out" ||
  { echo "build-verifier.sh: oskac build of verify.osk failed" >&2; exit 1; }
