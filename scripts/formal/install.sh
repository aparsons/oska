#!/usr/bin/env bash
# Installs the formal step's pinned tools (scripts/formal/pins.sh) into DIR:
# Lean, with its lake, as DIR/lean/bin, and TLC as DIR/tla2tools.jar. Linux
# x86_64 and aarch64 only; every download is checked against its pinned
# checksum and refused on a mismatch. TLC also needs a Java 11 or newer on
# PATH, which this does not install.
#
#   scripts/formal/install.sh [DIR]   default: $XDG_CACHE_HOME/oska/formal
#                                     ($XDG_CACHE_HOME unset: ~/.cache)
#
# run.sh finds both there with no setup. CI puts DIR/lean/bin on PATH and
# names the jar in OSKA_TLA2TOOLS.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/formal/pins.sh
. "$here/pins.sh"

[ $# -le 1 ] || {
  echo "usage: install.sh [DIR]" >&2
  exit 2
}
dir=${1:-$formal_cache}

die() {
  printf 'formal/install.sh: %s\n' "$1" >&2
  exit 1
}

# The toolchain file and the pin must name the same Lean.
# OSKA_FORMAL_DIR names another directory in place of formal/, for the tests.
formal_toolchain_matches "${OSKA_FORMAL_DIR:-$here/../../formal}/lean/lean-toolchain" ||
  die "refusing to install a Lean the project does not name"

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) asset=lean-$lean_version-linux.tar.zst want=$lean_sha256_x86_64 ;;
  Linux/aarch64 | Linux/arm64) asset=lean-$lean_version-linux_aarch64.tar.zst want=$lean_sha256_aarch64 ;;
  *) die "no pinned Lean for $(uname -s)/$(uname -m)" ;;
esac
command -v curl >/dev/null 2>&1 || die "curl not installed"
command -v zstd >/dev/null 2>&1 || die "zstd not installed (the Lean release is a .tar.zst)"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() { # URL FILE
  curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$2" "$1" || die "download failed: $1"
}

fetch "https://github.com/leanprover/lean4/releases/download/v$lean_version/$asset" "$work/$asset"
got=$(sha256sum "$work/$asset" | cut -d' ' -f1)
[ "$got" = "$want" ] || die "$asset does not match its pinned SHA-256 (got $got)"
mkdir "$work/lean"
tar -I zstd -xf "$work/$asset" -C "$work/lean"
# The archive holds one directory, lean-VERSION-linux[_aarch64].
top=$(find "$work/lean" -mindepth 1 -maxdepth 1 -type d)
[ -n "$top" ] && [ "$(printf '%s\n' "$top" | wc -l)" -eq 1 ] && [ -x "$top/bin/lean" ] && [ -x "$top/bin/lake" ] ||
  die "$asset does not hold one directory with bin/lean and bin/lake"

fetch "https://github.com/tlaplus/tlaplus/releases/download/v$tla_version/tla2tools.jar" "$work/tla2tools.jar"
got=$(sha256sum "$work/tla2tools.jar" | cut -d' ' -f1)
[ "$got" = "$tla_sha256" ] || die "tla2tools.jar does not match its pinned SHA-256 (got $got)"
got=$(sha1sum "$work/tla2tools.jar" | cut -d' ' -f1)
[ "$got" = "$tla_sha1" ] || die "tla2tools.jar does not match its pinned SHA-1 (got $got)"

mkdir -p "$dir"
rm -rf "$dir/lean"
mv "$top" "$dir/lean"
mv -f "$work/tla2tools.jar" "$dir/tla2tools.jar"
"$dir/lean/bin/lean" --version
echo "TLC $tla_version: $dir/tla2tools.jar"
