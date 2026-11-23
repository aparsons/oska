#!/usr/bin/env bash
# Tests scripts/x509-roots/generate.py against a cut of certdata.txt: the
# output is the golden file, is the same on a second run, keeps only the roots
# trusted for server authentication, and the committed std.x509_roots module
# says how many roots it holds and names its source.

set -euo pipefail
cd "$(dirname "$0")/../.."

fail=0
ok() { printf 'ok   %s\n' "$1"; }
bad() {
  printf 'FAIL %s\n' "$1"
  fail=1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fixture=scripts/x509-roots/fixture
python3 -I scripts/x509-roots/generate.py "$fixture/certdata.txt" NSS_3_134_RTM 2.94 >"$work/one.osk"
python3 -I scripts/x509-roots/generate.py "$fixture/certdata.txt" NSS_3_134_RTM 2.94 >"$work/two.osk"

if cmp -s "$work/one.osk" "$fixture/x509_roots.osk"; then ok "the output is the golden file"; else
  bad "the output differs from $fixture/x509_roots.osk (diff follows)"
  diff "$work/one.osk" "$fixture/x509_roots.osk" | cut -c1-200 | head -20
fi
if cmp -s "$work/one.osk" "$work/two.osk"; then ok "a second run gives the same bytes"; else bad "a second run differs"; fi

roots=$(grep -c '^        Root(name: ' "$work/one.osk" || true)
if [ "$roots" = 2 ]; then ok "only the roots trusted for server authentication are kept"; else bad "expected 2 roots, got $roots"; fi
if grep -q 'Explicitly Distrust DigiNotar' "$work/one.osk"; then bad "a root that is not trusted was kept"; else ok "a root that is not trusted is left out"; fi
if grep -q 'GlobalSign Root CA' "$work/one.osk"; then bad "a root that must be verified was kept"; else ok "a root that must be verified is left out"; fi
if grep -q 'distrust_after: [1-9]' "$work/one.osk"; then ok "a distrust-after date is kept"; else bad "no distrust-after date"; fi

module=crates/oska-check/src/std/x509_roots.osk
count=$(grep -c '^        Root(name: ' "$module" || true)
if sed -n 3p "$module" | grep -q "^// $count roots trusted for TLS server authentication"; then ok "the committed module's count line matches its roots"; else bad "the count line of $module is wrong"; fi
if sed -n 2p "$module" | grep -Eq "NSS release NSS_[0-9_]+_RTM \(builtins [0-9.]+\), SHA-256 [0-9a-f]{64}\.$"; then ok "the committed module names its NSS release and the SHA-256 of its certdata.txt"; else bad "line 2 of $module should name the NSS release and the source's SHA-256"; fi
if grep -Eq '^    "NSS_[0-9_]+_RTM"$' "$module" && grep -Eq '^    "[0-9]+\.[0-9]+"$' "$module"; then ok "the committed module has the NSS release as a constant"; else bad "$module has no nss_release constant"; fi
if grep -q 'ski: "[0-9a-f]' "$work/one.osk"; then ok "a subject key identifier is kept"; else bad "no subject key identifier in the output"; fi
if python3 -I scripts/x509-roots/generate.py "$fixture/certdata.txt" master 2.94 >/dev/null 2>&1; then bad "a release that is not an NSS tag was accepted"; else ok "a release that is not an NSS tag is refused"; fi

exit "$fail"
