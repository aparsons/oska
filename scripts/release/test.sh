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

# assemble.sh builds the container image, whose bytes hold this time
# (2026-11-20T00:00:00Z); the release takes it from the tag's commit.
export SOURCE_DATE_EPOCH=1795132800

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

# The files a release ships beside SHA256SUMS, as assemble.sh lists them:
# for Linux a glibc build (-gnu) and a musl build (-musl) per processor.
RELEASE_ASSETS=(
  oskac-linux-x86_64-gnu
  oskac-linux-x86_64-musl
  oskac-linux-aarch64-gnu
  oskac-linux-aarch64-musl
  oskac-macos-x86_64
  oskac-macos-aarch64
  oskac-windows-x86_64.exe
)

# Writes one fake binary per release target into ./builds.
fake_builds() {
  mkdir -p builds
  for asset in "${RELEASE_ASSETS[@]}"; do
    printf 'binary for %s\n' "$asset" >"builds/$asset"
  done
}

test_assemble_writes_every_binary_and_their_checksums() {
  fake_builds
  "$here/assemble.sh" v1.2.3 builds out
  (cd out/v1.2.3 && LC_ALL=C ls) >listing
  cat >expected <<'LIST'
SHA256SUMS
oskac-image-digest
oskac-linux-aarch64-gnu
oskac-linux-aarch64-musl
oskac-linux-x86_64-gnu
oskac-linux-x86_64-musl
oskac-macos-aarch64
oskac-macos-x86_64
oskac-windows-x86_64.exe
LIST
  diff expected listing || fail "unexpected files"
  # The SHA-256 of "binary for oskac-linux-x86_64-musl\n".
  grep -qx '26fe901d847eb45e0bff67045b6bae18e50b428918b48e48158900f29a2bd4fc  oskac-linux-x86_64-musl' \
    out/v1.2.3/SHA256SUMS || fail "SHA256SUMS lacks the linux-x86_64-musl line"
  [ "$(grep -c '^[0-9a-f]\{64\}  oskac-' out/v1.2.3/SHA256SUMS)" = 8 ] ||
    fail "SHA256SUMS does not list exactly the 7 binaries and the image digest"
  (cd out/v1.2.3 && sha256sum --check --strict SHA256SUMS) || fail "sums do not check"
  [ -x out/v1.2.3/oskac-linux-x86_64-musl ] || fail "binary not executable"
}

# The image digest is signed with the binaries, so the release signature
# covers the image: it is the digest image.sh prints for the release's own
# two musl builds.
test_assemble_signs_the_digest_of_the_image_built_from_its_musl_builds() {
  fake_builds
  "$here/assemble.sh" v1.2.3 builds out
  local digest
  digest=$("$here/image.sh" builds/oskac-linux-x86_64-musl builds/oskac-linux-aarch64-musl layout)
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || fail "image.sh printed '$digest'"
  [ "$(cat out/v1.2.3/oskac-image-digest)" = "$digest" ] ||
    fail "the digest file holds '$(cat out/v1.2.3/oskac-image-digest)', not $digest"
  [ "$(wc -l <out/v1.2.3/oskac-image-digest)" = 1 ] || fail "the digest file is not one line"
  grep -qE '^[0-9a-f]{64}  oskac-image-digest$' out/v1.2.3/SHA256SUMS ||
    fail "SHA256SUMS does not list the digest file"
  # Other musl builds give another image, and so another signed digest.
  printf 'another musl build\n' >builds/oskac-linux-aarch64-musl
  "$here/assemble.sh" v1.2.4 builds out
  [ "$(cat out/v1.2.4/oskac-image-digest)" != "$digest" ] || fail "the digest ignores the aarch64 musl build"
}

# assemble.sh signs what image.sh prints, so anything but one digest (a tag,
# an empty line, two digests, a digest with text after it) is refused, and
# no release is written.
test_assemble_refuses_an_image_builder_that_prints_no_digest() {
  fake_builds
  mkdir scripts
  cp "$here/assemble.sh" scripts/
  local printed hex=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  for printed in latest '' "sha256:$hex"$'\n'"sha256:$hex" "sha256:$hex x" "sha256:${hex}0" "sha512:$hex"; do
    printf '#!/usr/bin/env bash\nprintf "%%s\\n" %q\n' "$printed" >scripts/image.sh
    chmod +x scripts/image.sh
    if scripts/assemble.sh v1.2.3 builds out 2>err; then
      fail "assembled with image.sh printing '$printed'"
    fi
    grep -q 'not a digest' err || fail "error does not say why: $(cat err)"
    [ ! -e out/v1.2.3 ] || fail "left a release directory behind"
  done
  # The fake itself works: with a digest it assembles.
  printf '#!/usr/bin/env bash\necho sha256:%s\n' "$hex" >scripts/image.sh
  scripts/assemble.sh v1.2.3 builds out
  [ "$(cat out/v1.2.3/oskac-image-digest)" = "sha256:$hex" ] || fail "the fake image.sh is not used"
}

