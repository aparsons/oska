#!/usr/bin/env bash
# Tests for the sh installer. Run from anywhere, on Linux or macOS:
#
#   scripts/install/test.sh [<pattern>]
#
# With <pattern>, only the tests whose names match it (grep -E) run.
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
# scripts/release/assemble.sh and sign.sh lay it out: SHA256SUMS starts
# with the signed tag line. Each fake binary is a script that prints
# "oskac <reports>" (default: <version>), like the real `oskac --version`.
release() {
  local version=$1 reports=${2:-$1} dir="site/releases/v$1" asset
  release_key
  mkdir -p "$dir"
  for asset in $ASSETS; do
    printf '#!/bin/sh\necho "oskac %s"\n' "$reports" >"$dir/$asset"
  done
  {
    printf '# oska-release v%s\n' "$version"
    for asset in $ASSETS; do
      printf '%s  %s\n' "$(sha256 "$dir/$asset")" "$asset"
    done
  } >"$dir/SHA256SUMS"
  sign "$dir"
}

# (Re-)signs <dir>/SHA256SUMS with ./key.
sign() {
  rm -f "$1/SHA256SUMS.sig"
  ssh-keygen -q -Y sign -f key -n oska-release "$1/SHA256SUMS"
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
  "signature_verified": true,
  "receipt": "$PWD/home/.oska/receipt.json"
}
JSON
  diff <(jq -S . expected.json) <(jq -S . out.json) || fail "JSON output differs"
  [ "$(wc -l <out.json | tr -d ' ')" = 1 ] || fail "JSON output is not one line"
}

test_writes_a_receipt_listing_what_it_installed() {
  release 1.2.3
  latest 1.2.3
  install --json >out.json
  local receipt="$PWD/home/.oska/receipt.json"
  [ "$(jq -r .receipt out.json)" = "$receipt" ] || fail "JSON receipt is $(jq -r .receipt out.json)"
  jq -e . "$receipt" >/dev/null || fail "the receipt is not JSON"
  [ "$(jq -r .schema "$receipt")" = 1 ] || fail "schema is not 1"
  [ "$(jq -r .version "$receipt")" = 1.2.3 ] || fail "version is not 1.2.3"
  [ "$(jq -r .tag "$receipt")" = v1.2.3 ] || fail "tag is not v1.2.3"
  [ "$(jq -r .platform "$receipt")" = "$(host_platform)" ] || fail "platform is wrong"
  jq -r .installed_at "$receipt" | grep -Eqx '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z' ||
    fail "installed_at is not UTC RFC 3339: $(jq -r .installed_at "$receipt")"
  [ "$(jq -r .site "$receipt")" = "file://$PWD/site" ] || fail "site is $(jq -r .site "$receipt")"
  [ "$(jq -r .key_fingerprint "$receipt")" = "$(ssh-keygen -l -f key.pub | awk '{print $2}')" ] ||
    fail "key_fingerprint is $(jq -r .key_fingerprint "$receipt")"
  [ "$(jq -r .sha256 "$receipt")" = "$(sha256 "$(installed)")" ] || fail "sha256 is not the binary's"
  [ "$(jq -r .installer "$receipt")" = sh ] || fail "installer is not sh"
  [ "$(jq -r .index_serial "$receipt")" = 0 ] || fail "index_serial is not 0"
  # The legacy version file is still written, and listed.
  [ "$(cat home/.oska/version)" = 1.2.3 ] || fail "the version file is not 1.2.3"
  cat >expected.json <<JSON
[
  {"path": "$(installed)", "kind": "binary", "sha256": "$(sha256 "$(installed)")"},
  {"path": "$PWD/home/.oska/version", "kind": "record", "sha256": "$(printf '1.2.3\n' | sha256 /dev/stdin)"},
  {"path": "$receipt", "kind": "record"}
]
JSON
  diff <(jq -S . expected.json) <(jq -S .files "$receipt") || fail "files differ"
  local held
  held=$(cd home/.oska && find . -mindepth 1 -maxdepth 1 | LC_ALL=C sort | tr '\n' ' ')
  [ "$held" = "./bin ./receipt.json ./version " ] || fail "the home holds $held"
}

