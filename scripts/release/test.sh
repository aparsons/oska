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

# The whole pipeline with an index: the release run signs it beside the
# release (sign-index.sh), stage-site.sh publishes it and pins the
# installers to require it, and the served installer installs the release
# it names with index_verified true, which the smoke test checks.
test_smoke_published_checks_the_index_the_installer_verified() {
  working_builds 1.2.3
  throwaway_key key
  "$here/assemble.sh" v1.2.3 builds out
  "$here/sign.sh" out/v1.2.3 key key.pub
  "$here/sign-index.sh" --tag v1.2.3 key key.pub "file://$PWD/no-site" out/v1.2.3
  mkdir site
  OSKA_SITE_URL="file://$PWD/site" "$here/stage-site.sh" out site key.pub
  [ -f site/release-index.json ] || fail "no index published"
  "$here/smoke-published.sh" "file://$PWD/site" >smoke.out || fail "smoke failed: $(cat smoke.out)"
  grep -q '"index_verified": true' smoke.out || fail "the install did not verify the index: $(cat smoke.out)"
  grep -q 'release index serial 1' smoke.out || fail "did not report the index: $(cat smoke.out)"
  # An installer pinned to require the index, with none served, fails.
  mv site/release-index.json site/gone.json
  if "$here/smoke-published.sh" "file://$PWD/site" >smoke.out 2>&1; then
    fail "passed with the required index missing"
  fi
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

# ---- The signed release index ------------------------------------------------

# Verifies ./<dir>/release-index.json against ./key.pub in namespace <ns>,
# with plain ssh-keygen, as the installers do.
index_verifies_in() {
  local dir=$1 ns=$2
  printf 'oska-index namespaces="%s" %s\n' "$ns" "$(cat key.pub)" >allowed_index
  ssh-keygen -Y verify -f allowed_index -I oska-index -n "$ns" \
    -s "$dir/release-index.json.sig" <"$dir/release-index.json" >/dev/null 2>&1
}

# Seconds since the epoch of an RFC 3339 UTC time.
epoch() {
  date -u -d "$1" +%s
}

test_sign_index_with_none_published_signs_serial_1_naming_the_tag() {
  throwaway_key key
  mkdir out
  "$here/sign-index.sh" --tag v1.2.3 key key.pub "file://$PWD/no-site" out
  index_verifies_in out oska-index || fail "the index does not verify in oska-index"
  if index_verifies_in out oska-release; then
    fail "the index verifies as a release signature"
  fi
  [ "$(jq -r .schema out/release-index.json)" = 1 ] || fail "schema is not 1"
  [ "$(jq -r .serial out/release-index.json)" = 1 ] || fail "serial is not 1"
  [ "$(jq -c .latest out/release-index.json)" = '{"tag":"v1.2.3","version":"1.2.3"}' ] ||
    fail "latest is $(jq -c .latest out/release-index.json)"
  local issued expires now
  issued=$(jq -r .issued out/release-index.json)
  expires=$(jq -r .expires out/release-index.json)
  printf '%s\n%s\n' "$issued" "$expires" | grep -Evq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' &&
    fail "issued or expires is not UTC RFC 3339: $issued $expires"
  now=$(date -u +%s)
  [ $((now - $(epoch "$issued"))) -le 60 ] || fail "issued is not now: $issued"
  [ $(($(epoch "$expires") - $(epoch "$issued"))) = $((90 * 86400)) ] ||
    fail "expires is not 90 days after issued: $issued $expires"
  "$here/verify-index.sh" out key.pub >/dev/null || fail "verify-index.sh refuses it"
}

# Signs ./<dir>/release-index.json as written, with ./<key> in <ns>.
sign_index_as() {
  local dir=$1 key=${2:-key} ns=${3:-oska-index}
  rm -f "$dir/release-index.json.sig"
  ssh-keygen -q -Y sign -f "$key" -n "$ns" "$dir/release-index.json"
}

# Writes and signs an index in ./<dir> by hand: serial <serial>, latest
# <tag>, expiring <expires> (default: in 60 days).
hand_index() {
  local dir=$1 serial=$2 tag=$3 expires=${4:-$(date -u -d '+60 days' +%Y-%m-%dT%H:%M:%SZ)}
  mkdir -p "$dir"
  jq -n --argjson s "$serial" --arg t "$tag" --arg e "$expires" \
    '{schema: 1, serial: $s, issued: "2026-01-01T00:00:00Z", expires: $e,
      latest: {tag: $t, version: ($t | ltrimstr("v"))}}' >"$dir/release-index.json"
  sign_index_as "$dir"
}