test_assemble_refuses_to_build_the_image_without_a_source_date() {
  fake_builds
  if SOURCE_DATE_EPOCH='' "$here/assemble.sh" v1.2.3 builds out 2>err; then
    fail "assembled with no SOURCE_DATE_EPOCH"
  fi
  [ ! -e out/v1.2.3 ] || fail "left a release directory behind"
}

test_assemble_lists_the_assets_it_ships() {
  "$here/assemble.sh" --list >listing
  printf '%s\n' "${RELEASE_ASSETS[@]}" >expected
  diff expected listing || fail "assemble.sh --list differs"
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

# The SHA256 fingerprint of the OpenSSH public key in file $1, as
# `ssh-keygen -l -E sha256` prints it, worked out without ssh-keygen: the
# unpadded base64 of the SHA-256 of the key blob.
fingerprint_of() {
  local hex
  hex=$(awk '{print $2}' "$1" | base64 -d | sha256sum | cut -c1-64)
  # shellcheck disable=SC2059 # the format is the hex digest as \xHH escapes
  printf "SHA256:%s\n" "$(printf "$(printf '%s' "$hex" | sed 's/../\\x&/g')" | base64 | tr -d '=\n')"
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

# Re-signs ./out/<tag>/SHA256SUMS with ./key after the files changed: what
# a release looks like when the signing run itself was handed bad files.
resign_release() {
  local dir=out/$1
  {
    head -n 1 "$dir/SHA256SUMS"
    (cd "$dir" && find . -mindepth 1 -maxdepth 1 ! -name 'SHA256SUMS*' -printf '%P\n' |
      LC_ALL=C sort | xargs sha256sum --)
  } >"$dir/SHA256SUMS.new"
  mv "$dir/SHA256SUMS.new" "$dir/SHA256SUMS"
  rm -f "$dir/SHA256SUMS.sig"
  ssh-keygen -q -Y sign -f key -n oska-release "$dir/SHA256SUMS"
}

# The signed digest is what a user pulls by and what the push checks, so a
# digest file that is not exactly one digest is refused even when signed:
# a tag, two digests, another algorithm, or a short digest.
test_verify_refuses_a_signed_image_digest_that_is_not_one_sha256_digest() {
  signed_release v1.2.3
  resign_release v1.2.3
  "$here/verify.sh" out/v1.2.3 key.pub || fail "the re-signed release does not verify"
  local good bad
  good=$(cat out/v1.2.3/oskac-image-digest)
  for bad in latest "sha256:${good#sha256:}x" "sha512:${good#sha256:}" "${good%?}" \
    "$good"$'\n'"$good" "$good " "ghcr.io/aparsons/oskac@$good" ''; do
    printf '%s\n' "$bad" >out/v1.2.3/oskac-image-digest
    resign_release v1.2.3
    if "$here/verify.sh" out/v1.2.3 key.pub 2>err; then
      fail "verified the image digest '$bad'"
    fi
    grep -q oskac-image-digest err || fail "error does not name the digest file: $(cat err)"
  done
  # With no trailing newline it is still not the one line assemble.sh writes.
  printf '%s' "$good" >out/v1.2.3/oskac-image-digest
  resign_release v1.2.3
  if "$here/verify.sh" out/v1.2.3 key.pub 2>/dev/null; then
    fail "verified a digest file with no newline"
  fi
}

# A symlink in place of the digest file would let what it points at, outside
# what was signed, change after verifying: refused even when it resolves to
# the signed bytes.
test_verify_refuses_an_image_digest_that_is_a_symlink() {
  signed_release v1.2.3
  mv out/v1.2.3/oskac-image-digest real-digest
  ln -s "$PWD/real-digest" out/v1.2.3/oskac-image-digest
  (cd out/v1.2.3 && sha256sum --check --strict --quiet SHA256SUMS) || fail "the link does not match SHA256SUMS"
  if "$here/verify.sh" out/v1.2.3 key.pub 2>err; then
    fail "verified a digest file that is a symlink"
  fi
  grep -q 'not a regular file' err || fail "error does not say why: $(cat err)"
}

# Releases before the image (v0.5.0 and older) have no digest file and
# still verify, so the Pages build keeps publishing them.
test_verify_accepts_an_older_release_without_an_image_digest() {
  signed_release v1.2.3
  rm out/v1.2.3/oskac-image-digest
  resign_release v1.2.3
  "$here/verify.sh" out/v1.2.3 key.pub
}

test_verify_refuses_a_tampered_binary() {
  signed_release
  printf 'evil\n' >>out/v1.2.3/oskac-linux-aarch64-musl
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
    "linux-aarch64-gnu": "https://example.test/releases/v1.10.0/oskac-linux-aarch64-gnu",
    "linux-aarch64-musl": "https://example.test/releases/v1.10.0/oskac-linux-aarch64-musl",
    "linux-x86_64-gnu": "https://example.test/releases/v1.10.0/oskac-linux-x86_64-gnu",
    "linux-x86_64-musl": "https://example.test/releases/v1.10.0/oskac-linux-x86_64-musl",
    "macos-aarch64": "https://example.test/releases/v1.10.0/oskac-macos-aarch64",
    "macos-x86_64": "https://example.test/releases/v1.10.0/oskac-macos-x86_64",
    "windows-x86_64": "https://example.test/releases/v1.10.0/oskac-windows-x86_64.exe"
  }
}
JSON
  # The key's fingerprint, which the install guide and SECURITY.md print, is
  # in latest.json too, computed from the key.
  jq --arg fingerprint "$(fingerprint_of key.pub)" '.signing_key_fingerprint = $fingerprint' \
    expected.json >expected-with-key.json
  diff <(jq -S . expected-with-key.json) <(jq -S . site/latest.json) || fail "latest.json differs"
  # release_url (and the release notes) link to a page, not a bare folder.
  page=site/releases/v1.10.0/index.html
  [ -f "$page" ] || fail "no page at the release's URL"
  grep -qF '<title>oskac 1.10.0</title>' "$page" || fail "page names the wrong release"
  grep -qF 'href="oskac-linux-x86_64-musl"' "$page" || fail "page does not link a binary"
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
  [ "$(jq -r '.binaries["linux-x86_64-gnu"]' site/latest.json)" = \
    https://oskalang.dev/releases/v1.2.3/oskac-linux-x86_64-gnu ] || fail "wrong default site URL"
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
    aarch64 | arm64) asset=oskac-linux-aarch64-gnu ;;
    *) asset=oskac-linux-x86_64-gnu ;;
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

