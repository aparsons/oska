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

# Assembles and signs ./out/<tag> (default v1.2.3) with the key ./key,
# making the fake binaries and the key if they are not there yet.
signed_release() {
  local tag=${1:-v1.2.3}
  [ -d builds ] || fake_builds
  [ -f key ] || throwaway_key key
  "$here/assemble.sh" "$tag" builds out
  "$here/sign.sh" "out/$tag" key key.pub
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

test_assemble_binds_the_tag_into_the_checksums_it_signs() {
  fake_builds
  "$here/assemble.sh" v1.2.3 builds out
  [ "$(head -n 1 out/v1.2.3/SHA256SUMS)" = '# oska-release v1.2.3' ] ||
    fail "SHA256SUMS does not start with the tag line: $(head -n 1 out/v1.2.3/SHA256SUMS)"
  [ "$(grep -c '^#' out/v1.2.3/SHA256SUMS)" = 1 ] || fail "more than one comment line"
}

# The rollback attack: a genuine, signed old release published again under
# a newer tag. Its signature is valid, so only the signed tag line gives it
# away.
test_verify_refuses_a_signed_release_moved_to_another_tag() {
  signed_release v0.1.0
  mv out/v0.1.0 out/v9.9.9
  if "$here/verify.sh" out/v9.9.9 key.pub 2>err; then
    fail "verified v0.1.0's signed files as v9.9.9"
  fi
  grep -q 'v0.1.0' err || fail "error does not name the signed tag: $(cat err)"
}

test_verify_refuses_signed_checksums_that_name_no_tag() {
  signed_release v1.2.3
  # Signed by the real key, but in the old format with no tag line.
  sed -i '/^#/d' out/v1.2.3/SHA256SUMS
  rm out/v1.2.3/SHA256SUMS.sig
  ssh-keygen -q -Y sign -f key -n oska-release out/v1.2.3/SHA256SUMS
  if "$here/verify.sh" out/v1.2.3 key.pub; then
    fail "verified a SHA256SUMS that names no tag"
  fi
}

test_verify_refuses_a_checksum_line_that_is_not_a_digest_and_a_name() {
  signed_release v1.2.3
  printf '# a second comment\n' >>out/v1.2.3/SHA256SUMS
  rm out/v1.2.3/SHA256SUMS.sig
  ssh-keygen -q -Y sign -f key -n oska-release out/v1.2.3/SHA256SUMS
  if "$here/verify.sh" out/v1.2.3 key.pub; then
    fail "verified a SHA256SUMS with a stray line"
  fi
}

test_stage_site_refuses_an_old_release_republished_under_a_newer_tag() {
  signed_release v0.1.0
  cp -R out/v0.1.0 out/v9.9.9
  mkdir site
  if "$here/stage-site.sh" out site key.pub 2>err; then
    fail "staged v0.1.0's signed files as v9.9.9"
  fi
  [ ! -e site/releases/v9.9.9 ] || fail "copied the mislabelled release"
  [ ! -e site/latest.json ] || fail "wrote a latest.json"
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

test_stage_site_publishes_releases_and_points_latest_at_the_newest() {
  signed_release v1.2.3
  signed_release v1.10.0
  signed_release v2.0.0-rc.1
  mkdir site
  OSKA_SITE_URL=https://example.test "$here/stage-site.sh" out site key.pub
  cmp key.pub site/oska-release.pub || fail "public key not published"
  # What is served verifies, beside the one page the site adds.
  for tag in v1.2.3 v1.10.0 v2.0.0-rc.1; do
    mkdir served
    cp -R "site/releases/$tag" "served/$tag"
    rm "served/$tag/index.html"
    "$here/verify.sh" "served/$tag" key.pub
    rm -r served
  done
  cat >expected.json <<'JSON'
{
  "version": "1.10.0",
  "tag": "v1.10.0",
  "release_url": "https://example.test/releases/v1.10.0/",
  "checksums_url": "https://example.test/releases/v1.10.0/SHA256SUMS",
  "signature_url": "https://example.test/releases/v1.10.0/SHA256SUMS.sig",
  "signing_key_url": "https://example.test/oska-release.pub",
  "signing_namespace": "oska-release",
  "installers": {
    "sh": "https://example.test/install.sh",
    "ps1": "https://example.test/install.ps1"
  },
  "binaries": {
    "linux-aarch64": "https://example.test/releases/v1.10.0/oskac-linux-aarch64",
    "linux-x86_64": "https://example.test/releases/v1.10.0/oskac-linux-x86_64",
    "macos-aarch64": "https://example.test/releases/v1.10.0/oskac-macos-aarch64",
    "macos-x86_64": "https://example.test/releases/v1.10.0/oskac-macos-x86_64",
    "windows-x86_64": "https://example.test/releases/v1.10.0/oskac-windows-x86_64.exe"
  }
}
JSON
  diff <(jq -S . expected.json) <(jq -S . site/latest.json) || fail "latest.json differs"
  # release_url (and the release notes) link to a page, not a bare folder.
  page=site/releases/v1.10.0/index.html
  [ -f "$page" ] || fail "no page at the release's URL"
  grep -qF '<title>oskac 1.10.0</title>' "$page" || fail "page names the wrong release"
  grep -qF 'href="oskac-linux-x86_64"' "$page" || fail "page does not link a binary"
  grep -qF "$(sed -n 2p site/releases/v1.10.0/SHA256SUMS | awk '{print $1}')" "$page" ||
    fail "page does not show a checksum"
  grep -qF 'https://example.test/oska-release.pub' "$page" || fail "page does not link the key"
}

test_stage_site_keeps_what_the_generator_wrote_in_latest_json() {
  signed_release v1.2.3
  mkdir site
  printf '{"docs": "main", "version": "stale"}\n' >site/latest.json
  "$here/stage-site.sh" out site key.pub
  [ "$(jq -r .docs site/latest.json)" = main ] || fail "lost the generator's field"
  [ "$(jq -r .version site/latest.json)" = 1.2.3 ] || fail "release version did not win"
  [ "$(jq -r '.binaries["linux-x86_64"]' site/latest.json)" = \
    https://oskalang.dev/releases/v1.2.3/oskac-linux-x86_64 ] || fail "wrong default site URL"
}

test_stage_site_publishes_installers_pinned_to_the_key_that_installs_from_it() {
  working_builds 1.2.3
  signed_release v1.2.3
  mkdir site home
  "$here/stage-site.sh" out site key.pub
  pinned=$(awk '{print $1, $2}' key.pub)
  grep -qF "'$pinned'" site/install.sh || fail "install.sh is not pinned to key.pub"
  grep -qF "'$pinned'" site/install.ps1 || fail "install.ps1 is not pinned to key.pub"
  HOME="$PWD/home" OSKA_SITE_URL="file://$PWD/site" sh site/install.sh --json >out.json
  [ "$(jq -r .version out.json)" = 1.2.3 ] || fail "did not install 1.2.3"
  case "$(uname -m)" in
    aarch64 | arm64) asset=oskac-linux-aarch64 ;;
    *) asset=oskac-linux-x86_64 ;;
  esac
  cmp "out/v1.2.3/$asset" home/.oska/bin/oskac || fail "installed the wrong binary"
}

test_stage_site_refuses_a_release_that_does_not_verify() {
  signed_release v1.2.3
  printf 'evil\n' >>out/v1.2.3/oskac-windows-x86_64.exe
  mkdir site
  if "$here/stage-site.sh" out site key.pub; then
    fail "staged a tampered release"
  fi
  [ ! -e site/releases/v1.2.3/oskac-windows-x86_64.exe ] || fail "copied the tampered binary"
}

test_stage_site_without_releases_or_key_stages_nothing() {
  mkdir site out
  "$here/stage-site.sh" out site missing.pub
  [ -z "$(ls -A site)" ] || fail "staged something from nothing"
}

test_stage_site_refuses_releases_without_a_key_to_check_them() {
  signed_release v1.2.3
  mkdir site
  if "$here/stage-site.sh" out site missing.pub; then
    fail "staged releases it could not verify"
  fi
}

# Writes working fake binaries into ./builds: scripts that answer
# --version, in text and JSON, with <version>, as the real oskac does.
working_builds() {
  local version=$1
  mkdir -p builds
  for asset in oskac-linux-x86_64 oskac-linux-aarch64 oskac-macos-x86_64 \
    oskac-macos-aarch64 oskac-windows-x86_64.exe; do
    cat >"builds/$asset" <<BIN
#!/bin/sh
case "\$*" in
  *--format=json*) echo '{"name":"oskac","version":"$version"}' ;;
  *) echo "oskac $version" ;;