test_sign_index_signs_the_published_serial_plus_1_and_never_moves_latest_back() {
  throwaway_key key
  hand_index site 41 v1.3.0
  mkdir out
  "$here/sign-index.sh" --tag v1.4.0 key key.pub "file://$PWD/site" out
  [ "$(jq -r .serial out/release-index.json)" = 42 ] || fail "serial is not 42"
  [ "$(jq -r .latest.tag out/release-index.json)" = v1.4.0 ] || fail "latest is not v1.4.0"
  local tag
  # An older patch release, a pre-release or no tag (a refresh) keep v1.3.0.
  for tag in v1.2.9 v1.10.0-rc.1 ''; do
    rm -f out/*
    "$here/sign-index.sh" ${tag:+--tag "$tag"} key key.pub "file://$PWD/site" out
    [ "$(jq -r .serial out/release-index.json)" = 42 ] || fail "${tag:-refresh}: serial is not 42"
    [ "$(jq -r .latest.tag out/release-index.json)" = v1.3.0 ] ||
      fail "${tag:-refresh}: latest is $(jq -r .latest.tag out/release-index.json)"
  done
  # Version order, not text order.
  hand_index site 7 v1.9.0
  rm -f out/*
  "$here/sign-index.sh" --tag v1.10.0 key key.pub "file://$PWD/site" out
  [ "$(jq -r .latest.tag out/release-index.json)" = v1.10.0 ] || fail "v1.10.0 is not newer than v1.9.0"
}

test_sign_index_refuses_a_key_that_does_not_match_the_published_one() {
  throwaway_key key
  throwaway_key published
  mkdir out
  if "$here/sign-index.sh" --tag v1.2.3 key published.pub "file://$PWD/no-site" out 2>err; then
    fail "signed an index with a key other than the published one"
  fi
  grep -q 'does not match' err || fail "error is $(cat err)"
  [ -z "$(ls -A out)" ] || fail "wrote $(ls -A out)"
}

test_sign_index_refuses_to_sign_over_a_published_index_that_does_not_verify() {
  throwaway_key key
  throwaway_key attacker
  hand_index site 41 v1.3.0
  sign_index_as site attacker
  mkdir out
  if "$here/sign-index.sh" --tag v1.4.0 key key.pub "file://$PWD/site" out 2>err; then
    fail "signed over an index by another key"
  fi
  [ -z "$(ls -A out)" ] || fail "wrote $(ls -A out)"
  rm site/release-index.json.sig
  if "$here/sign-index.sh" --tag v1.4.0 key key.pub "file://$PWD/site" out 2>err; then
    fail "signed over an unsigned index"
  fi
  [ -z "$(ls -A out)" ] || fail "wrote $(ls -A out)"
}

# After a key rotation the published index is signed by the old key, which
# --previous-key names; the new index continues its serial.
test_sign_index_after_a_rotation_continues_the_serial_of_the_old_key() {
  throwaway_key old
  throwaway_key key
  hand_index site 41 v1.3.0
  sign_index_as site old
  mkdir out
  "$here/sign-index.sh" --previous-key old.pub key key.pub "file://$PWD/site" out
  [ "$(jq -r .serial out/release-index.json)" = 42 ] || fail "serial is not 42"
  "$here/verify-index.sh" out key.pub >/dev/null || fail "the new index does not verify with the new key"
}

test_sign_index_with_no_stable_release_signs_nothing() {
  throwaway_key key
  mkdir out
  "$here/sign-index.sh" --tag v0.1.0-rc.1 key key.pub "file://$PWD/no-site" out >msg
  grep -q 'no index signed' msg || fail "did not say so: $(cat msg)"
  [ -z "$(ls -A out)" ] || fail "wrote $(ls -A out)"
}

test_verify_index_refuses_tampering_another_namespace_and_a_bad_format() {
  throwaway_key key
  hand_index good 3 v1.2.3
  [ "$("$here/verify-index.sh" good key.pub)" = "3 $(jq -r .expires good/release-index.json) v1.2.3" ] ||
    fail "verify-index.sh prints $("$here/verify-index.sh" good key.pub)"
  local case
  for case in tampered release_namespace extra_field pre_release zero_serial version_mismatch; do
    rm -rf bad
    cp -R good bad
    case $case in
      tampered) sed -i 's/"serial": 3/"serial": 4/' bad/release-index.json ;;
      release_namespace) sign_index_as bad key oska-release ;;
      extra_field) jq '.note = "x"' good/release-index.json >bad/release-index.json && sign_index_as bad ;;
      pre_release) jq '.latest = {tag: "v1.3.0-rc.1", version: "1.3.0-rc.1"}' good/release-index.json >bad/release-index.json && sign_index_as bad ;;
      zero_serial) jq '.serial = 0' good/release-index.json >bad/release-index.json && sign_index_as bad ;;
      version_mismatch) jq '.latest.version = "1.2.4"' good/release-index.json >bad/release-index.json && sign_index_as bad ;;
    esac
    if "$here/verify-index.sh" bad key.pub >/dev/null 2>err; then
      fail "$case: verified"
    fi
  done
}

# Whether the installers in ./site are pinned to require the index: yes or
# no, the same in both.
index_pin() {
  local sh ps1
  sh=$(sed -n "s/^INDEX_REQUIRED='\(.*\)'$/\1/p" site/install.sh)
  ps1=$(sed -n "s/^ *\$IndexRequired = '\(.*\)'$/\1/p" site/install.ps1)
  [ "$sh" = "$ps1" ] || fail "install.sh pins '$sh', install.ps1 '$ps1'"
  echo "$sh"
}

# Releases v1.2.3 and v1.3.0, as GitHub Releases hold them: v1.3.0 carries
# the index its release run signed (serial 5) and v1.2.3 an older one.
releases_with_indexes() {
  signed_release v1.2.3
  signed_release v1.3.0
  hand_index out/v1.2.3 4 v1.2.3 2026-01-02T00:00:00Z
  hand_index out/v1.3.0 5 v1.3.0
  mkdir site
}

test_stage_site_publishes_the_highest_serial_index_and_pins_installers_to_require_it() {
  releases_with_indexes
  "$here/stage-site.sh" out site key.pub
  cmp out/v1.3.0/release-index.json site/release-index.json || fail "did not publish serial 5"
  cmp out/v1.3.0/release-index.json.sig site/release-index.json.sig || fail "did not publish its signature"
  # The index is kept apart from the release files: what is served under
  # releases/ still holds only what SHA256SUMS lists.
  for tag in v1.2.3 v1.3.0; do
    [ ! -e "site/releases/$tag/release-index.json" ] || fail "$tag: the index is among the release files"
    [ ! -e "site/releases/$tag/release-index.json.sig" ] || fail "$tag: the index signature is among the release files"
  done
  [ "$(index_pin)" = yes ] || fail "installers are not pinned to require the index"
  # latest.json is unchanged and stays informational.
  [ "$(jq -r .tag site/latest.json)" = v1.3.0 ] || fail "latest.json names $(jq -r .tag site/latest.json)"
  [ "$(jq 'has("serial") or has("expires")' site/latest.json)" = false ] || fail "latest.json carries index fields"
}

test_stage_site_without_an_index_leaves_installers_on_latest_json() {
  signed_release v1.2.3
  mkdir site
  "$here/stage-site.sh" out site key.pub
  [ ! -e site/release-index.json ] || fail "published an index"
  [ "$(index_pin)" = no ] || fail "installers require an index that is not there"
}

test_stage_site_refuses_an_index_that_does_not_verify() {
  local case
  for case in another_key release_namespace tampered; do
    rm -rf out site builds key key.pub attacker attacker.pub
    releases_with_indexes
    case $case in
      another_key) throwaway_key attacker && sign_index_as out/v1.2.3 attacker ;;
      release_namespace) sign_index_as out/v1.2.3 key oska-release ;;
      tampered) sed -i 's/"serial": 4/"serial": 9/' out/v1.2.3/release-index.json ;;
    esac
    if "$here/stage-site.sh" out site key.pub 2>err; then
      fail "$case: staged a site with an index that does not verify"
    fi
    [ ! -e site/release-index.json ] || fail "$case: published an index"
  done
}

test_stage_site_refuses_an_expired_index_but_not_an_expired_older_one() {
  releases_with_indexes
  # v1.2.3's serial-4 index expired long ago; serial 5 supersedes it.
  "$here/stage-site.sh" out site key.pub
  rm -rf site
  mkdir site
  hand_index out/v1.3.0 5 v1.3.0 2026-01-03T00:00:00Z
  if "$here/stage-site.sh" out site key.pub 2>err; then
    fail "staged an expired index"
  fi
  grep -q 'expired' err || fail "error does not say it expired: $(cat err)"
}

test_stage_site_warns_when_the_index_expires_within_30_days() {
  releases_with_indexes
  hand_index out/v1.3.0 5 v1.3.0 "$(date -u -d '+29 days' +%Y-%m-%dT%H:%M:%SZ)"
  "$here/stage-site.sh" out site key.pub >log
  grep -q '^::warning::.*release index.*expires.*under 30 days' log || fail "no warning annotation: $(cat log)"
  rm -rf site
  mkdir site
  hand_index out/v1.3.0 5 v1.3.0 "$(date -u -d '+31 days' +%Y-%m-%dT%H:%M:%SZ)"
  "$here/stage-site.sh" out site key.pub >log
  if grep -q 'warning' log; then fail "warned with 31 days left: $(cat log)"; fi
}

test_stage_site_refuses_an_index_naming_a_release_it_does_not_publish() {
  releases_with_indexes
  hand_index out/v1.3.0 5 v1.4.0
  if "$here/stage-site.sh" out site key.pub 2>err; then
    fail "staged an index naming v1.4.0, which is not published"
  fi
  grep -q 'v1.4.0' err || fail "error does not name the release: $(cat err)"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