# ---- The release page links its notes on the changelog (#514) ---------------

test_stage_site_release_page_links_its_version_on_the_changelog() {
  signed_release v1.10.0
  mkdir site
  OSKA_SITE_URL=https://example.test "$here/stage-site.sh" out site key.pub
  grep -qF '<a href="https://example.test/changelog.html#v1100">What changed in 1.10.0</a>' \
    site/releases/v1.10.0/index.html || fail "the release page does not link #v1100"
}

test_stage_site_pre_release_page_links_its_base_version_on_the_changelog() {
  signed_release v1.10.0-rc.1
  mkdir site
  "$here/stage-site.sh" out site key.pub
  page=site/releases/v1.10.0-rc.1/index.html
  grep -qF '<a href="https://oskalang.dev/changelog.html#v1100">What changed in 1.10.0</a>' "$page" ||
    fail "the pre-release page does not link its base version, #v1100"
  if grep -q 'rc' <(grep -F changelog.html "$page"); then
    fail "the changelog link names the pre-release"
  fi
}

# ---- End of the changelog link tests (#514) ----------------------------------

# Writes working fake binaries into ./builds: scripts that answer
# --version, in text and JSON, with <version>, as the real oskac does.
working_builds() {
  local version=$1
  mkdir -p builds
  for asset in "${RELEASE_ASSETS[@]}"; do
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

# The largest serial is 999999999999999 (15 digits), the same in
# verify-index.sh, sign-index.sh and both installers.
test_the_largest_serial_is_999999999999999_everywhere() {
  throwaway_key key
  hand_index top 999999999999999 v1.2.3
  "$here/verify-index.sh" top key.pub >/dev/null || fail "verify-index.sh refuses serial 999999999999999"
  hand_index over 1000000000000000 v1.2.3
  if "$here/verify-index.sh" over key.pub >/dev/null 2>&1; then
    fail "verify-index.sh accepts serial 1000000000000000"
  fi
  hand_index site 999999999999998 v1.2.3
  mkdir out
  "$here/sign-index.sh" key key.pub "file://$PWD/site" out >/dev/null
  [ "$(jq -r .serial out/release-index.json)" = 999999999999999 ] || fail "did not sign serial 999999999999999"
  rm -f out/*
  hand_index site 999999999999999 v1.2.3
  if "$here/sign-index.sh" key key.pub "file://$PWD/site" out >/dev/null 2>err; then
    fail "signed a serial above 999999999999999"
  fi
  grep -q 'largest serial' err || fail "error does not say why: $(cat err)"
  [ -z "$(ls -A out)" ] || fail "wrote $(ls -A out)"
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

# An expired index does not stop the Pages build, so the rest of the site
# still deploys. It is published, loudly, and the installers stay pinned to
# require it: they refuse it (stale_index), so only "install the latest"
# fails, and a named release still installs.
test_stage_site_publishes_an_expired_index_with_a_warning() {
  working_builds 1.2.3
  throwaway_key key
  "$here/assemble.sh" v1.2.3 builds out
  "$here/sign.sh" out/v1.2.3 key key.pub
  hand_index out/v1.2.3 5 v1.2.3 2026-01-03T00:00:00Z
  mkdir site
  OSKA_SITE_URL="file://$PWD/site" "$here/stage-site.sh" out site key.pub >log ||
    fail "the build stopped on an expired index: $(cat log)"
  grep -q '^::warning::.*release index.*expired' log || fail "no warning annotation: $(cat log)"
  cmp out/v1.2.3/release-index.json site/release-index.json || fail "did not publish the index"
  [ "$(index_pin)" = yes ] || fail "installers are no longer pinned to require the index"
  local status=0
  OSKA_HOME="$PWD/oska" OSKA_SITE_URL="file://$PWD/site" sh -s -- --json <site/install.sh >latest.json || status=$?
  [ "$status" = 1 ] && [ "$(jq -r .error latest.json)" = stale_index ] ||
    fail "installing the latest did not fail with stale_index: $(cat latest.json)"
  OSKA_HOME="$PWD/oska" OSKA_SITE_URL="file://$PWD/site" sh -s -- --json --version 1.2.3 <site/install.sh >named.json ||
    fail "a named release did not install: $(cat named.json)"
}

# Still refused: an index that is tampered with, unsigned, or not the
# highest serial (an older one is only ever superseded, never published).
test_stage_site_publishes_the_highest_serial_even_when_older_ones_expired() {
  releases_with_indexes
  # v1.2.3's serial-4 index expired long ago; serial 5 supersedes it.
  "$here/stage-site.sh" out site key.pub >log
  cmp out/v1.3.0/release-index.json site/release-index.json || fail "did not publish serial 5"
  if grep -q 'expired' log; then fail "warned about a superseded index: $(cat log)"; fi
  rm -rf site
  mkdir site
  rm out/v1.2.3/release-index.json.sig
  if "$here/stage-site.sh" out site key.pub 2>err; then
    fail "staged a site with an unsigned index"
  fi
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

test_fingerprint_of_agrees_with_ssh_keygen() {
  throwaway_key key
  [ "$(fingerprint_of key.pub)" = "$(ssh-keygen -l -E sha256 -f key.pub | awk '{print $2}')" ] ||
    fail "fingerprint_of: $(fingerprint_of key.pub)"
}

# ---- The monthly re-sign (resign-index.yml) ----------------------------------

# What resign-index.yml does once a month: sign-index.sh with no --tag over
# the live index, the result attached to the release it names, then the Pages
# build. An index that has already expired is re-signed as well, and the
# installers install the latest release again.
test_resigning_an_expired_index_lets_the_latest_install_again() {
  working_builds 1.2.3
  throwaway_key key
  "$here/assemble.sh" v1.2.3 builds out
  "$here/sign.sh" out/v1.2.3 key key.pub
  hand_index out/v1.2.3 5 v1.2.3 2026-01-03T00:00:00Z
  mkdir site
  OSKA_SITE_URL="file://$PWD/site" "$here/stage-site.sh" out site key.pub >/dev/null
  mkdir resigned
  "$here/sign-index.sh" key key.pub "file://$PWD/site" resigned >/dev/null
  [ "$(jq -r .serial resigned/release-index.json)" = 6 ] || fail "serial is not 6"
  local tag
  tag=$(jq -r .latest.tag resigned/release-index.json)
  [ "$tag" = v1.2.3 ] || fail "latest moved to $tag"
  cp resigned/release-index.json resigned/release-index.json.sig "out/$tag/"
  rm -rf site
  mkdir site
  OSKA_SITE_URL="file://$PWD/site" "$here/stage-site.sh" out site key.pub >log
  if grep -q 'expire' log; then fail "warned about the fresh index: $(cat log)"; fi
  OSKA_HOME="$PWD/oska" OSKA_SITE_URL="file://$PWD/site" sh -s -- --json <site/install.sh >latest.json ||
    fail "installing the latest failed: $(cat latest.json)"
  [ "$(jq -r .index_verified latest.json)" = true ] || fail "the install did not verify the index: $(cat latest.json)"
}

# Prints each job of workflow $1 as "<job> <environment or -> <reads the key:
# yes or no>", reading indentation as scripts/workflow-guard/guard.sh does.
workflow_jobs() {
  awk '
    function finish() {
      if (job != "") print job, (env == "" ? "-" : env), (key ? "yes" : "no")
      job = ""
    }
    /^[[:space:]]*(#.*)?$/ { next }
    {
      ind = $0; sub(/[^ ].*$/, "", ind); ind = length(ind)
    }
    ind == 0 { finish(); in_jobs = ($0 ~ /^jobs:/); next }
    !in_jobs { next }
    job_indent == "" { job_indent = ind }
    ind == job_indent {
      finish()
      job = $0; sub(/^[[:space:]]*/, "", job); sub(/:.*$/, "", job)
      env = ""; key = 0; key_indent = ""
      next
    }
    job != "" {
      if (key_indent == "") key_indent = ind
      if (ind == key_indent && $0 ~ /^[[:space:]]*environment:[[:space:]]*[^[:space:]]/) {
        env = $0; sub(/^[[:space:]]*environment:[[:space:]]*/, "", env); sub(/[[:space:]]+#.*$/, "", env)
      }
      if ($0 ~ /secrets\.RELEASE_SIGNING_KEY/) key = 1
    }
    END { finish() }
  ' "$1"
}

