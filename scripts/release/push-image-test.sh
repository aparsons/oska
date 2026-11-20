#!/usr/bin/env bash
# Tests for scripts/release/push-image.sh, which pushes the signed oskac
# image to the registry. Run from anywhere:
#
#   scripts/release/push-image-test.sh
#
# Each test works in its own scratch directory: it assembles and signs a
# release of fake binaries, as release.yml does, and runs push-image.sh on
# it and the image layout assemble.sh wrote, with a fake skopeo on PATH.
# The fake keeps a registry in ./registry, so a test can make the registry
# serve other bytes than were pushed.

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

# assemble.sh needs it to build the image: 2026-11-20T00:00:00Z.
export SOURCE_DATE_EPOCH=1795132800

REPO=ghcr.io/example/oskac

# Assembles and signs ./release/<tag> (default v1.2.3) from fake binaries
# with the throwaway key ./key, as the release run's assemble and sign jobs
# do, the signed release index beside it included. The image layout
# assemble.sh wrote is ./layout-<tag>. A live site in ./site, if there is
# one, is where sign-index.sh reads the published index from.
signed_release() {
  local tag=${1:-v1.2.3} asset
  mkdir -p builds
  while read -r asset; do
    printf 'binary for %s\n' "$asset" >"builds/$asset"
  done < <("$here/assemble.sh" --list)
  [ -f key ] || ssh-keygen -q -t ed25519 -N '' -C push-image-test -f key
  "$here/assemble.sh" "$tag" builds release "layout-$tag" >/dev/null
  "$here/sign.sh" "release/$tag" key key.pub
  "$here/sign-index.sh" --tag "$tag" key key.pub "file://$PWD/site" "release/$tag" >/dev/null
}

# A fake skopeo in ./fakebin. A copy stores the image index the layout
# names under the destination's tag or digest; `inspect --raw` serves it.
# FAKE_SKOPEO picks how it misbehaves:
#   serve_other   the registry serves other bytes than were pushed
#   fail_copy     every copy fails
#   fail_inspect  every inspect fails
fake_skopeo() {
  mkdir -p fakebin registry
  cat >fakebin/skopeo <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
reg=$FAKE_REGISTRY
printf '%s\n' "$*" >>"$reg/calls"
# The key a reference is stored under: @sha256:<hex> or :<tag>.
key() {
  local ref=${1#docker://}
  case $ref in
    *@*) echo "@${ref##*@}" ;;
    *) echo ":${ref##*:}" ;;
  esac
}
case $1 in
  copy)
    [ "${FAKE_SKOPEO:-}" != fail_copy ] || { echo "fake skopeo: copy failed" >&2; exit 1; }
    src= dst=
    for arg in "$@"; do
      case $arg in
        oci:*) src=${arg#oci:} ;;
        docker://*) dst=$arg ;;
      esac
    done
    digest=$(jq -r '.manifests[0].digest' "$src/index.json")
    cp "$src/blobs/sha256/${digest#sha256:}" "$reg/$(key "$dst")"
    echo "$(key "$dst")" >>"$reg/pushed"
    ;;
  inspect)
    [ "${FAKE_SKOPEO:-}" != fail_inspect ] || { echo "fake skopeo: inspect failed" >&2; exit 1; }
    cat "$reg/$(key "${*: -1}")"
    [ "${FAKE_SKOPEO:-}" != serve_other ] || printf ' '
    ;;
  *) echo "fake skopeo: unexpected $*" >&2; exit 2 ;;
esac
FAKE
  chmod +x fakebin/skopeo
}

# Runs push-image.sh's step $1 (digest or tags) with the fake skopeo on
# release ./release/<tag> and layout ./layout-<tag>.
push_step() {
  local step=$1 tag=${2:-v1.2.3}
  PATH="$PWD/fakebin:$PATH" FAKE_REGISTRY="$PWD/registry" \
    "$here/push-image.sh" "$step" "release/$tag" key.pub "layout-$tag" "$REPO"
}

# The tags step, which does everything the digest step does and then tags.
push() {
  push_step tags "$@"
}

# What was pushed, in order, on one line: @sha256:<hex> or :<tag> each.
pushed() {
  if [ -f registry/pushed ]; then
    tr '\n' ' ' <registry/pushed | sed 's/ $//'
  fi
}

# The signed digest of ./release/<tag>.
signed_digest() {
  cat "release/${1:-v1.2.3}/oskac-image-digest"
}

