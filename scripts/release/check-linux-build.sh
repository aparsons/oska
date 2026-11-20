#!/usr/bin/env bash
# Check that a Linux oskac build runs where the install guide says it runs.
#
#   scripts/release/check-linux-build.sh gnu <binary>
#   scripts/release/check-linux-build.sh musl <binary>
#
# gnu, the glibc build (oskac-linux-<arch>-gnu): the newest glibc symbol
# version it needs is no newer than GLIBC_2.35, the glibc that install.sh
# requires and the install guide promises. It must need some glibc
# version: a glibc build that names none is not what the release built.
#
# musl, the musl build (oskac-linux-<arch>-musl): it is static, so it runs
# with no C library and in an empty root (ADR 0022). It has no PT_INTERP
# (it asks for no dynamic loader) and no DT_NEEDED (it needs no shared
# library). It may be static-pie, with a dynamic section of its own.
#
# Fails closed: a file that readelf or objdump cannot read is refused.
# Needs GNU binutils (readelf, objdump), as on the Ubuntu runners.

set -euo pipefail

MAX_GLIBC=2.35

die() {
  echo "check-linux-build.sh: $*" >&2
  exit 1
}

[ $# -eq 2 ] || die "usage: check-linux-build.sh gnu|musl <binary>"
build=$1
bin=$2

[ -f "$bin" ] || die "no file $bin"

case $build in
  gnu)
    symbols=$(objdump -T "$bin") || die "objdump cannot read the dynamic symbols of $bin"
    # Here-strings, not `printf | grep`: under pipefail a grep that stops
    # reading early fails the printf, and the check with it.
    versions=$(grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' <<<"$symbols" || true)
    needs=$(sort -uV <<<"$versions" | tail -n 1)
    [ -n "$needs" ] || die "$bin needs no glibc symbol version; it is not a glibc build"
    echo "$bin needs $needs"
    if ! sort -V -C <<<"$needs"$'\n'"GLIBC_$MAX_GLIBC"; then
      die "$bin needs $needs, newer than the documented glibc $MAX_GLIBC"
    fi
    ;;
  musl)
    readelf -hW "$bin" >/dev/null 2>&1 || die "$bin is not an ELF file readelf can read"
    headers=$(readelf -lW "$bin") || die "readelf cannot read the program headers of $bin"
    if grep -qE '^[[:space:]]+INTERP[[:space:]]' <<<"$headers"; then
      die "$bin has a PT_INTERP: it asks for a dynamic loader, so it is not static"
    fi
    dynamic=$(readelf -dW "$bin") || die "readelf cannot read the dynamic section of $bin"
    needed=$(grep -F '(NEEDED)' <<<"$dynamic" || true)
    if [ -n "$needed" ]; then
      die "$bin has a DT_NEEDED: it needs a shared library, so it is not static: $needed"
    fi
    echo "$bin is static: no PT_INTERP, no DT_NEEDED"
    ;;
  *) die "the build is '$build', not gnu or musl" ;;
esac