# The build matrix in release.yml, one line per row:
# "<asset> <linux build or -> <target or ->".
release_matrix() {
  awk '
    /^ *- asset: / { if (asset != "") print asset, linux, target
                     asset = $3; linux = "-"; target = "-"; next }
    asset != "" && /^ *linux: / { linux = $2; next }
    asset != "" && /^ *target: / { target = $2; next }
    asset != "" && /^ *runs-on:/ { print asset, linux, target; asset = "" }
  ' "$here/../../.github/workflows/release.yml"
}

test_the_release_workflow_builds_and_checks_every_asset_a_release_ships() {
  # One row per asset assemble.sh ships, no more: a -gnu or -musl asset
  # with its Linux build, and a musl one with the musl target of its
  # processor.
  local asset arch
  "$here/assemble.sh" --list | while read -r asset; do
    case $asset in
      oskac-linux-*-gnu) echo "$asset gnu -" ;;
      oskac-linux-*-musl)
        arch=${asset#oskac-linux-}
        echo "$asset musl ${arch%-musl}-unknown-linux-musl"
        ;;
      oskac-linux-*) fail "assemble.sh ships $asset, which names no Linux build" ;;
      *) echo "$asset - -" ;;
    esac
  done | LC_ALL=C sort >expected
  [ "$(wc -l <expected)" -ge 7 ] || fail "assemble.sh --list lists $(wc -l <expected) assets"
  release_matrix | LC_ALL=C sort >matrix
  diff expected matrix || fail "release.yml does not build exactly the assets assemble.sh ships"
  # Every row's binary is checked by its Linux build's rules.
  # shellcheck disable=SC2016 # the workflow's shell variables, written out
  grep -qF 'scripts/release/check-linux-build.sh "$LINUX_BUILD" "dist/$ASSET"' \
    "$here/../../.github/workflows/release.yml" || fail "release.yml does not check the Linux builds"
}

