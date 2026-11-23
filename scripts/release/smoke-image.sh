#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Smoke-test a musl build of oskac on an empty root, as the release run does
# on each Linux processor before anything is published (ADR 0022):
#
#   scripts/release/smoke-image.sh <musl-binary> <image-layout> <example.osk>
#
# <musl-binary> is this processor's oskac-linux-<arch>-musl, <image-layout>
# the OCI layout assemble.sh wrote with image.sh. In two places:
#   - in `alpine` (musl, no glibc), with the binary mounted, and
#   - in the image itself, loaded into Docker for this host's processor,
#     which holds nothing but /oskac and /tmp and runs as user 65532,
# it runs `oskac --version`, `oskac run <example>` (which builds natively in
# /tmp), and `oskac build <example>`, then runs the executable it built in
# the same place. Each must print exactly `Hello from Oska!`, as
# examples/hello.osk does, and --version what the binary prints on the host.
# Then `oskac test` on a one-test file this script writes, which also builds
# natively in /tmp: its JSON report must say the one test passed.
#
# Needs Docker and skopeo. Any failure exits non-zero.

set -euo pipefail
shopt -s inherit_errexit

die() {
  echo "smoke-image.sh: $*" >&2
  exit 1
}

[ $# -eq 3 ] || die "usage: smoke-image.sh <musl-binary> <image-layout> <example.osk>"
binary=$1
layout=$2
example=$3

# alpine 3.22, pinned by the digest of its multi-processor index.
ALPINE=alpine@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f
EXPECTED="Hello from Oska!"
# `oskac test --format=json smoke_test.osk`, one line, with the test's
# `duration_ms` (which varies from run to run) taken out by expect_test.
EXPECTED_TEST='{"tests":[{"name":"oskac test runs here","status":"passed","failure":null}],"passed":1,"failed":0,"skipped":0,"diagnostics":[]}'

command -v docker >/dev/null 2>&1 || die "docker is not installed"
command -v skopeo >/dev/null 2>&1 || die "skopeo is not installed"
[ -f "$binary" ] || die "no binary $binary"
[ -f "$layout/index.json" ] || die "no OCI layout at $layout"
[ -f "$example" ] || die "no example $example"

case $(uname -m) in
  x86_64) arch=amd64 ;;
  aarch64) arch=arm64 ;;
  *) die "no image for processor $(uname -m)" ;;
esac

work=$(mktemp -d "${TMPDIR:-/tmp}/oska-smoke-image.XXXXXXXX")
image=oska-smoke-image:$arch-$$
trap 'rm -rf "$work"; docker image rm -f "$image" >/dev/null 2>&1 || true' EXIT
mkdir "$work/bin" "$work/w"
cp "$binary" "$work/bin/oskac"
chmod 755 "$work/bin/oskac"
cp "$example" "$work/w/example.osk"
cat >"$work/w/smoke_test.osk" <<'OSK'
test "oskac test runs here" {
    t.assert_eq(1 + 1, 2)
}
OSK
# The image runs as user 65532, which must write the executable it builds.
chmod 777 "$work/w"

version=$("$work/bin/oskac" --version)
[[ $version =~ ^oskac\ [0-9] ]] || die "the binary prints '$version' for --version"

# Checks that "$@" prints exactly $1 (then runs it with the rest).
expect() {
  local want=$1 got
  shift
  got=$("$@") || die "failed: $*"
  [ "$got" = "$want" ] || die "$* printed '$got', not '$want'"
  echo "ok: $* printed '$got'"
}

# Checks that `oskac test --format=json` ("$@") reports exactly
# $EXPECTED_TEST once its one `"duration_ms":<n>,` is taken out.
expect_test() {
  local got masked
  got=$("$@") || die "failed: $*"
  masked=$(sed -E 's/"duration_ms":[0-9]+,//' <<<"$got")
  [ "$masked" = "$EXPECTED_TEST" ] || die "$* printed '$got', not '$EXPECTED_TEST' (with a duration_ms)"
  echo "ok: $* printed '$got'"
}

# In alpine: musl, no glibc, the binary mounted at /b/oskac.
in_alpine=(docker run --rm --network none -v "$work/bin:/b:ro" -v "$work/w:/w" -w /w "$ALPINE")
expect "$version" "${in_alpine[@]}" /b/oskac --version
expect "$EXPECTED" "${in_alpine[@]}" /b/oskac run --no-cache example.osk
"${in_alpine[@]}" /b/oskac build --no-cache example.osk -o /w/built-in-alpine ||
  die "oskac build failed in alpine"
expect "$EXPECTED" "${in_alpine[@]}" /w/built-in-alpine
expect_test "${in_alpine[@]}" /b/oskac test --no-cache --format=json smoke_test.osk

# In the image: this processor's entry of the index, as `docker pull` of
# the published tag would choose it.
skopeo copy "oci:$layout" "docker-daemon:$image" >&2 ||
  die "cannot load the image for $arch into Docker"
[ "$(docker image inspect -f '{{.Os}}/{{.Architecture}} {{.Config.User}}' "$image")" = \
  "linux/$arch 65532:65532" ] || die "the loaded image is not linux/$arch run as 65532:65532"
in_image=(docker run --rm --network none -v "$work/w:/w" -w /w)
expect "$version" "${in_image[@]}" "$image" --version
expect "$EXPECTED" "${in_image[@]}" "$image" run --no-cache example.osk
"${in_image[@]}" "$image" build --no-cache example.osk -o /w/built-in-image ||
  die "oskac build failed in the image"
expect "$EXPECTED" "${in_image[@]}" --entrypoint /w/built-in-image "$image"
expect_test "${in_image[@]}" "$image" test --no-cache --format=json smoke_test.osk

echo "ok: $version runs, builds and runs $example, and runs a test, in alpine and in the image on linux/$arch"
