#!/usr/bin/env bash
# Moves the pinned Rust to a new stable version.
#
#   scripts/bump-rust.sh 1.101.0
#
# 1. Sets `channel` in rust-toolchain.toml.
# 2. Regenerates `escaped_table` and `lowercase_table` in
#    crates/oska-check/src/std/rt.osk from that Rust's Unicode
#    (scripts/bump-rust/tables.rs, built with `rustc +<version>`).
# 3. Runs scripts/check.sh. If a newer Clippy objects to existing code, the
#    script stops with check.sh's output and a person fixes the code.
#
# Running it with the version already pinned leaves rt.osk unchanged.
# BUMP_RUST_SKIP_CHECK=1 stops after step 2 (for checking that property).

set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$#" -ne 1 ] || ! [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/bump-rust.sh X.Y.Z" >&2
  exit 2
fi
version=$1

if ! command -v rustup >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/rustup" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi
command -v rustup >/dev/null 2>&1 || {
  echo "bump-rust.sh: rustup is required" >&2
  exit 1
}

rt=crates/oska-check/src/std/rt.osk

rustup toolchain install "$version" --profile minimal -c rustfmt -c clippy

sed -i -E "s/^channel = \".*\"/channel = \"$version\"/" rust-toolchain.toml
grep -qx "channel = \"$version\"" rust-toolchain.toml || {
  echo "bump-rust.sh: could not set channel in rust-toolchain.toml" >&2
  exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rustc "+$version" --edition 2021 -O -o "$work/tables" scripts/bump-rust/tables.rs

# Replaces the string on the line after `fn <name>() -> String {` in rt.osk.
splice() {
  local name=$1 kind=$2
  "$work/tables" "$kind" >"$work/$kind"
  [ -s "$work/$kind" ] || {
    echo "bump-rust.sh: no $kind table generated" >&2
    exit 1
  }
  awk -v head="fn $name() -> String {" -v file="$work/$kind" '
    found == 1 {
      getline table < file
      print "    \"" table "\""
      found = 2
      next
    }
    $0 == head { found = 1 }
    { print }
    END { if (found != 2) exit 1 }
  ' "$rt" >"$work/rt.osk" || {
    echo "bump-rust.sh: fn $name not found in $rt" >&2
    exit 1
  }
  cat "$work/rt.osk" >"$rt"
}

splice escaped_table escaped
splice lowercase_table lowercase

if [ "${BUMP_RUST_SKIP_CHECK:-}" = 1 ]; then
  exit 0
fi
scripts/check.sh
