#!/usr/bin/env bash
# Tests for the sh installer. Run from anywhere, on Linux or macOS:
#
#   scripts/install/test.sh
#
# Each test builds a local release fixture in its own scratch directory: a
# site tree laid out like https://oskalang.dev (releases/vX.Y.Z/ with fake
# binaries, SHA256SUMS signed by a throwaway key, latest.json), an installer
# pinned to that key by pin.sh, and a fake home. The installer is piped to
# `sh`, as `curl ... | sh` does, with OSKA_SITE_URL pointing at the fixture.
#
# Portable on purpose (bash 3.2, BSD tools), so CI can run it on macOS too.

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

ASSETS="oskac-linux-aarch64 oskac-linux-x86_64 oskac-macos-aarch64 oskac-macos-x86_64 oskac-windows-x86_64.exe"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# The platform the installer should pick on this machine.
host_platform() {
  local os arch
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) fail "unsupported test host $(uname -s)" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "unsupported test host $(uname -m)" ;;
  esac
  if [ "$os" = macos ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
    arch=aarch64
  fi
  echo "$os-$arch"
}

# Writes the throwaway release key ./key and ./key.pub, once.
release_key() {
  [ -f key ] || ssh-keygen -q -t ed25519 -N '' -C oska-release-test -f key
}

# Publishes release <version> in ./site/releases/v<version>/, as
# scripts/release/assemble.sh and sign.sh lay it out. Each fake binary is a
# script that prints "oskac <version>", like the real `oskac --version`.
release() {
  local version=$1 dir="site/releases/v$1" asset
  release_key
  mkdir -p "$dir"
  for asset in $ASSETS; do
    printf '#!/bin/sh\necho "oskac %s"\n' "$version" >"$dir/$asset"
  done
  for asset in $ASSETS; do
    printf '%s  %s\n' "$(sha256 "$dir/$asset")" "$asset"
  done >"$dir/SHA256SUMS"
  ssh-keygen -q -Y sign -f key -n oska-release "$dir/SHA256SUMS"
}

# Points ./site/latest.json at release <version>, in the shape
# scripts/release/stage-site.sh writes.
latest() {
  local url="https://oskalang.dev/releases/v$1"
  cat >site/latest.json <<JSON
{
  "docs": "main",
  "version": "$1",
  "tag": "v$1",
  "release_url": "$url/",
  "checksums_url": "$url/SHA256SUMS",
  "signature_url": "$url/SHA256SUMS.sig",
  "signing_key_url": "https://oskalang.dev/oska-release.pub",
  "signing_namespace": "oska-release",
  "installers": {
    "sh": "https://oskalang.dev/install.sh",
    "ps1": "https://oskalang.dev/install.ps1"
  },
  "binaries": {
    "linux-x86_64": "$url/oskac-linux-x86_64"
  }
}
JSON
}

# Pins the installers to ./key.pub in ./pinned/.
pinned() {
  release_key
  [ -d pinned ] || "$here/pin.sh" key.pub pinned
}

# Runs the pinned installer as `curl ... | sh -s -- <args>` would, against
# the fixture site, with ./home as the home directory.
install() {
  pinned
  mkdir -p home
  HOME="$PWD/home" OSKA_SITE_URL="${SITE_URL:-file://$PWD/site}" \
    sh -s -- "$@" <pinned/install.sh
}

installed() {
  echo "$PWD/home/.oska/bin/oskac"
}

test_installs_the_latest_release_and_reports_it_as_json() {
  release 1.2.3
  latest 1.2.3
  install --json >out.json
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "installed binary is not 1.2.3"
  cat >expected.json <<JSON
{
  "ok": true,
  "action": "installed",
  "version": "1.2.3",
  "tag": "v1.2.3",
  "previous_version": null,
  "platform": "$(host_platform)",
  "path": "$(installed)",
  "bin_dir": "$PWD/home/.oska/bin",
  "on_path": false,
  "checksum_verified": true,
  "signature_verified": true
}
JSON
  diff <(jq -S . expected.json) <(jq -S . out.json) || fail "JSON output differs"
  [ "$(wc -l <out.json | tr -d ' ')" = 1 ] || fail "JSON output is not one line"
}

