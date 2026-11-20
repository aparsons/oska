#!/usr/bin/env bash
# Build the `oskac` container image as reproducible OCI bytes (ADR 0022).
#
#   SOURCE_DATE_EPOCH=<seconds> scripts/release/image.sh \
#     <x86_64-musl-binary> <aarch64-musl-binary> <out-dir>
#
# Writes an OCI image layout to <out-dir> (which must not exist, or be empty)
# and prints the digest of its image index, `sha256:<hex>`, on stdout. The
# layout's index.json holds one entry: that image index, which lists one
# image for linux/amd64 and one for linux/arm64. Each image has one
# uncompressed tar layer with exactly two entries:
#
#   oskac  the musl build for that processor, mode 0755
#   tmp/   an empty directory, mode 1777
#
# both owned by uid and gid 0, with mtime SOURCE_DATE_EPOCH. Its config runs
# `/oskac` as user 65532:65532, with no Env, and its `created` time is
# SOURCE_DATE_EPOCH (the release tag's commit time).
#
# The same inputs give the same bytes, and so the same digest, on any host:
# the script writes the tar headers itself (no tar implementation's defaults,
# no file owner, mode or time from the host), leaves the layer uncompressed
# (no compressor's version in the bytes), and writes JSON with fixed key
# order. A security reviewer rebuilds the image from a release's signed musl
# binaries and compares the digest. Changing any byte this script writes
# changes every digest, so an older release no longer rebuilds to its signed
# digest: image-test.sh pins one.
#
# Needs bash, coreutils (sha256sum, head, wc, mkdir, mv) and jq. No Docker.

set -euo pipefail