test_pushes_the_signed_digest_then_tags_it_and_latest() {
  signed_release v1.2.3
  fake_skopeo
  push v1.2.3 >out
  local signed
  signed=$(signed_digest)
  [ "$(pushed)" = "@$signed :v1.2.3 :latest" ] || fail "pushed $(pushed)"
  for ref in "@$signed" :v1.2.3 :latest; do
    [ "sha256:$(sha256sum <"registry/$ref" | cut -c1-64)" = "$signed" ] ||
      fail "$ref does not hold the signed image index"
  done
  # Every image of the index, both processors, with the bytes unchanged,
  # from the layout assemble.sh wrote.
  [ "$(grep -c -- "^copy --all --preserve-digests oci:layout-v1.2.3 docker://$REPO" registry/calls)" = 3 ] ||
    fail "did not copy the layout with every image and its digests kept: $(cat registry/calls)"
  grep -qF "$REPO@$signed" out || fail "does not print the pullable reference: $(cat out)"
}

# The step before the GitHub Release exists: by digest only, read back,
# and no public tag, so a failed release leaves no tag naming it.
test_the_digest_step_pushes_by_digest_and_tags_nothing() {
  signed_release v1.2.3
  fake_skopeo
  push_step digest v1.2.3 >out
  local signed
  signed=$(signed_digest)
  [ "$(pushed)" = "@$signed" ] || fail "pushed $(pushed)"
  [ "$(grep -c '^inspect --raw ' registry/calls)" = 1 ] || fail "did not read the push back: $(cat registry/calls)"
  grep -qF "$REPO@$signed" out || fail "does not print the pullable reference: $(cat out)"
}

# The step must be named: the old four-argument call, or an unknown step,
# pushes nothing rather than guessing whether to tag.
test_refuses_a_missing_or_unknown_step() {
  signed_release v1.2.3
  fake_skopeo
  if PATH="$PWD/fakebin:$PATH" FAKE_REGISTRY="$PWD/registry" \
    "$here/push-image.sh" release/v1.2.3 key.pub layout-v1.2.3 "$REPO" 2>err; then
    fail "pushed with no step named"
  fi
  grep -q usage err || fail "error does not give the usage: $(cat err)"
  local step
  for step in '' latest --tags 'digest tags'; do
    if push_step "$step" v1.2.3 2>err; then
      fail "pushed with the step '$step'"
    fi
  done
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
}

test_a_pre_release_does_not_move_latest() {
  signed_release v1.3.0-rc.1
  fake_skopeo
  push v1.3.0-rc.1 >/dev/null
  [ "$(pushed)" = "@$(signed_digest v1.3.0-rc.1) :v1.3.0-rc.1" ] || fail "pushed $(pushed)"
}

# A patch for an older line (v1.2.3 after v2.0.0): the signed release index
# still names v2.0.0, so latest stays where it is, as the index does.
test_an_older_release_does_not_move_latest_back() {
  ssh-keygen -q -t ed25519 -N '' -C push-image-test -f key
  mkdir site
  "$here/sign-index.sh" --tag v2.0.0 key key.pub "file://$PWD/no-site" site >/dev/null
  signed_release v1.2.3
  [ "$(jq -r .latest.tag release/v1.2.3/release-index.json)" = v2.0.0 ] || fail "the index moved back"
  fake_skopeo
  push v1.2.3 >/dev/null
  [ "$(pushed)" = "@$(signed_digest) :v1.2.3" ] || fail "pushed $(pushed)"
}

# The index decides latest, so one that does not verify stops the push.
test_refuses_a_release_index_that_does_not_verify() {
  signed_release v1.2.3
  fake_skopeo
  ssh-keygen -q -t ed25519 -N '' -C attacker -f attacker
  rm release/v1.2.3/release-index.json.sig
  ssh-keygen -q -Y sign -f attacker -n oska-index release/v1.2.3/release-index.json
  if push v1.2.3 2>err; then
    fail "pushed with an index by another key"
  fi
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
}

# The registry serves other bytes for the digest just pushed: the release
# fails before any public tag names them.
test_refuses_when_the_pushed_digest_differs_from_the_signed_one() {
  signed_release v1.2.3
  fake_skopeo
  if FAKE_SKOPEO=serve_other push v1.2.3 2>err; then
    fail "passed with a pushed digest the signature does not cover"
  fi
  grep -q 'not the signed' err || fail "error does not say why: $(cat err)"
  [ "$(pushed)" = "@$(signed_digest)" ] || fail "went on to tag it: pushed $(pushed)"
}

test_refuses_when_the_registry_cannot_be_read_back() {
  signed_release v1.2.3
  fake_skopeo
  if FAKE_SKOPEO=fail_inspect push v1.2.3 2>err; then
    fail "passed without reading the push back"
  fi
  grep -q 'cannot read' err || fail "error does not say why: $(cat err)"
  [ "$(pushed)" = "@$(signed_digest)" ] || fail "went on to tag it: pushed $(pushed)"
}

# A layout that is not the signed image (built from other inputs, here
# another SOURCE_DATE_EPOCH): nothing is pushed.
test_refuses_a_layout_that_is_not_the_signed_image() {
  signed_release v1.2.3
  fake_skopeo
  rm -r layout-v1.2.3
  SOURCE_DATE_EPOCH=1 "$here/image.sh" builds/oskac-linux-x86_64-musl \
    builds/oskac-linux-aarch64-musl layout-v1.2.3 >/dev/null
  if push v1.2.3 2>err; then
    fail "pushed a layout that is not the signed image"
  fi
  grep -q 'not the signed' err || fail "error does not say why: $(cat err)"
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
}

# The signed index, but a blob under it changed (another binary in a
# layer), or a second entry in index.json: nothing is pushed.
test_refuses_a_layout_with_a_changed_blob_or_a_second_image() {
  local case blob
  for case in changed_blob second_entry; do
    rm -rf release registry builds layout-v1.2.3
    signed_release v1.2.3
    fake_skopeo
    case $case in
      changed_blob)
        blob=$(find layout-v1.2.3/blobs/sha256 -type f -size +1k | head -n 1)
        printf 'evil' >>"$blob"
        ;;
      second_entry)
        jq '.manifests += [.manifests[0]]' layout-v1.2.3/index.json >index.json
        mv index.json layout-v1.2.3/index.json
        ;;
    esac
    if push v1.2.3 2>err; then
      fail "$case: pushed"
    fi
    [ -z "$(pushed)" ] || fail "$case: pushed $(pushed)"
  done
}

