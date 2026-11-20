#!/usr/bin/env bash
# Tests for the image builder, scripts/release/image.sh. Run from anywhere:
#
#   scripts/release/image-test.sh
#
# Each test works in its own scratch directory: it makes fake musl binaries,
# builds the image layout from them, and checks what a registry or a security
# reviewer would see. It needs jq and GNU tar (scripts/check.sh runs it on
# Linux).

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

command -v jq >/dev/null 2>&1 || {
  echo "image-test.sh: jq is not installed" >&2
  exit 1
}

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

# 2026-11-20T00:00:00Z
export SOURCE_DATE_EPOCH=1795132800

# Writes the two fake musl binaries into ./bin.
fake_binaries() {
  mkdir -p bin
  printf 'fake musl oskac for x86_64\n' >bin/oskac-linux-x86_64-musl
  printf 'fake musl oskac for aarch64\n' >bin/oskac-linux-aarch64-musl
}

# Builds the image from ./bin into $1 and prints what image.sh printed.
build() {
  "$here/image.sh" bin/oskac-linux-x86_64-musl bin/oskac-linux-aarch64-musl "$1"
}

# The blob a digest names, in layout $1.
blob() {
  printf '%s/blobs/sha256/%s' "$1" "${2#sha256:}"
}

# The image manifest for architecture $2 in layout $1.
manifest_for() {
  local index
  index=$(jq -r '.manifests[0].digest' "$1/index.json")
  jq -r --arg arch "$2" \
    '.manifests[] | select(.platform.architecture == $arch) | .digest' \
    "$(blob "$1" "$index")"
}

