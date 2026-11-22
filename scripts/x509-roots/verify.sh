#!/usr/bin/env bash
# Checks that the committed std.x509_roots module is what generate.py makes from
# the pinned NSS release: fetches certdata.txt and nssckbi.h at the release tag
# the module names, checks the SHA-256 in its header, regenerates the module and
# compares. It needs the network, so check.sh does not run it; run it when the
# roots are updated and in the release runbook (docs/agents/release.md).

set -euo pipefail
cd "$(dirname "$0")/../.."

module=crates/oska-check/src/std/x509_roots.osk
release=$(sed -n 's/^    "\(NSS_[0-9_]*_RTM\)"$/\1/p' "$module")
builtins=$(grep -A1 'pub fn nss_builtins_version' "$module" | sed -n 's/^    "\([0-9.]*\)"$/\1/p')
[ -n "$release" ] && [ -n "$builtins" ] || {
  echo "x509_roots.osk names no NSS release" >&2
  exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
base="https://raw.githubusercontent.com/nss-dev/nss/$release/lib/ckfw/builtins"
curl -fsS "$base/certdata.txt" -o "$work/certdata.txt"
curl -fsS "$base/nssckbi.h" -o "$work/nssckbi.h"

actual=$(sed -n 's/^#define NSS_BUILTINS_LIBRARY_VERSION "\(.*\)"$/\1/p' "$work/nssckbi.h")
[ "$actual" = "$builtins" ] || {
  echo "NSS_BUILTINS_LIBRARY_VERSION at $release is $actual, the module says $builtins" >&2
  exit 1
}
python3 -I scripts/x509-roots/generate.py "$work/certdata.txt" "$release" "$builtins" >"$work/roots.osk"
if cmp -s "$work/roots.osk" "$module"; then
  echo "ok   $module is what generate.py makes from $release"
else
  echo "FAIL $module differs from what generate.py makes from $release" >&2
  diff "$work/roots.osk" "$module" | cut -c1-200 | head -20 >&2
  exit 1
fi
