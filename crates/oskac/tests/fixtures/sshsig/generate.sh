#!/usr/bin/env bash
# The commands that made the fixtures in this directory (#234). No test
# runs this script or ssh-keygen: the outputs are committed, and the
# tests read them. The key is a throwaway made only for the tests.
#
#   test_key / test_key.pub   ssh-keygen -t ed25519 -N '' -C oska-test-key -f test_key
#   message.txt               the signed message
#   message.sha512.sig        signature in namespace oska-test, default hash
#   message.sha256.sig        the same with -O hashalg=sha256
#   other_key.pub             a second key, for the wrong-key tests
#   releases/<tag>/           SHA256SUMS and SHA256SUMS.sig of each
#                             published release, downloaded from its
#                             GitHub release assets (see the loop below)
set -euo pipefail
cd "$(dirname "$0")"

ssh-keygen -q -t ed25519 -N '' -C oska-test-key -f test_key
ssh-keygen -q -t ed25519 -N '' -C other-key -f other_key
rm other_key
printf 'Oska signs this message for the sshsig tests.\n' > message.txt
ssh-keygen -Y sign -q -f test_key -n oska-test message.txt
mv message.txt.sig message.sha512.sig
ssh-keygen -Y sign -q -f test_key -n oska-test -O hashalg=sha256 message.txt
mv message.txt.sig message.sha256.sig
rm test_key # the private half is never committed

for tag in v0.1.0 v0.2.0 v0.3.0 v0.4.0 v0.5.0 v0.6.0; do
  mkdir -p "releases/$tag"
  for asset in SHA256SUMS SHA256SUMS.sig; do
    id=$(gh api "repos/aparsons/oska/releases/tags/$tag" --jq ".assets[]|select(.name==\"$asset\")|.id")
    gh api -H 'Accept: application/octet-stream' "repos/aparsons/oska/releases/assets/$id" > "releases/$tag/$asset"
  done
done

# Checked against OpenSSH when they were made:
#   ssh-keygen -l -f test_key.pub
#   ssh-keygen -l -f ../../../../../site/oska-release.pub
#   ssh-keygen -Y verify -f <allowed_signers> -I <principal> -n oska-test -s message.sha512.sig < message.txt
