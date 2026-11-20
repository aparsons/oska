#!/usr/bin/env bash
# Tests for scripts/release/push-image.sh, which pushes the signed oskac
# image to the registry. Run from anywhere:
#
#   scripts/release/push-image-test.sh
#
# Each test works in its own scratch directory: it assembles and signs a
# release of fake binaries, as release.yml does, and runs push-image.sh with
# a fake skopeo on PATH. The fake keeps a registry in ./registry: a copy
# stores the image index the layout names under its tag, and `inspect --raw`
# serves it back, so a test can make the registry serve other bytes.

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

# 2026-11-20T00:00:00Z: the tag's commit time, in the release run.
export SOURCE_DATE_EPOCH=1795132800

REPO=ghcr.io/example/oskac

# Assembles and signs ./release/<tag> (default v1.2.3) from fake binaries
# with the throwaway key ./key, as the release run's assemble and sign jobs
# do, the signed release index beside it included.
signed_release() {
  local tag=${1:-v1.2.3} asset
  mkdir -p builds
  while read -r asset; do
    printf 'binary for %s\n' "$asset" >"builds/$asset"
  done < <("$here/assemble.sh" --list)
  [ -f key ] || ssh-keygen -q -t ed25519 -N '' -C push-image-test -f key
  "$here/assemble.sh" "$tag" builds release >/dev/null
  "$here/sign.sh" "release/$tag" key key.pub
  "$here/sign-index.sh" --tag "$tag" key key.pub "file://$PWD/no-site" "release/$tag" >/dev/null
}

# A fake skopeo in ./fakebin. FAKE_SKOPEO picks how it misbehaves:
#   serve_other  the registry serves other bytes than were pushed
#   fail_copy    every copy fails
fake_skopeo() {
  mkdir -p fakebin registry
  cat >fakebin/skopeo <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
reg=$FAKE_REGISTRY
printf '%s\n' "$*" >>"$reg/calls"
case $1 in
  copy)
    [ "${FAKE_SKOPEO:-}" != fail_copy ] || { echo "fake skopeo: copy failed" >&2; exit 1; }
    src= dst=
    for arg in "$@"; do
      case $arg in
        oci:*) src=${arg#oci:} ;;
        docker://*) dst=${arg#docker://} ;;
      esac
    done
    digest=$(jq -r '.manifests[0].digest' "$src/index.json")
    cp "$src/blobs/sha256/${digest#sha256:}" "$reg/${dst##*:}"
    ;;
  inspect)
    ref=${*: -1}
    ref=${ref#docker://}
    cat "$reg/${ref##*:}"
    [ "${FAKE_SKOPEO:-}" != serve_other ] || printf ' '
    ;;
  *) echo "fake skopeo: unexpected $*" >&2; exit 2 ;;
esac
FAKE
  chmod +x fakebin/skopeo
}

# Runs push-image.sh with the fake skopeo on release ./release/<tag>.
push() {
  local tag=${1:-v1.2.3}
  PATH="$PWD/fakebin:$PATH" FAKE_REGISTRY="$PWD/registry" \
    "$here/push-image.sh" "release/$tag" key.pub "$REPO"
}

# The skopeo commands that pushed, one per line: "<tag>".
pushed_tags() {
  if [ -f registry/calls ]; then
    sed -n "s|^copy .* docker://$REPO:\([^ ]*\)\$|\1|p" registry/calls
  fi
}

test_pushes_the_signed_digest_as_the_tag_and_latest() {
  signed_release v1.2.3
  fake_skopeo
  push v1.2.3 >out
  [ "$(pushed_tags | tr '\n' ' ')" = "v1.2.3 latest " ] ||
    fail "pushed $(pushed_tags | tr '\n' ' ')"
  local signed
  signed=$(cat release/v1.2.3/oskac-image-digest)
  for tag in v1.2.3 latest; do
    [ "sha256:$(sha256sum <"registry/$tag" | cut -c1-64)" = "$signed" ] ||
      fail "$tag does not hold the signed image index"
  done
  # Every image of the index, both processors, with the bytes unchanged.
  grep -q -- '^copy .*--all .*--preserve-digests ' registry/calls ||
    fail "did not copy every image with its digests kept: $(cat registry/calls)"
  grep -qF "$REPO@$signed" out || fail "does not print the pullable reference: $(cat out)"
}

test_a_pre_release_does_not_move_latest() {
  signed_release v1.3.0-rc.1
  fake_skopeo
  push v1.3.0-rc.1 >/dev/null
  [ "$(pushed_tags | tr '\n' ' ')" = "v1.3.0-rc.1 " ] ||
    fail "pushed $(pushed_tags | tr '\n' ' ')"
}

# The registry ends up serving bytes the signature does not cover: the
# release fails, and latest never moves to them.
test_refuses_when_the_pushed_digest_differs_from_the_signed_one() {
  signed_release v1.2.3
  fake_skopeo
  if FAKE_SKOPEO=serve_other push v1.2.3 2>err; then
    fail "passed with a pushed digest the signature does not cover"
  fi
  grep -q 'not the signed' err || fail "error does not say why: $(cat err)"
  [ "$(pushed_tags | tr '\n' ' ')" = "v1.2.3 " ] ||
    fail "went on after the mismatch: pushed $(pushed_tags | tr '\n' ' ')"
}