test_refuses_a_release_that_does_not_verify() {
  local case
  for case in tampered_binary other_key unsigned_file no_signature; do
    rm -rf release registry builds layout-v1.2.3 key key.pub other other.pub
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
    [ -z "$(pushed)" ] || fail "$case: pushed $(pushed)"
  done
}

# A release signed for a name that is no release tag, such as latest, would
# push over that tag: refused even though its signature is good.
test_refuses_a_release_not_named_for_a_release_tag() {
  signed_release v1.2.3
  fake_skopeo
  local name
  for name in latest v1.2 1.2.3 v1.2.3-; do
    rm -rf "release/$name" "layout-$name"
    cp -R release/v1.2.3 "release/$name"
    cp -R layout-v1.2.3 "layout-$name"
    rm "release/$name/release-index.json" "release/$name/release-index.json.sig" "release/$name/SHA256SUMS.sig"
    sed -i "1s/.*/# oska-release $name/" "release/$name/SHA256SUMS"
    ssh-keygen -q -Y sign -f key -n oska-release "release/$name/SHA256SUMS"
    "$here/verify.sh" "release/$name" key.pub || fail "$name: the re-signed release does not verify"
    if push "$name" 2>err; then
      fail "pushed from release/$name"
    fi
    grep -q 'not named for a release tag' err || fail "$name: error does not say why: $(cat err)"
  done
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
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
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
}

test_a_failed_copy_fails_the_push() {
  signed_release v1.2.3
  fake_skopeo
  if FAKE_SKOPEO=fail_copy push v1.2.3 2>err; then
    fail "passed with a failed copy"
  fi
  [ "$(grep -c '^copy ' registry/calls)" = 1 ] || fail "went on after the failed copy: $(cat registry/calls)"
}

test_refuses_a_bad_repository_or_missing_skopeo() {
  signed_release v1.2.3
  fake_skopeo
  local bad
  for bad in '' 'ghcr.io/x/y:tag' 'ghcr.io/x/y@sha256:00' 'ghcr.io/X/y' '-ghcr.io/x' 'ghcr.io/x y'; do
    if PATH="$PWD/fakebin:$PATH" FAKE_REGISTRY="$PWD/registry" \
      "$here/push-image.sh" tags release/v1.2.3 key.pub layout-v1.2.3 "$bad" 2>/dev/null; then
      fail "accepted the repository '$bad'"
    fi
  done
  [ -z "$(pushed)" ] || fail "pushed $(pushed)"
  # No skopeo at all: refused, not skipped. PATH keeps only what the
  # script needs besides skopeo.
  mkdir nobin
  local tool
  for tool in bash env jq sha256sum ssh-keygen head tail wc od tr cut awk sort find cp mv \
    rm mkdir mktemp basename dirname cat grep ls printf xargs date sed; do
    if command -v "$tool" >/dev/null 2>&1 && [ -x "$(command -v "$tool")" ]; then
      ln -sf "$(command -v "$tool")" "nobin/$tool"
    fi
  done
  if PATH="$PWD/nobin" "$here/push-image.sh" tags release/v1.2.3 key.pub layout-v1.2.3 "$REPO" 2>err; then
    fail "passed with no skopeo"
  fi
  grep -q skopeo err || fail "error does not name skopeo: $(cat err)"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
