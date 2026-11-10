#!/usr/bin/env bash
# Tests for the release scripts. Run from anywhere:
#
#   scripts/release/test.sh
#
# Each test works in its own scratch directory: it makes fake binaries, runs
# the scripts the release and pages workflows run, and checks what they
# produce.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

failures=0
passes=0

fail() {
  echo "    $*" >&2
  return 1
}

run_test() {
  local name=$1
  local dir="$scratch/$name"
  mkdir -p "$dir"
  # Run outside any `if`, `&&` or `||` so that `set -e` holds inside the
  # test: a test stops at its first failing command.
  local status
  set +e
  (
    set -e
    cd "$dir"
    "$name"
  ) >"$dir.log" 2>&1
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    passes=$((passes + 1))
    echo "ok   $name"
  else
    failures=$((failures + 1))
    echo "FAIL $name"
    sed 's/^/    /' "$dir.log"
  fi
}

# Writes one fake binary per release target into ./builds.
fake_builds() {
  mkdir -p builds
  for asset in oskac-linux-x86_64 oskac-linux-aarch64 oskac-macos-x86_64 \
    oskac-macos-aarch64 oskac-windows-x86_64.exe; do
    printf 'binary for %s\n' "$asset" >"builds/$asset"
  done
}

test_assemble_writes_every_binary_and_their_checksums() {
  fake_builds
  "$here/assemble.sh" v1.2.3 builds out
  (cd out/v1.2.3 && LC_ALL=C ls) >listing
  cat >expected <<'LIST'
SHA256SUMS
oskac-linux-aarch64
oskac-linux-x86_64
oskac-macos-aarch64
oskac-macos-x86_64
oskac-windows-x86_64.exe
LIST
  diff expected listing || fail "unexpected files"
  # The SHA-256 of "binary for oskac-linux-x86_64\n".
  grep -qx '3d729e2e60bed0f14d643cfecf4af7efea56f4bf88745a804071904108af8548  oskac-linux-x86_64' \
    out/v1.2.3/SHA256SUMS || fail "SHA256SUMS lacks the linux-x86_64 line"
  (cd out/v1.2.3 && sha256sum --check --strict SHA256SUMS) || fail "sums do not check"
  [ -x out/v1.2.3/oskac-linux-x86_64 ] || fail "binary not executable"
}

test_assemble_refuses_a_release_missing_a_target() {
  fake_builds
  rm builds/oskac-macos-x86_64
  if "$here/assemble.sh" v1.2.3 builds out 2>err; then
    fail "assembled without macos-x86_64"
  fi
  grep -q oskac-macos-x86_64 err || fail "error does not name the missing binary"
}

test_assemble_refuses_a_tag_that_is_not_a_version() {
  fake_builds
  if "$here/assemble.sh" 1.2.3 builds out 2>err; then
    fail "assembled a tag without the leading v"
  fi
  if "$here/assemble.sh" v1.2.3/../x builds out 2>err; then
    fail "assembled a tag with a path in it"
  fi
}

# Writes a throwaway signing key pair to ./<name> and ./<name>.pub.
throwaway_key() {
  ssh-keygen -q -t ed25519 -N '' -C "$1" -f "$1"
}

# Assembles and signs ./out/v1.2.3 with the key ./key.
signed_release() {
  fake_builds
  throwaway_key key
  "$here/assemble.sh" v1.2.3 builds out
  "$here/sign.sh" out/v1.2.3 key key.pub
}

test_a_signed_release_verifies_with_its_public_key() {
  signed_release
  [ -f out/v1.2.3/SHA256SUMS.sig ] || fail "no SHA256SUMS.sig"
  "$here/verify.sh" out/v1.2.3 key.pub
  # The signature also verifies with plain ssh-keygen, as installers do it.
  printf 'oska-release namespaces="oska-release" %s\n' "$(cat key.pub)" >allowed_signers
  ssh-keygen -Y verify -f allowed_signers -I oska-release -n oska-release \
    -s out/v1.2.3/SHA256SUMS.sig <out/v1.2.3/SHA256SUMS
}

test_verify_refuses_a_tampered_binary() {
  signed_release
  printf 'evil\n' >>out/v1.2.3/oskac-linux-aarch64
  if "$here/verify.sh" out/v1.2.3 key.pub; then
    fail "verified a tampered binary"
  fi
}

test_verify_refuses_tampered_checksums() {
  signed_release
  sed -i 's/^./0/' out/v1.2.3/SHA256SUMS
  if "$here/verify.sh" out/v1.2.3 key.pub; then
    fail "verified a SHA256SUMS the key did not sign"
  fi
}

test_verify_refuses_a_signature_by_another_key() {
  signed_release
  throwaway_key other
  if "$here/verify.sh" out/v1.2.3 other.pub; then
    fail "verified with a key that did not sign"
  fi
}

test_verify_refuses_a_file_the_checksums_do_not_cover() {
  signed_release
  printf 'extra\n' >out/v1.2.3/oskac-linux-riscv64
  if "$here/verify.sh" out/v1.2.3 key.pub; then
    fail "verified a release with an unsigned file in it"
  fi
}

test_sign_refuses_a_key_that_does_not_match_the_published_one() {
  fake_builds
  throwaway_key key
  throwaway_key published
  "$here/assemble.sh" v1.2.3 builds out
  if "$here/sign.sh" out/v1.2.3 key published.pub 2>err; then
    fail "signed with a key other than the published one"
  fi
  [ ! -e out/v1.2.3/SHA256SUMS.sig ] || fail "left a signature behind"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_'); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