test_only_jobs_in_the_release_environment_read_the_signing_key() {
  local workflows=$here/../../.github/workflows file job env key readers=0
  for file in "$workflows"/*.yml; do
    while read -r job env key; do
      [ "$key" = yes ] || continue
      readers=$((readers + 1))
      [ "$env" = release ] || fail "$(basename "$file"): job $job reads RELEASE_SIGNING_KEY outside the release environment"
    done < <(workflow_jobs "$file")
  done
  # release.yml's sign job and resign-index.yml's.
  [ "$readers" = 2 ] || fail "$readers jobs read the key, not 2"
}

# Prints the name of each job in workflow $1 that has a line matching the
# extended regular expression $2, reading indentation as workflow_jobs does.
jobs_matching() {
  awk -v pattern="$2" '
    /^[[:space:]]*(#.*)?$/ { next }
    { ind = $0; sub(/[^ ].*$/, "", ind); ind = length(ind) }
    ind == 0 { in_jobs = ($0 ~ /^jobs:/); job = ""; next }
    !in_jobs { next }
    job_indent == "" { job_indent = ind }
    ind == job_indent { job = $0; sub(/^[[:space:]]*/, "", job); sub(/:.*$/, "", job); next }
    job != "" && $0 ~ pattern { print job }
  ' "$1" | LC_ALL=C sort -u
}