test_re_running_updates_in_place() {
  release 1.2.3
  latest 1.2.3
  install --json >first.json
  release 1.3.0
  latest 1.3.0
  install --json >second.json
  [ "$("$(installed)" --version)" = "oskac 1.3.0" ] || fail "did not update to 1.3.0"
  [ "$(jq -r .action second.json)" = updated ] || fail "action is not updated"
  [ "$(jq -r .previous_version second.json)" = 1.2.3 ] || fail "previous_version is not 1.2.3"
  [ "$(jq -r .version second.json)" = 1.3.0 ] || fail "version is not 1.3.0"
  install --json >third.json
  [ "$(jq -r .action third.json)" = reinstalled ] || fail "same version is not reinstalled"
  [ "$(ls -A home/.oska/bin)" = oskac ] || fail "left files behind in bin: $(ls -A home/.oska/bin)"
}

test_version_installs_that_release_rather_than_the_latest() {
  release 1.2.3
  release 1.3.0
  latest 1.3.0
  install --version 1.2.3 >out.txt
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "--version 1.2.3 not installed"
  grep -q "Installed oskac 1.2.3 to $(installed)" out.txt || fail "no human summary"
  install --version=v1.3.0 --json >out.json
  [ "$("$(installed)" --version)" = "oskac 1.3.0" ] || fail "--version=v1.3.0 not installed"
}

# Installs 1.2.3 cleanly, then publishes 1.3.0 and lets the test tamper
# with it. The tampered 1.3.0 must be refused with <code> and leave 1.2.3
# installed.
assert_refused() {
  local code=$1
  install --json --version 1.3.0 >out.json && fail "installed a tampered release"
  [ "$(jq -r .ok out.json)" = false ] || fail "JSON does not say ok: false"
  [ "$(jq -r .error out.json)" = "$code" ] || fail "error is $(jq -r .error out.json), not $code"
  [ "$(wc -l <out.json | tr -d ' ')" = 1 ] || fail "JSON output is not one line"
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "the installed oskac changed"
  [ "$(cat home/.oska/version)" = 1.2.3 ] || fail "the receipt changed"
  [ "$(ls -A home/.oska/bin)" = oskac ] || fail "left files behind in bin"
}

with_good_install() {
  release 1.2.3
  latest 1.2.3
  install --json >/dev/null
  release 1.3.0
}

test_refuses_a_tampered_binary() {
  with_good_install
  printf 'evil\n' >>"site/releases/v1.3.0/oskac-$(host_platform)"
  assert_refused checksum_mismatch
}

test_refuses_checksums_rewritten_to_match_a_tampered_binary() {
  with_good_install
  local dir=site/releases/v1.3.0 asset
  asset=oskac-$(host_platform)
  printf '#!/bin/sh\necho evil\n' >"$dir/$asset"
  awk -v f="$asset" -v h="$(sha256 "$dir/$asset")" '$2 == f {$1 = h; print h "  " f; next} {print}' \
    "$dir/SHA256SUMS" >sums
  mv sums "$dir/SHA256SUMS"
  assert_refused bad_signature
}

test_refuses_a_signature_by_another_key() {
  with_good_install
  ssh-keygen -q -t ed25519 -N '' -C attacker -f attacker
  rm site/releases/v1.3.0/SHA256SUMS.sig
  ssh-keygen -q -Y sign -f attacker -n oska-release site/releases/v1.3.0/SHA256SUMS
  assert_refused bad_signature
}

test_refuses_a_signature_in_another_namespace() {
  with_good_install
  rm site/releases/v1.3.0/SHA256SUMS.sig
  ssh-keygen -q -Y sign -f key -n file site/releases/v1.3.0/SHA256SUMS
  assert_refused bad_signature
}

test_refuses_a_release_without_a_signature() {
  with_good_install
  rm site/releases/v1.3.0/SHA256SUMS.sig
  assert_refused download_failed
}