test_same_inputs_in_two_directories_give_the_same_digest() {
  fake_binaries
  mkdir a b
  cp -p bin/* a/
  cp -p bin/* b/
  chmod 600 b/*
  touch -d '2001-01-01' b/*
  first=$(cd a && "$here/image.sh" oskac-linux-x86_64-musl oskac-linux-aarch64-musl out)
  second=$(cd b && "$here/image.sh" oskac-linux-x86_64-musl oskac-linux-aarch64-musl out)
  [[ $first =~ ^sha256:[0-9a-f]{64}$ ]] || fail "printed '$first', not a digest"
  [ "$first" = "$second" ] || fail "digests differ: $first $second"
  diff -r a/out b/out || fail "the layouts differ"
}

test_the_index_has_one_image_per_linux_processor() {
  fake_binaries
  digest=$(build out)
  [ "$(cat out/oci-layout)" = '{"imageLayoutVersion":"1.0.0"}' ] ||
    fail "oci-layout: $(cat out/oci-layout)"
  [ "$(jq -r '.manifests | length' out/index.json)" = 1 ] ||
    fail "index.json does not hold exactly one entry"
  [ "$(jq -r '.manifests[0].digest' out/index.json)" = "$digest" ] ||
    fail "index.json does not point at the printed digest $digest"
  [ "$(jq -r '.manifests[0].mediaType' out/index.json)" = \
    application/vnd.oci.image.index.v1+json ] || fail "index.json entry is not an image index"
  index=$(blob out "$digest")
  [ "$(jq -c '[.manifests[].platform]' "$index")" = \
    '[{"architecture":"amd64","os":"linux"},{"architecture":"arm64","os":"linux"}]' ] ||
    fail "platforms: $(jq -c '[.manifests[].platform]' "$index")"
}

# Checks the image for OCI architecture $1: config, and a layer holding
# binary $2 as /oskac and an empty /tmp, nothing else.
check_image() {
  local manifest config layer listing
  manifest=$(blob out "$(manifest_for out "$1")")
  [ "$(jq -r '.layers | length' "$manifest")" = 1 ] || fail "$1: not exactly one layer"
  config=$(blob out "$(jq -r '.config.digest' "$manifest")")
  [ "$(jq -c .config "$config")" = '{"User":"65532:65532","Entrypoint":["/oskac"]}' ] ||
    fail "$1: config $(jq -c .config "$config")"
  [ "$(jq -r '.architecture + " " + .os + " " + .created' "$config")" = \
    "$1 linux 2026-11-20T00:00:00Z" ] || fail "$1: platform or created wrong in config"
  layer=$(blob out "$(jq -r '.layers[0].digest' "$manifest")")
  [ "$(jq -r '.rootfs.diff_ids[0]' "$config")" = "$(jq -r '.layers[0].digest' "$manifest")" ] ||
    fail "$1: diff_id is not the layer digest"
  listing=$(LC_ALL=C TZ=UTC tar --numeric-owner --full-time -tvf "$layer" |
    awk '{print $1, $2, $3, $4, $5, $6}')
  [ "$listing" = "-rwxr-xr-x 0/0 $(wc -c <"$2" | tr -d ' ') 2026-11-20 00:00:00 oskac
drwxrwxrwt 0/0 0 2026-11-20 00:00:00 tmp/" ] || fail "$1: layer holds
$listing"
  mkdir "x-$1"
  tar -xf "$layer" -C "x-$1"
  cmp "x-$1/oskac" "$2" || fail "$1: /oskac is not $2"
  [ -z "$(ls -A "x-$1/tmp")" ] || fail "$1: /tmp is not empty"
}

test_each_image_holds_only_its_binary_and_tmp_and_runs_as_65532() {
  fake_binaries
  build out >/dev/null
  check_image amd64 bin/oskac-linux-x86_64-musl
  check_image arm64 bin/oskac-linux-aarch64-musl
}

test_a_changed_binary_gives_a_different_digest() {
  fake_binaries
  before=$(build a)
  printf 'fake musl oskac for aarch64, patched\n' >bin/oskac-linux-aarch64-musl
  after=$(build b)
  [ "$before" != "$after" ] || fail "digest unchanged: $before"
}

test_a_changed_source_date_epoch_gives_a_different_digest() {
  fake_binaries
  before=$(build a)
  after=$(SOURCE_DATE_EPOCH=1795132801 build b)
  [ "$before" != "$after" ] || fail "digest unchanged: $before"
}

# The digest of these fake inputs, written down once. A change to any byte
# the builder writes changes it, and then an older release no longer
# rebuilds to its signed digest. Change it only on purpose.
test_the_digest_of_fixed_inputs_never_drifts() {
  fake_binaries
  digest=$(build out)
  [ "$digest" = sha256:84a62c946cf5a0ff8ab1b402715e540674157adaad31ae7b51c8e5ed3e2cc670 ] || fail "digest is now $digest"
}

test_every_blob_matches_its_digest_and_descriptor_size() {
  fake_binaries
  build out >/dev/null
  count=0
  for file in out/blobs/sha256/*; do
    sum=$(sha256sum <"$file")
    [ "${sum%% *}" = "$(basename "$file")" ] || fail "$file does not match its name"
    count=$((count + 1))
  done
  # One index, and per processor a manifest, a config and a layer.
  [ "$count" = 7 ] || fail "$count blobs, not 7"
  [ -z "$(find out -name '*.tmp')" ] || fail "a temporary file is left"
  # Every descriptor, from index.json down through the index and manifests.
  index=$(jq -r '.manifests[0].digest' out/index.json)
  descriptors=$(
    jq -r '.manifests[0] | "\(.digest) \(.size)"' out/index.json
    jq -r '.manifests[] | "\(.digest) \(.size)"' "$(blob out "$index")"
    for manifest in $(jq -r '.manifests[].digest' "$(blob out "$index")"); do
      jq -r '.config, .layers[] | "\(.digest) \(.size)"' "$(blob out "$manifest")"
    done
  )
  [ "$(printf '%s\n' "$descriptors" | wc -l | tr -d ' ')" = 7 ] ||
    fail "descriptors: $descriptors"
  while read -r digest size; do
    [ "$(wc -c <"$(blob out "$digest")" | tr -d ' ')" = "$size" ] ||
      fail "$digest is not $size bytes"
  done <<<"$descriptors"
}

# Runs image.sh with the given arguments and environment changes, and
# checks that it fails with a message that contains $1.
refuses() {
  local want=$1
  shift
  if out=$("$@" 2>&1); then
    fail "accepted: $*"
  fi
  [[ $out == *"$want"* ]] || fail "message '$out' does not say '$want'"
}

test_refuses_a_missing_or_malformed_source_date_epoch() {
  fake_binaries
  args=(bin/oskac-linux-x86_64-musl bin/oskac-linux-aarch64-musl out)
  refuses "SOURCE_DATE_EPOCH is not set" env -u SOURCE_DATE_EPOCH "$here/image.sh" "${args[@]}"
  # shellcheck disable=SC2016 # a command substitution that must stay text
  for bad in "" -1 1.5 0x10 "1 2" 017 99999999999 '$(touch pwned)'; do
    refuses "SOURCE_DATE_EPOCH" env SOURCE_DATE_EPOCH="$bad" "$here/image.sh" "${args[@]}"
  done
  [ ! -e pwned ] || fail "SOURCE_DATE_EPOCH ran a command"
}

test_refuses_a_missing_empty_or_symlinked_binary() {
  fake_binaries
  refuses "is not a regular file" "$here/image.sh" bin/nope bin/oskac-linux-aarch64-musl out
  : >bin/empty
  refuses "is empty" "$here/image.sh" bin/oskac-linux-x86_64-musl bin/empty out
  ln -s /bin/sh bin/link
  refuses "is not a regular file" "$here/image.sh" bin/link bin/oskac-linux-aarch64-musl out
  mkdir bin/dir
  refuses "is not a regular file" "$here/image.sh" bin/oskac-linux-x86_64-musl bin/dir out
  refuses "usage" "$here/image.sh" bin/oskac-linux-x86_64-musl
}

test_refuses_an_output_directory_that_is_not_empty() {
  fake_binaries
  mkdir out
  printf 'stale\n' >out/blob
  refuses "is not empty" build out
  printf 'file\n' >file
  refuses "is not a directory" build file
  mkdir real
  ln -s real link
  refuses "is not a directory" build link
  [ -z "$(ls -A real)" ] || fail "wrote through the symlink"
}

run_test test_same_inputs_in_two_directories_give_the_same_digest
run_test test_a_changed_binary_gives_a_different_digest
run_test test_a_changed_source_date_epoch_gives_a_different_digest
run_test test_the_digest_of_fixed_inputs_never_drifts
run_test test_every_blob_matches_its_digest_and_descriptor_size
run_test test_refuses_a_missing_or_malformed_source_date_epoch
run_test test_refuses_a_missing_empty_or_symlinked_binary
run_test test_refuses_an_output_directory_that_is_not_empty
run_test test_the_index_has_one_image_per_linux_processor
run_test test_each_image_holds_only_its_binary_and_tmp_and_runs_as_65532

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