# Prints the lines of job $2 in workflow $1, from its name to the next job.
job_text() {
  awk -v want="$2" '
    /^[[:space:]]*(#.*)?$/ { if (job == want) print; next }
    { ind = $0; sub(/[^ ].*$/, "", ind); ind = length(ind) }
    ind == 0 { in_jobs = ($0 ~ /^jobs:/); job = ""; next }
    !in_jobs { next }
    job_indent == "" { job_indent = ind }
    ind == job_indent { job = $0; sub(/^[[:space:]]*/, "", job); sub(/:.*$/, "", job) }
    job == want { print }
  ' "$1"
}

# Only the two jobs that push the image may write packages, in every
# workflow, and a workflow-wide permission would give it to every job.
test_only_the_image_push_jobs_can_write_packages() {
  local workflows=$here/../../.github/workflows file found=
  for file in "$workflows"/*.yml; do
    if grep -Eq '^permissions:.*(packages|write-all)' "$file" ||
      [ -n "$(awk '/^permissions:/ {p = 1; next} /^[^ ]/ {p = 0} p && /packages/' "$file")" ]; then
      fail "$(basename "$file") gives packages access to every job"
    fi
    while read -r job; do
      [ -n "$job" ] || continue
      found="$found $(basename "$file"):$job"
    done < <(jobs_matching "$file" 'packages:|write-all')
  done
  [ "$found" = " release.yml:publish release.yml:tag-image" ] || fail "jobs with packages access:$found"
}

# The image is pushed by digest in the publish job, after the sign job,
# from the signed release, before the GitHub Release exists. Its public
# tags (vX.Y.Z, latest) come only in tag-image, which needs publish, so
# only once the GitHub Release exists: a release that fails to publish
# leaves no tag naming it. Only push-image.sh pushes, which refuses a
# digest the signature does not cover.
test_the_image_is_pushed_by_digest_before_publishing_and_tagged_after() {
  local wf=$here/../../.github/workflows/release.yml publish tagjob
  publish=$(job_text "$wf" publish)
  tagjob=$(job_text "$wf" tag-image)
  [ -n "$tagjob" ] || fail "release.yml has no tag-image job"
  grep -q '^    needs: sign$' <<<"$publish" || fail "publish does not need sign"
  grep -q '^    needs: \[sign, publish\]$' <<<"$tagjob" || fail "tag-image does not need publish"
  local job text
  for job in publish tag-image; do
    text=$(job_text "$wf" "$job")
    grep -q '^    if: github.event_name == '"'push'"'$' <<<"$text" || fail "$job runs on a pull request"
    grep -q '^          name: image-layout$' <<<"$text" || fail "$job does not push the layout assemble wrote"
    # No package is installed in a job that holds write tokens.
    if grep -Eq 'apt-get|apt install|pip install|npm install|curl' <<<"$text"; then
      fail "$job installs software"
    fi
  done
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -qF 'scripts/release/push-image.sh digest "release/$TAG" site/oska-release.pub image ghcr.io/aparsons/oskac' <<<"$publish" ||
    fail "publish does not push by digest with push-image.sh from the signed release"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -qF 'scripts/release/push-image.sh tags "release/$TAG" site/oska-release.pub image ghcr.io/aparsons/oskac' <<<"$tagjob" ||
    fail "tag-image does not tag with push-image.sh from the signed release"
  if grep -q 'push-image.sh tags' <<<"$publish"; then
    fail "publish tags the image, which may be before the GitHub Release exists"
  fi
  if grep -Eq 'push-image.sh digest|gh release create' <<<"$tagjob"; then
    fail "tag-image does more than tag"
  fi
  local push create
  push=$(grep -n 'push-image.sh digest' <<<"$publish" | head -n 1 | cut -d: -f1)
  create=$(grep -n 'gh release create' <<<"$publish" | head -n 1 | cut -d: -f1)
  [ -n "$push" ] && [ -n "$create" ] && [ "$push" -lt "$create" ] ||
    fail "the image is not pushed by digest before the GitHub Release is created"
  # No other job, in any workflow, runs skopeo copy or docker push.
  local file
  for file in "$here"/../../.github/workflows/*.yml; do
    while read -r job; do
      [ -n "$job" ] || continue
      case "$(basename "$file"):$job" in
        release.yml:publish | release.yml:tag-image) ;;
        *) fail "$(basename "$file"):$job may push an image" ;;
      esac
    done < <(jobs_matching "$file" 'push-image\.sh|skopeo (copy|login)[^#]*docker://|docker push|skopeo login')
  done
}

# CI's musl job checks that the build is static with the same tested
# script as the release, not a copy of its own that could drift.
test_ci_checks_the_musl_build_with_the_release_check() {
  local musl
  musl=$(job_text "$here/../../.github/workflows/ci.yml" musl)
  [ -n "$musl" ] || fail "ci.yml has no musl job"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -qF 'scripts/release/check-linux-build.sh musl "target/$TARGET/release/oskac"' <<<"$musl" ||
    fail "the musl job does not run check-linux-build.sh musl"
  if grep -Eq 'readelf|objdump' <<<"$musl"; then
    fail "the musl job has its own static check"
  fi
}

# The dry run, and the release, smoke-test both musl builds in alpine and
# in the image, on each processor, before anything is signed.
test_the_release_smoke_tests_the_musl_builds_on_both_processors() {
  local wf=$here/../../.github/workflows/release.yml smoke
  smoke=$(job_text "$wf" smoke-image)
  [ -n "$smoke" ] || fail "release.yml has no smoke-image job"
  grep -qE '^    if:' <<<"$smoke" && fail "the smoke-image job does not always run"
  grep -q 'runner: ubuntu-24.04$' <<<"$smoke" || fail "no x86_64 row"
  grep -q 'runner: ubuntu-24.04-arm$' <<<"$smoke" || fail "no aarch64 row"
  grep -q 'binary: oskac-linux-x86_64-musl$' <<<"$smoke" || fail "no x86_64 musl build"
  grep -q 'binary: oskac-linux-aarch64-musl$' <<<"$smoke" || fail "no aarch64 musl build"
  grep -q 'scripts/release/smoke-image.sh ' <<<"$smoke" || fail "does not run smoke-image.sh"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -q 'test "$(uname -m)" = "$MACHINE"' <<<"$smoke" || fail "does not check the processor"
  grep -q '^    needs: \[assemble, smoke-image\]$' <<<"$(job_text "$wf" sign)" ||
    fail "sign does not wait for the smoke test"
}

test_the_index_is_resigned_monthly_and_on_demand_from_main_only() {
  local wf=$here/../../.github/workflows/resign-index.yml
  [ -f "$wf" ] || fail "no resign-index.yml"
  grep -Eq "^[[:space:]]+- cron: '[0-9]+ [0-9]+ [0-9]+ \* \*'" "$wf" || fail "no monthly schedule"
  grep -q '^  workflow_dispatch:' "$wf" || fail "cannot be run on demand"
  # It signs nothing for a ref other than main, which the release
  # environment admits besides the v* tags.
  [ "$(grep -c "if: github.ref == 'refs/heads/main'" "$wf")" -ge 1 ] || fail "does not refuse other refs"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -q 'sign-index.sh "$key" site/oska-release.pub https://oskalang.dev' "$wf" ||
    fail "does not re-sign the live index with no --tag"
  # Its wait ignores the dry run release.yml does on a pull request.
  grep -q 'select(.event == "push" or .event == "workflow_dispatch")' "$wf" ||
    fail "the wait counts runs that cannot publish"
  grep -q 'gh workflow run pages.yml --ref main' "$wf" || fail "does not publish through pages.yml"
}

# ---- Release notes in release.yml ------------------------------------------
#
# notes.sh itself is tested by scripts/release/notes-test.sh. These are
# the checks around its steps: the notes step runs on a pull request too,
# so the dry run fails for a version with no section; it comes before sign,
# in a job that holds no secret, no environment and no permission of its
# own; and no workflow rewrites a release body.

test_the_release_workflow_writes_the_notes_before_signing() {
  local wf=$here/../../.github/workflows/release.yml assemble step
  assemble=$(job_text "$wf" assemble)
  step=$(awk '/- name: Write the release notes$/ {p = 1; print; next} p && /^      - / {exit} p' <<<"$assemble")
  [ -n "$step" ] || fail "assemble does not write the release notes"
  grep -q '^        if:' <<<"$step" && fail "the notes step does not run on every event"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -qF 'scripts/release/notes.sh "$TAG" CHANGELOG.md site/oska-release.pub' <<<"$step" ||
    fail "the notes step does not run notes.sh with the committed key"
  # shellcheck disable=SC2016 # the workflow's own text, not expanded here
  grep -qF 'TAG: ${{ steps.tag.outputs.tag }}' <<<"$step" ||
    fail "the notes step does not take the tag that assemble named"
  grep -Eq '^    (environment|permissions):|secrets\.' <<<"$assemble" &&
    fail "assemble holds a secret, an environment or a permission"
  # Before the dry run signs and before the unsigned release is uploaded.
  local notes dry upload
  notes=$(grep -n 'name: Write the release notes' <<<"$assemble" | cut -d: -f1)
  dry=$(grep -n 'name: Sign and verify with a throwaway key' <<<"$assemble" | cut -d: -f1)
  upload=$(grep -n 'name: unsigned-' <<<"$assemble" | cut -d: -f1)
  [ "$notes" -lt "$dry" ] && [ "$notes" -lt "$upload" ] ||
    fail "the notes are written after the release is signed or uploaded"
  local file
  for file in "$here"/../../.github/workflows/*.yml; do
    if grep -Eq 'gh release edit|--notes[ =]|--notes$' "$file"; then
      fail "$(basename "$file") sets a release body other than from the notes file"
    fi
  done
}

# ---- /.well-known/security.txt ----------------------------------------------

# The value of the field $1 in ./site/.well-known/security.txt.
security_field() {
  sed -n "s/^$1: //p" site/.well-known/security.txt
}

test_security_txt_points_at_private_reporting_and_expires_a_year_after_the_deploy() {
  mkdir site
  OSKA_SITE_URL=https://example.test "$here/security-txt.sh" site
  [ "$(security_field Contact)" = https://github.com/aparsons/oska/security/advisories/new ] ||
    fail "Contact: $(security_field Contact)"
  [ "$(security_field Policy)" = https://example.test/security.html ] ||
    fail "Policy: $(security_field Policy)"
  [ "$(security_field Canonical)" = https://example.test/.well-known/security.txt ] ||
    fail "Canonical: $(security_field Canonical)"
  [ "$(security_field Preferred-Languages)" = en ] || fail "no Preferred-Languages: en"
  expires=$(security_field Expires)
  [[ $expires =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
    fail "Expires is not an RFC 3339 UTC time: $expires"
  days=$((($(date -u -d "$expires" +%s) - $(date -u +%s)) / 86400))
  [ "$days" -ge 364 ] && [ "$days" -le 366 ] || fail "Expires is $days days away: $expires"
  # Nothing else: every line is one of the fields above.
  [ "$(wc -l <site/.well-known/security.txt)" = 5 ] || fail "$(cat site/.well-known/security.txt)"
}

test_security_txt_refuses_a_missing_site_directory() {
  if "$here/security-txt.sh" missing 2>err; then
    fail "wrote into a site that is not there"
  fi
}

# The checker, on ./site served as a file:// URL, without retries.
check_security_txt() {
  OSKA_FETCH_RETRIES=0 "$here/check-security-txt.sh" "file://$PWD/site"
}

test_check_security_txt_accepts_what_the_deploy_writes() {
  mkdir site
  "$here/security-txt.sh" site
  check_security_txt >check.out
  grep -q '^ok: .*/.well-known/security.txt expires ' check.out || fail "no success line: $(cat check.out)"
}

test_check_security_txt_fails_when_it_is_missing() {
  mkdir site
  if check_security_txt 2>err; then
    fail "passed with no security.txt"
  fi
  grep -q 'security.txt' err || fail "error does not name the file: $(cat err)"
}

test_check_security_txt_fails_without_each_required_field() {
  mkdir site
  "$here/security-txt.sh" site
  cp site/.well-known/security.txt full.txt
  for field in Contact Expires Policy Canonical Preferred-Languages; do
    grep -v "^$field: " full.txt >site/.well-known/security.txt
    if check_security_txt 2>err; then
      fail "passed without $field"
    fi
    grep -q "$field" err || fail "error does not name $field: $(cat err)"
  done
}

test_check_security_txt_fails_once_it_has_expired() {
  mkdir site
  "$here/security-txt.sh" site
  sed -i 's/^Expires: .*/Expires: 2020-01-01T00:00:00Z/' site/.well-known/security.txt
  if check_security_txt 2>err; then
    fail "passed with an Expires in the past"
  fi
  grep -q 'expired' err || fail "error does not say it expired: $(cat err)"
}

test_check_security_txt_fails_on_an_expires_it_cannot_read() {
  mkdir site
  "$here/security-txt.sh" site
  sed -i 's/^Expires: .*/Expires: next year/' site/.well-known/security.txt
  if check_security_txt 2>err; then
    fail "passed with an Expires that is not a time"
  fi
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