test_refuses_a_binary_the_checksums_do_not_list() {
  with_good_install
  local dir=site/releases/v1.3.0
  grep -v "oskac-$(host_platform)\$" "$dir/SHA256SUMS" >sums
  mv sums "$dir/SHA256SUMS"
  rm "$dir/SHA256SUMS.sig"
  ssh-keygen -q -Y sign -f key -n oska-release "$dir/SHA256SUMS"
  assert_refused checksum_mismatch
}

test_refuses_a_version_that_was_never_released() {
  with_good_install
  rm -r site/releases/v1.3.0
  assert_refused download_failed
}

test_fails_cleanly_while_the_site_has_no_release() {
  mkdir site
  # What the site generator writes before stage-site.sh adds a release.
  printf '{\n  "version": "0.1.0",\n  "docs": "main"\n}\n' >site/latest.json
  install --json >out.json && fail "installed with no release"
  [ "$(jq -r .error out.json)" = download_failed ] || fail "error is not download_failed"
  [ ! -e home/.oska ] || fail "created ~/.oska"
}

test_refuses_to_download_over_plain_http() {
  with_good_install
  SITE_URL=http://oskalang.dev assert_refused insecure_url
}

test_an_installer_that_was_never_pinned_installs_nothing() {
  release 1.2.3
  latest 1.2.3
  mkdir home
  if HOME="$PWD/home" OSKA_SITE_URL="file://$PWD/site" sh -s -- --json \
    <"$here/install.sh" >out.json; then
    fail "the unpinned installer installed"
  fi
  [ "$(jq -r .error out.json)" = no_signing_key ] || fail "error is not no_signing_key"
  [ ! -e home/.oska ] || fail "created ~/.oska"
}

test_rejects_bad_arguments_with_exit_status_2() {
  local status=0
  install --json --frobnicate >out.json || status=$?
  [ "$status" = 2 ] || fail "unknown option exited $status"
  [ "$(jq -r .error out.json)" = usage ] || fail "error is not usage"
  status=0
  install --version '1.2.3; rm -rf /' 2>err.txt || status=$?
  [ "$status" = 2 ] || fail "bad version exited $status"
  grep -q 'is not a version' err.txt || fail "no message for the bad version"
}

test_human_output_explains_how_to_put_oskac_on_the_path() {
  release 1.2.3
  latest 1.2.3
  install >out.txt
  grep -qF "export PATH=\"$PWD/home/.oska/bin:\$PATH\"" out.txt || fail "no PATH hint"
  PATH="$PWD/home/.oska/bin:$PATH" install >again.txt
  if grep -q 'export PATH' again.txt; then
    fail "PATH hint shown although the directory is on PATH"
  fi
  PATH="$PWD/home/.oska/bin:$PATH" install --json >out.json
  [ "$(jq -r .on_path out.json)" = true ] || fail "on_path is not true"
}

test_oska_home_moves_the_install() {
  release 1.2.3
  latest 1.2.3
  OSKA_HOME="$PWD/elsewhere" install --json >out.json
  [ "$(jq -r .path out.json)" = "$PWD/elsewhere/bin/oskac" ] || fail "path is not under OSKA_HOME"
  [ "$("$PWD/elsewhere/bin/oskac" --version)" = "oskac 1.2.3" ] || fail "not installed under OSKA_HOME"
}

test_pin_refuses_a_key_that_is_not_one_ed25519_public_key() {
  ssh-keygen -q -t ed25519 -N '' -f k
  if "$here/pin.sh" k out 2>err; then fail "pinned a private key"; fi
  cat k.pub k.pub >two.pub
  if "$here/pin.sh" two.pub out 2>err; then fail "pinned two keys"; fi
  printf "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA'; rm -rf ~; '\n" >evil.pub
  if "$here/pin.sh" evil.pub out 2>err; then fail "pinned a key with quotes in it"; fi
  [ ! -e out ] || fail "wrote installers"
  "$here/pin.sh" k.pub out
  grep -qF "'$(awk '{print $1, $2}' k.pub)'" out/install.sh || fail "key not pinned in install.sh"
  grep -qF "'$(awk '{print $1, $2}' k.pub)'" out/install.ps1 || fail "key not pinned in install.ps1"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_'); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
