#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for check-graph.sh, the crate-graph check of the crate-boundaries
# skill (installed in Oska as scripts/crate-graph/check.sh).
#
#   test.sh [path-to-check-script]
#
# With no argument it tests check-graph.sh or check.sh next to this file.
# Each case builds a small workspace in a temporary directory, so the tests
# do not depend on the real crates.
# Exit 0 when every case passes; exit 1 on any failure.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
if [ $# -gt 0 ]; then
  check=$1
elif [ -f "$here/check-graph.sh" ]; then
  check="$here/check-graph.sh"
else
  check="$here/check.sh"
fi
if [ ! -f "$check" ]; then
  printf 'test.sh: no check script at %s\n' "$check" >&2
  exit 1
fi

tmp=$(mktemp -d)
chmod 700 "$tmp"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0

# crate REPO NAME [DEPENDENCY-LINES]: write REPO/crates/NAME/Cargo.toml.
crate() {
  mkdir -p "$1/crates/$2"
  printf '[package]\nname = "%s"\nversion = "0.1.0"\n\n[dependencies]\n%s\n' "$2" "${3:-}" >"$1/crates/$2/Cargo.toml"
}

# base: a workspace shaped like Oska's, with its allowed file.
base() {
  local r
  r=$(mktemp -d "$tmp/ws.XXXXXX")
  crate "$r" oska-syntax
  crate "$r" oska-sys
  crate "$r" oska-check 'oska-syntax = { path = "../oska-syntax" }'
  crate "$r" oska-eval 'oska-syntax = { path = "../oska-syntax" }'
  crate "$r" oska-native $'oska-check = { path = "../oska-check" }\noska-syntax = { path = "../oska-syntax" }'
  crate "$r" oska-run $'oska-check = { path = "../oska-check" }  # a comment\noska-syntax = { path = "../oska-syntax", version = "0.1" }\noska-sys.path = "../oska-sys"\n\n[dev-dependencies]\noska-native = { path = "../oska-native" }'
  crate "$r" oskac $'oska-check = { path = "../oska-check" }\noska-native = { path = "../oska-native" }\noska-run = { path = "../oska-run" }\noska-syntax = { path = "../oska-syntax" }\n\n[dependencies.oska-sys]\npath = "../oska-sys"\n\n[lints]\nworkspace = true'
  cat >"$r/allowed.txt" <<'TXT'
# The allowed edges.
oska-syntax:
oska-sys:
oska-check: oska-syntax
oska-eval: oska-syntax
oska-native: oska-check oska-syntax
oska-run: oska-check oska-native oska-syntax oska-sys
oskac: oska-check oska-native oska-run oska-syntax oska-sys
TXT
  printf '%s\n' "$r"
}

# expect NAME WANT-EXIT WANT-TEXT REPO [ARGS...]
expect() {
  local name=$1 want=$2 text=$3 r=$4 out status=0
  shift 4
  out=$(bash "$check" --repo "$r" --allowed "$r/allowed.txt" "$@" 2>&1) || status=$?
  if [ "$status" -ne "$want" ]; then
    failed=$((failed + 1))
    printf 'FAIL [%s] exit %s, want %s; output:\n%s\n' "$name" "$status" "$want" "$out" >&2
  elif [[ $out != *"$text"* ]]; then
    failed=$((failed + 1))
    printf 'FAIL [%s] output lacks "%s":\n%s\n' "$name" "$text" "$out" >&2
  else
    passed=$((passed + 1))
  fi
}

# --- the allowed graph passes ---
r=$(base)
expect 'allowed graph passes' 0 '7 crates, every edge allowed, no cycle' "$r"
expect 'show prints the graph' 0 'oska-run: oska-check oska-native oska-syntax oska-sys' "$r" --show

# --- a new edge fails ---
r=$(base)
printf 'oska-run = { path = "../oska-run" }\n' >>"$r/crates/oska-eval/Cargo.toml"
expect 'new edge fails' 1 'new edge oska-eval -> oska-run' "$r"

r=$(base)
printf 'oska-sys = { path = "../oska-sys" }\n' >>"$r/crates/oska-check/Cargo.toml"
expect 'new edge to oska-sys needs a reason' 1 'widens access to system calls' "$r"

r=$(base)
printf '\n[dev-dependencies]\noska-run = { path = "../oska-run" }\n' >>"$r/crates/oska-native/Cargo.toml"
expect 'new dev-dependency edge fails' 1 'new edge oska-native -> oska-run' "$r"

r=$(base)
printf "\n[target.'cfg(unix)'.dependencies]\noska-sys = { path = \"../oska-sys\" }\n" >>"$r/crates/oska-eval/Cargo.toml"
expect 'new target-specific edge fails' 1 'new edge oska-eval -> oska-sys' "$r"

# --- a cycle fails ---
r=$(base)
printf 'oska-check = { path = "../oska-check" }\n' >>"$r/crates/oska-syntax/Cargo.toml"
sed -i 's/^oska-syntax:$/oska-syntax: oska-check/' "$r/allowed.txt"
expect 'cycle fails' 1 'cycle: ' "$r"

# --- missing crates fail ---
r=$(base)
rm -r "$r/crates/oska-eval"
expect 'crate missing from crates/ fails' 1 'oska-eval in' "$r"

r=$(base)
crate "$r" oska-new
expect 'crate missing from allowed fails' 1 'the crate oska-new is not in' "$r"

# --- unreadable input fails closed ---
r=$(base)
rm "$r/crates/oska-sys/Cargo.toml"
ln -s "$r/nowhere" "$r/crates/oska-sys/Cargo.toml"
expect 'unreadable Cargo.toml fails closed' 2 'cannot read' "$r"

r=$(base)
rm "$r/allowed.txt"
expect 'missing allowed file fails closed' 2 'cannot read the allowed-edges file' "$r"

r=$(base)
printf 'oska-run -> oska-sys\n' >>"$r/allowed.txt"
expect 'bad allowed line fails closed' 2 'cannot read the line' "$r"

r=$(base)
printf 'serde = "1.0"\n' >>"$r/crates/oska-check/Cargo.toml"
expect 'registry dependency fails closed' 2 'cannot read the dependency line' "$r"

r=$(base)
printf 'oska-sys = { workspace = true }\n' >>"$r/crates/oska-run/Cargo.toml"
expect 'workspace dependency fails closed' 2 'workspace = true' "$r"

r=$(base)
printf 'oska-sys = {\n  path = "../oska-sys"\n}\n' >>"$r/crates/oska-eval/Cargo.toml"
expect 'multi-line inline table fails closed' 2 'cannot read the dependency line' "$r"

r=$(base)
printf 'other = { git = "https://example.invalid/x" }\n' >>"$r/crates/oska-eval/Cargo.toml"
expect 'dependency without a path fails closed' 2 'has no path' "$r"

r=$(base)
mkdir -p "$r/vendor/x"
printf 'x = { path = "../../vendor/x" }\n' >>"$r/crates/oska-eval/Cargo.toml"
expect 'path outside crates/ fails closed' 2 'is not a crate under crates/' "$r"

r=$(base)
sed -i 's/^name = .*//' "$r/crates/oska-sys/Cargo.toml"
expect 'crate without a name fails closed' 2 'no [package] name' "$r"

mkdir "$tmp/empty"
expect 'no crates/ fails closed' 2 'no crates/ directory' "$tmp/empty"

total=$((passed + failed))
if [ "$failed" -ne 0 ]; then
  printf '%s of %s cases failed\n' "$failed" "$total" >&2
  exit 1
fi
printf 'all %s cases passed\n' "$total"