# An install from before receipts has only the version file. The next run
# takes the previous version from it and writes a receipt.
test_updates_an_install_that_has_only_a_version_file() {
  release 1.2.3
  release 1.3.0
  latest 1.2.3
  install --json >/dev/null
  rm home/.oska/receipt.json
  latest 1.3.0
  install --json >out.json
  [ "$(jq -r .action out.json)" = updated ] || fail "action is $(jq -r .action out.json)"
  [ "$(jq -r .previous_version out.json)" = 1.2.3 ] || fail "previous_version is not 1.2.3"
  [ "$(jq -r .version home/.oska/receipt.json)" = 1.3.0 ] || fail "no receipt for 1.3.0"
}

test_the_receipt_wins_over_the_version_file() {
  release 1.2.3
  release 1.3.0
  latest 1.2.3
  install --json >/dev/null
  printf '9.9.9\n' >home/.oska/version
  latest 1.3.0
  install --json >out.json
  [ "$(jq -r .action out.json)" = updated ] || fail "action is $(jq -r .action out.json)"
  [ "$(jq -r .previous_version out.json)" = 1.2.3 ] || fail "previous_version is not the receipt's 1.2.3"
}

# The receipt is advisory: one that vouches for a tampered binary changes
# nothing about what is verified.
test_a_tampered_receipt_cannot_vouch_for_a_tampered_binary() {
  with_good_install
  local asset evil
  asset=site/releases/v1.3.0/oskac-$(host_platform)
  printf 'evil\n' >>"$asset"
  evil=$(sha256 "$asset")
  jq --arg h "$evil" '.version = "1.3.0" | .tag = "v1.3.0" | .sha256 = $h | .files[0].sha256 = $h' \
    home/.oska/receipt.json >receipt.json
  mv receipt.json home/.oska/receipt.json
  assert_refused_unchanged checksum_mismatch 1 --version 1.3.0
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "the installed oskac changed"
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

# Every path under ./home, with each file's SHA-256: compare two of these to
# show a run changed nothing.
snapshot() {
  local f
  find home | LC_ALL=C sort | while IFS= read -r f; do
    if [ -f "$f" ]; then
      printf '%s %s\n' "$f" "$(sha256 "$f")"
    else
      printf '%s\n' "$f"
    fi
  done
}

# Runs the installer with <args>, which must refuse with <code> and exit
# status <status>, as one line of JSON, and leave ./home exactly as it was.
assert_refused_unchanged() {
  local code=$1 status=$2 got=0
  shift 2
  snapshot >before.txt
  install --json "$@" >out.json || got=$?
  [ "$got" = "$status" ] || fail "exited $got, not $status"
  [ "$(jq -r .ok out.json)" = false ] || fail "JSON does not say ok: false"
  [ "$(jq -r .error out.json)" = "$code" ] || fail "error is $(jq -r .error out.json), not $code"
  [ "$(wc -l <out.json | tr -d ' ')" = 1 ] || fail "JSON output is not one line"
  snapshot >after.txt
  diff before.txt after.txt || fail "the home changed"
}

test_refuses_to_downgrade_to_an_older_latest_release() {
  release 1.2.3
  release 1.3.0
  latest 1.3.0
  install --json >/dev/null
  latest 1.2.3
  assert_refused_unchanged downgrade_refused 1
  jq -r .message out.json | grep -q '1\.3\.0' || fail "message does not name the installed version"
  jq -r .message out.json | grep -q '1\.2\.3' || fail "message does not name the older version"
  [ "$("$(installed)" --version)" = "oskac 1.3.0" ] || fail "the installed oskac changed"
}

test_refuses_a_downgrade_named_with_version_alone() {
  release 1.2.3
  release 1.3.0
  latest 1.3.0
  install --json >/dev/null
  assert_refused_unchanged downgrade_refused 1 --version 1.2.3
  jq -r .message out.json | grep -q -- '--allow-downgrade' || fail "message does not name --allow-downgrade"
}

test_downgrades_with_version_and_allow_downgrade() {
  release 1.2.3
  release 1.3.0
  latest 1.3.0
  install --json >/dev/null
  install --json --allow-downgrade --version 1.2.3 >out.json
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "did not downgrade to 1.2.3"
  [ "$(jq -r .action out.json)" = downgraded ] || fail "action is $(jq -r .action out.json)"
  [ "$(jq -r .previous_version out.json)" = 1.3.0 ] || fail "previous_version is not 1.3.0"
  [ "$(jq -r .version home/.oska/receipt.json)" = 1.2.3 ] || fail "the receipt does not say 1.2.3"
  install --version 1.3.0 >up.txt
  install --version=1.2.3 --allow-downgrade >down.txt
  grep -q "Downgraded oskac 1.3.0 to 1.2.3 in $(installed)" down.txt || fail "no human summary: $(cat down.txt)"
}

test_allow_downgrade_without_version_is_a_usage_error() {
  release 1.2.3
  latest 1.2.3
  install --json >/dev/null
  assert_refused_unchanged usage 2 --allow-downgrade
}

# Install (the default), --check and --uninstall are modes, and one run is
# in one of them. --version and --allow-downgrade belong to install.
test_modes_are_mutually_exclusive() {
  release 1.2.3
  latest 1.2.3
  install --json >/dev/null
  local args
  while read -r args; do
    # shellcheck disable=SC2086 # each line is a list of arguments
    assert_refused_unchanged usage 2 $args
    jq -r .message out.json | grep -q 'cannot be used' || fail "$args: message is $(jq -r .message out.json)"
  done <<'CASES'
--check --uninstall
--uninstall --check
--check --version 1.2.3
--uninstall --version 1.2.3
--check --allow-downgrade
--uninstall --version 1.2.3 --allow-downgrade
CASES
}

# A pre-release installed with --version is newer than the latest release,
# so a plain re-run keeps it rather than move back.
test_a_plain_re_run_keeps_a_newer_pre_release() {
  release 1.3.0
  release 1.4.0-pre.1
  latest 1.3.0
  install --json --version 1.4.0-pre.1 >/dev/null
  assert_refused_unchanged downgrade_refused 1
  jq -r .message out.json | grep -q '1\.4\.0-pre\.1 is installed, which is newer than the latest release, 1\.3\.0' ||
    fail "message is $(jq -r .message out.json)"
  release 1.4.0
  latest 1.4.0
  install --json >out.json
  [ "$(jq -r .action out.json)" = updated ] || fail "1.4.0-pre.1 to 1.4.0 is $(jq -r .action out.json)"
}

# Versions compare by semver precedence: numerically, part by part, with a
# pre-release below its release.
test_compares_versions_by_semver_precedence() {
  local from to action
  while read -r from to action; do
    rm -rf home site/latest.json
    release "$from"
    release "$to"
    install --json --version "$from" >/dev/null
    install --json --allow-downgrade --version "$to" >out.json || fail "$from to $to: $(cat out.json)"
    [ "$(jq -r .action out.json)" = "$action" ] || fail "$from to $to is $(jq -r .action out.json), not $action"
  done <<'CASES'
1.9.0 1.10.0 updated
1.10.0 1.9.0 downgraded
1.2.10 1.2.9 downgraded
2.0.0 10.0.0 updated
1.0.0-alpha 1.0.0 updated
1.0.0 1.0.0-rc.1 downgraded
1.0.0-alpha 1.0.0-alpha.1 updated
1.0.0-alpha.1 1.0.0-alpha.beta updated
1.0.0-beta.11 1.0.0-beta.2 downgraded
1.0.0-beta 1.0.0-beta.2 updated
1.0.0-rc.1 1.0.0-rc.1 reinstalled
CASES
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
  snapshot >before.txt
  install --json --version 1.3.0 >out.json && fail "installed a tampered release"
  [ "$(jq -r .ok out.json)" = false ] || fail "JSON does not say ok: false"
  [ "$(jq -r .error out.json)" = "$code" ] || fail "error is $(jq -r .error out.json), not $code"
  [ "$(wc -l <out.json | tr -d ' ')" = 1 ] || fail "JSON output is not one line"
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "the installed oskac changed"
  snapshot >after.txt
  diff before.txt after.txt || fail "the home changed"
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
  sign "$dir"
  assert_refused checksum_mismatch
}

# The rollback attack: genuine, signed files of an old release served as a
# newer one. The signature is valid; only the signed tag line refuses it.
test_refuses_an_older_signed_release_published_as_the_latest() {
  with_good_install
  rm -r site/releases/v1.3.0
  cp -R site/releases/v1.2.3 site/releases/v1.3.0
  latest 1.3.0
  install --json >out.json && fail "installed 1.2.3's files as 1.3.0"
  [ "$(jq -r .error out.json)" = version_mismatch ] || fail "error is $(jq -r .error out.json)"
  jq -r .message out.json | grep -q 'v1.2.3' || fail "message does not name the signed tag"
}

test_refuses_an_older_signed_release_from_a_mirror_for_a_newer_version() {
  with_good_install
  mkdir -p mirror/releases
  cp -R site/releases/v1.2.3 mirror/releases/v1.3.0
  SITE_URL="file://$PWD/mirror" assert_refused version_mismatch
}

test_refuses_signed_checksums_that_name_no_tag() {
  with_good_install
  local dir=site/releases/v1.3.0
  grep -v '^#' "$dir/SHA256SUMS" >sums
  mv sums "$dir/SHA256SUMS"
  sign "$dir"
  assert_refused version_mismatch
}

test_refuses_a_binary_that_does_not_report_the_signed_version() {
  release 1.2.3
  latest 1.2.3
  install --json >/dev/null
  release 1.3.0 1.2.9
  assert_refused version_mismatch
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

# Someone who can write to the bin directory plants symlinks where a
# predictable staging file (.oskac.<pid>) would go, and one in place of
# oskac itself. The installer must write through none of them.
test_never_writes_through_a_symlink_planted_in_the_bin_directory() {
  release 1.2.3
  latest 1.2.3
  mkdir -p home/.oska/bin
  printf 'victim\n' >victim
  # One process plants them all, so the PIDs the installer will get are
  # still ahead.
  perl -e 'my $n = $ARGV[0]; symlink("$ENV{PWD}/victim", "home/.oska/bin/.oskac.$_") or die for $n .. $n + 300' \
    "$(sh -c 'echo $$')"
  ln -s "$PWD/victim" home/.oska/bin/oskac
  install --json >out.json
  [ "$(cat victim)" = victim ] || fail "wrote through a planted symlink"
  [ ! -L home/.oska/bin/oskac ] || fail "oskac is still the planted symlink"
  [ "$("$(installed)" --version)" = "oskac 1.2.3" ] || fail "did not install 1.2.3"
  [ -z "$(find home/.oska/bin -name '.oskac.*' -type d)" ] || fail "left a staging directory"
}

# Interrupted while it checks the staged binary (a slow fake that signals
# when it starts), the installer removes its temp and staging directories,
# leaves the old install as it was, and dies of the same signal.
test_an_interrupted_install_leaves_nothing_behind() {
  with_good_install
  local dir=site/releases/v1.3.0 asset sig pid status waited
  asset=oskac-$(host_platform)
  printf '#!/bin/sh\n: >"%s/started"\nsleep 2\necho "oskac 1.3.0"\n' "$PWD" >"$dir/$asset"
  awk -v f="$asset" -v h="$(sha256 "$dir/$asset")" '$2 == f {print h "  " f; next} {print}' \
    "$dir/SHA256SUMS" >sums
  mv sums "$dir/SHA256SUMS"
  sign "$dir"
  pinned
  for sig in INT TERM HUP; do
    rm -rf started tmp
    mkdir tmp
    snapshot >before.txt
    # perl restores the default action for SIGINT, which a background job
    # of a non-interactive shell would otherwise ignore.
    HOME="$PWD/home" OSKA_SITE_URL="file://$PWD/site" TMPDIR="$PWD/tmp" \
      perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die' sh -s -- --json --version 1.3.0 \
      <pinned/install.sh >out.json &
    pid=$!
    # Wait until the installer is in the binary check: the fake writes
    # `started` there, after the temp and staging directories exist. Give
    # up if the installer ends first or never gets there.
    waited=0
    while [ ! -e started ]; do
      kill -0 "$pid" 2>/dev/null || fail "$sig: the installer ended before it checked the binary: $(cat out.json)"
      waited=$((waited + 1))
      [ "$waited" -le 300 ] || fail "$sig: the installer never checked the binary"
      sleep 0.1
    done
    [ -n "$(find home/.oska/bin -name '.oskac.*')" ] || fail "$sig: no staging directory to clean up"
    [ -n "$(ls -A tmp)" ] || fail "$sig: no temp directory in $PWD/tmp to clean up"
    kill -s "$sig" "$pid"
    status=0
    wait "$pid" || status=$?
    [ "$status" = $((128 + $(kill -l "$sig"))) ] || fail "$sig: exited $status, not by the signal"
    [ -z "$(ls -A tmp)" ] || fail "$sig: left $(ls -A tmp) in the temp directory"
    snapshot >after.txt
    diff before.txt after.txt || fail "$sig: the home changed"
  done
}

# Makes the installer see Linux x86_64 with the C library <libc>: "glibc
# X.Y", or "musl" (as on Alpine, where `getconf GNU_LIBC_VERSION` fails and
# `ldd --version` names musl). Fakes uname, getconf and ldd in ./fakebin,
# so it works on a macOS test host too.
# shellcheck disable=SC2016 # the fakes expand $1 when they run
fake_linux() {
  mkdir -p fakebin
  printf '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) echo Linux ;; esac\n' >fakebin/uname
  case $1 in
    glibc*)
      printf '#!/bin/sh\n[ "$1" = GNU_LIBC_VERSION ] && echo "%s"\n' "$1" >fakebin/getconf
      printf '#!/bin/sh\necho "ldd (GNU libc) %s"\n' "${1#glibc }" >fakebin/ldd
      ;;
    musl)
      printf '#!/bin/sh\necho "getconf: GNU_LIBC_VERSION: unknown variable" >&2\nexit 1\n' >fakebin/getconf
      printf '#!/bin/sh\necho "musl libc (x86_64)" >&2\necho "Version 1.2.5" >&2\nexit 1\n' >fakebin/ldd
      ;;
  esac
  chmod +x fakebin/*
}

assert_refused_libc() {
  PATH="$PWD/fakebin:$PATH" install --json >out.json && fail "installed on $1"
  [ "$(jq -r .error out.json)" = unsupported_libc ] || fail "error is $(jq -r .error out.json), not unsupported_libc"
  [ ! -e home/.oska ] || fail "created ~/.oska"
}

test_refuses_a_linux_without_glibc() {
  release 1.2.3
  latest 1.2.3
  fake_linux musl
  assert_refused_libc musl
  jq -r .message out.json | grep -q musl || fail "message does not mention musl"
}

test_refuses_a_glibc_older_than_the_one_releases_are_built_against() {
  release 1.2.3
  latest 1.2.3
  fake_linux 'glibc 2.31'
  assert_refused_libc 'glibc 2.31'
  jq -r .message out.json | grep -q '2\.35' || fail "message does not name glibc 2.35"
}

test_installs_on_glibc_2_35_and_newer() {
  release 1.2.3
  latest 1.2.3
  local libc
  for libc in 'glibc 2.35' 'glibc 2.41' 'glibc 3.0'; do
    fake_linux "$libc"
    PATH="$PWD/fakebin:$PATH" install --json >out.json || fail "refused $libc: $(cat out.json)"
    [ "$(jq -r .platform out.json)" = linux-x86_64 ] || fail "platform is not linux-x86_64"
  done
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

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
