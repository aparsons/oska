#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# The commands that made the fixtures in this directory (#258). No test runs
# this script or ssh-keygen: the outputs are committed, and the `oskac self
# check` tests read them through the loopback TLS test server. The key is a
# throwaway made for the tests only, and its private half is never committed.
#
#   test_key.pub            the key the test build of `oskac` pins
#   exe/genuine, exe/modified
#                           stand-ins for the running `oskac` (the test
#                           build takes its executable from OSKAC_TEST_EXE)
#   site/<case>/            what the site serves: release-index.json(.sig)
#                           in the namespace oska-index, and
#                           releases/v<X.Y.Z>/SHA256SUMS(.sig) in
#                           oska-release, one directory per case
set -euo pipefail
cd "$(dirname "$0")"

rm -rf site exe test_key test_key.pub
ssh-keygen -q -t ed25519 -N '' -C oska-test-key -f test_key
mkdir -p exe
printf 'a stand-in for the running oskac, as released\n' > exe/genuine
printf 'a stand-in for the running oskac, as released, then edited\n' > exe/modified
sum=$(sha256sum exe/genuine | cut -d' ' -f1)

# index <dir> <serial> <issued> <expires> <latest> <namespace>
index() {
  mkdir -p "$1"
  printf '{"schema": 1, "serial": %s, "issued": "%s", "expires": "%s", "latest": {"tag": "v%s", "version": "%s"}}\n' \
    "$2" "$3" "$4" "$5" "$5" > "$1/release-index.json"
  ssh-keygen -Y sign -q -f test_key -n "$6" "$1/release-index.json"
}

# release <dir> <version>
release() {
  mkdir -p "$1/releases/v$2"
  printf '# oska-release v%s\n%s  oskac-fixture\n' "$2" "$sum" > "$1/releases/v$2/SHA256SUMS"
  ssh-keygen -Y sign -q -f test_key -n oska-release "$1/releases/v$2/SHA256SUMS"
}

for name in ok soon expired wrong_namespace tampered_index bad_checksums; do
  case $name in
    soon) expires=2030-01-10T00:00:00Z ;;
    expired) expires=2020-01-01T00:00:00Z ;;
    *) expires=2099-01-01T00:00:00Z ;;
  esac
  ns=oska-index
  [ "$name" = wrong_namespace ] && ns=oska-release
  index "site/$name" 5 2019-12-01T00:00:00Z "$expires" 0.7.0 "$ns"
  release "site/$name" 0.6.0
  release "site/$name" 0.7.0
done
# A signed index changed after signing, and a signed SHA256SUMS changed after
# signing.
sed -i 's/"serial": 5/"serial": 6/' site/tampered_index/release-index.json
sed -i "s/$sum/$(printf '%064d' 0)/" site/bad_checksums/releases/v0.6.0/SHA256SUMS

rm test_key # the private half is never committed