esac
BIN
    chmod +x "builds/$asset"
  done
}

# Stages a signed release of working fake binaries into ./site, as
# pages.yml does, with URLs that point at ./site.
staged_site() {
  local version=${1:-1.2.3}
  mkdir -p site
  working_builds "$version"
  throwaway_key key
  "$here/assemble.sh" "v$version" builds out
  "$here/sign.sh" "out/v$version" key key.pub
  OSKA_SITE_URL="file://$PWD/site" "$here/stage-site.sh" out site key.pub
}

test_smoke_published_installs_the_latest_release_and_runs_it() {
  staged_site 1.2.3
  "$here/smoke-published.sh" "file://$PWD/site" >smoke.out
  grep -q '^ok: .* installs oskac 1.2.3' smoke.out || fail "no success line"
  "$here/smoke-published.sh" "file://$PWD/site" v1.2.3 >/dev/null
}

test_smoke_published_fails_when_the_published_version_is_not_the_expected_one() {
  staged_site 1.2.3
  if "$here/smoke-published.sh" "file://$PWD/site" 0.1.0; then
    fail "accepted 1.2.3 as 0.1.0"
  fi
}

test_smoke_published_fails_when_a_published_binary_was_altered() {
  staged_site 1.2.3
  for asset in site/releases/v1.2.3/oskac-*; do
    printf 'evil\n' >>"$asset"
  done
  if "$here/smoke-published.sh" "file://$PWD/site" >smoke.out 2>&1; then
    fail "installed a binary that does not match SHA256SUMS"
  fi
  grep -q checksum_mismatch smoke.out || fail "did not fail on the checksum: $(cat smoke.out)"
}

test_smoke_published_with_no_release_succeeds_unless_a_version_is_expected() {
  mkdir site
  printf '{"version": "0.1.0", "docs": "main"}\n' >site/latest.json
  "$here/smoke-published.sh" "file://$PWD/site" >smoke.out
  grep -q 'no release is published' smoke.out || fail "did not say there is no release"
  # In a workflow log, a passing smoke with nothing installed must stand out.
  grep -q '^::warning::.*no release is published' smoke.out || fail "no GitHub warning annotation"
  if "$here/smoke-published.sh" "file://$PWD/site" 0.1.0; then
    fail "passed with no release but a version expected"
  fi
}

for t in $(declare -F | awk '{print $3}' | grep '^test_'); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