die() {
  echo "image.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || die "usage: image.sh <x86_64-musl-binary> <aarch64-musl-binary> <out-dir>"
amd64_bin=$1
arm64_bin=$2
out=$3

command -v jq >/dev/null 2>&1 || die "jq is not installed"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is not installed"

epoch=${SOURCE_DATE_EPOCH-}
[ -n "$epoch" ] || die "SOURCE_DATE_EPOCH is not set"
# At most 11 octal digits fit a tar mtime field: 8^11 - 1 seconds.
[[ $epoch =~ ^(0|[1-9][0-9]{0,10})$ ]] && [ "$epoch" -le 8589934591 ] ||
  die "SOURCE_DATE_EPOCH '$epoch' is not a whole number of seconds"

for bin in "$amd64_bin" "$arm64_bin"; do
  [ -f "$bin" ] && [ ! -L "$bin" ] || die "$bin is not a regular file"
  [ -s "$bin" ] || die "$bin is empty"
done

if [ -e "$out" ] || [ -L "$out" ]; then
  [ -d "$out" ] && [ ! -L "$out" ] || die "$out exists and is not a directory"
  [ -z "$(ls -A "$out")" ] || die "$out is not empty"
fi
mkdir -p "$out/blobs/sha256"
blobs="$out/blobs/sha256"

# Prints the size in bytes of file $1.
size_of() {
  local n
  n=$(wc -c <"$1")
  echo $((n))
}

# Moves file $1 into the blob store under its digest and prints the digest.
store() {
  local hex
  hex=$(sha256sum <"$1")
  hex=${hex%% *}
  [[ $hex =~ ^[0-9a-f]{64}$ ]] || die "sha256sum printed '$hex'"
  mv "$1" "$blobs/$hex"
  echo "sha256:$hex"
}

# Writes text $1 then NUL bytes up to $2 bytes in all.
field() {
  [ "${#1}" -le "$2" ] || die "tar field '$1' is longer than $2 bytes"
  printf '%s' "$1"
  head -c $(($2 - ${#1})) /dev/zero
}

# Writes one ustar header: name $1, mode $2 (octal), size $3, typeflag $4.
# Every field is fixed here: owner and group 0 with no names, mtime from
# SOURCE_DATE_EPOCH, no link, no device numbers, no prefix.
tar_header() {
  local name=$1 mode size mtime ids text sum i
  mode=$(printf '%07o' "$((8#$2))")
  size=$(printf '%011o' "$3")
  mtime=$(printf '%011o' "$epoch")
  ids=0000000
  # The checksum is the byte sum of the header with its own field as eight
  # spaces. Every other byte is a NUL or a character of these texts.
  text="$name$mode$ids$ids$size$mtime$4ustar00"
  sum=$((8 * 32))
  for ((i = 0; i < ${#text}; i++)); do
    sum=$((sum + $(printf '%d' "'${text:i:1}")))
  done
  field "$name" 100
  field "$mode" 8
  field "$ids" 8
  field "$ids" 8
  field "$size" 12
  field "$mtime" 12
  printf '%06o' "$sum"
  printf '\0 '
  printf '%s' "$4"
  field "" 100 # linkname
  printf 'ustar\0'
  printf '00'
  field "" 32 # uname
  field "" 32 # gname
  field "" 8  # devmajor
  field "" 8  # devminor
  field "" 155 # prefix
  field "" 12
}

# Writes the layer tar for binary $1 to file $2.
layer_tar() {
  local size
  size=$(size_of "$1")
  {
    tar_header oskac 0755 "$size" 0
    cat "$1"
    head -c $(((512 - size % 512) % 512)) /dev/zero
    tar_header tmp/ 1777 0 5
    head -c 1024 /dev/zero
  } >"$2"
}

created=$(jq -nr --argjson e "$epoch" '$e | todate')

# Builds the image for binary $1 and OCI architecture $2, and prints its
# manifest descriptor with platform, as compact JSON.
image() {
  local layer layer_size config config_size manifest size
  layer_tar "$1" "$out/layer.tmp"
  layer_size=$(size_of "$out/layer.tmp")
  layer=$(store "$out/layer.tmp")

  jq -cjn --arg created "$created" --arg arch "$2" --arg layer "$layer" '{
    created: $created,
    architecture: $arch,
    os: "linux",
    config: {User: "65532:65532", Entrypoint: ["/oskac"]},
    rootfs: {type: "layers", diff_ids: [$layer]}
  }' >"$out/config.tmp"
  config_size=$(size_of "$out/config.tmp")
  config=$(store "$out/config.tmp")

  jq -cjn --arg config "$config" --argjson config_size "$config_size" \
    --arg layer "$layer" --argjson layer_size "$layer_size" '{
    schemaVersion: 2,
    mediaType: "application/vnd.oci.image.manifest.v1+json",
    config: {
      mediaType: "application/vnd.oci.image.config.v1+json",
      digest: $config,
      size: $config_size
    },
    layers: [{
      mediaType: "application/vnd.oci.image.layer.v1.tar",
      digest: $layer,
      size: $layer_size
    }]
  }' >"$out/manifest.tmp"
  size=$(size_of "$out/manifest.tmp")
  manifest=$(store "$out/manifest.tmp")

  jq -cjn --arg digest "$manifest" --argjson size "$size" --arg arch "$2" '{
    mediaType: "application/vnd.oci.image.manifest.v1+json",
    digest: $digest,
    size: $size,
    platform: {architecture: $arch, os: "linux"}
  }'
}

amd64=$(image "$amd64_bin" amd64)
arm64=$(image "$arm64_bin" arm64)

jq -cjn --argjson amd64 "$amd64" --argjson arm64 "$arm64" '{
  schemaVersion: 2,
  mediaType: "application/vnd.oci.image.index.v1+json",
  manifests: [$amd64, $arm64]
}' >"$out/index.tmp"
index_size=$(size_of "$out/index.tmp")
index=$(store "$out/index.tmp")

jq -cjn --arg digest "$index" --argjson size "$index_size" '{
  schemaVersion: 2,
  mediaType: "application/vnd.oci.image.index.v1+json",
  manifests: [{
    mediaType: "application/vnd.oci.image.index.v1+json",
    digest: $digest,
    size: $size
  }]
}' >"$out/index.json"
printf '{"imageLayoutVersion":"1.0.0"}' >"$out/oci-layout"

echo "$index"