# The signed digest names another image than the signed musl builds make,
# for example one built from other binaries: nothing is pushed.
test_refuses_an_image_the_signed_musl_builds_do_not_rebuild_to() {
  signed_release v1.2.3
  fake_skopeo
  local other
  other=$(SOURCE_DATE_EPOCH=1 "$here/image.sh" builds/oskac-linux-x86_64-musl \
    builds/oskac-linux-aarch64-musl other-layout)
  printf '%s\n' "$other" >release/v1.2.3/oskac-image-digest
  rm release/v1.2.3/SHA256SUMS.sig
  {
    head -n 1 release/v1.2.3/SHA256SUMS
    (cd release/v1.2.3 && find . -maxdepth 1 -name 'oskac-*' -printf '%P\n' |
      LC_ALL=C sort | xargs sha256sum --)
  } >sums
  mv sums release/v1.2.3/SHA256SUMS
  ssh-keygen -q -Y sign -f key -n oska-release release/v1.2.3/SHA256SUMS
  if push v1.2.3 2>err; then
    fail "pushed an image the musl builds do not rebuild to"
  fi
  grep -q 'rebuild' err || fail "error does not say why: $(cat err)"
  [ -z "$(pushed_tags)" ] || fail "pushed $(pushed_tags)"
}

# The same image, rebuilt with another SOURCE_DATE_EPOCH (not the tag's
# commit time), is another digest: refused, nothing pushed.
test_refuses_when_the_source_date_is_not_the_one_the_release_used() {
  signed_release v1.2.3
  fake_skopeo
  if SOURCE_DATE_EPOCH=1795132801 push v1.2.3 2>err; then
    fail "pushed with another SOURCE_DATE_EPOCH"
  fi
  [ -z "$(pushed_tags)" ] || fail "pushed $(pushed_tags)"
}

test_refuses_a_release_that_does_not_verify() {
  local case
  for case in tampered_binary other_key unsigned_file no_signature; do
    rm -rf release registry builds key key.pub other other.pub
    signed_release v1.2.3
    fake_skopeo
    case $case in
      tampered_binary) printf 'evil\n' >>release/v1.2.3/oskac-linux-aarch64-musl ;;
      other_key)
        ssh-keygen -q -t ed25519 -N '' -C other -f other
        rm release/v1.2.3/SHA256SUMS.sig
        ssh-keygen -q -Y sign -f other -n oska-release release/v1.2.3/SHA256SUMS
        ;;
      unsigned_file) printf 'extra\n' >release/v1.2.3/oskac-linux-riscv64-musl ;;
      no_signature) rm release/v1.2.3/SHA256SUMS.sig ;;
    esac
    if push v1.2.3 2>err; then
      fail "$case: pushed from a release that does not verify"
    fi
    [ -z "$(pushed_tags)" ] || fail "$case: pushed $(pushed_tags)"
  done
}

test_refuses_a_release_with_no_image_digest() {
  signed_release v1.2.3
  fake_skopeo
  rm release/v1.2.3/oskac-image-digest
  {
    head -n 1 release/v1.2.3/SHA256SUMS
    grep -v ' oskac-image-digest$' release/v1.2.3/SHA256SUMS | tail -n +2
  } >sums
  mv sums release/v1.2.3/SHA256SUMS
  rm release/v1.2.3/SHA256SUMS.sig
  ssh-keygen -q -Y sign -f key -n oska-release release/v1.2.3/SHA256SUMS
  if push v1.2.3 2>err; then
    fail "pushed with no signed digest"
  fi
  grep -q oskac-image-digest err || fail "error does not name the digest file: $(cat err)"
  [ -z "$(pushed_tags)" ] || fail "pushed $(pushed_tags)"
}

test_a_failed_copy_fails_the_push() {
  signed_release v1.2.3
  fake_skopeo
  if FAKE_SKOPEO=fail_copy push v1.2.3 2>err; then
    fail "passed with a failed copy"
  fi
  [ -z "$(sed -n '/^copy .*:latest$/p' registry/calls)" ] || fail "went on to latest"
}

test_refuses_a_bad_repository_or_missing_skopeo() {
  signed_release v1.2.3
  fake_skopeo
  local bad
  for bad in '' 'ghcr.io/x/y:tag' 'ghcr.io/x/y@sha256:00' 'ghcr.io/X/y' '-ghcr.io/x' 'ghcr.io/x y'; do
    if PATH="$PWD/fakebin:$PATH" FAKE_REGISTRY="$PWD/registry" \
      "$here/push-image.sh" release/v1.2.3 key.pub "$bad" 2>/dev/null; then
      fail "accepted the repository '$bad'"
    fi
  done
  [ -z "$(pushed_tags)" ] || fail "pushed $(pushed_tags)"
  # No skopeo at all: refused, not skipped. PATH keeps only what the
  # script needs besides skopeo.
  mkdir nobin
  local tool
  for tool in bash env jq sha256sum ssh-keygen head tail wc od tr cut awk sort find cp mv \
    rm mkdir mktemp basename dirname cat grep ls printf xargs; do
    if command -v "$tool" >/dev/null 2>&1 && [ -x "$(command -v "$tool")" ]; then
      ln -sf "$(command -v "$tool")" "nobin/$tool"
    fi
  done
  if PATH="$PWD/nobin" "$here/push-image.sh" release/v1.2.3 key.pub "$REPO" 2>err; then
    fail "passed with no skopeo"
  fi
  grep -q skopeo err || fail "error does not name skopeo: $(cat err)"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
