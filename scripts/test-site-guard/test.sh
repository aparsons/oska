#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for guard.sh: each case builds a small git tree, and expects the guard
# to refuse it, or (for a clean tree) to pass.
#
#   scripts/test-site-guard/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
passed=0
failures=0

tree() {
  local dir
  dir=$(mktemp -d "$work/tree.XXXXXX")
  git -C "$dir" init -q
  mkdir -p "$dir/crates/oskac" "$dir/.github/workflows"
  printf '[package]\nname = "oskac"\n\n[features]\n# test builds only\ntest-site = []\n\n[lints]\n' >"$dir/crates/oskac/Cargo.toml"
  printf 'jobs:\n  a:\n    steps:\n      - run: cargo build --profile dist --locked -p oskac\n      - run: cargo check --workspace --all-features\n' >"$dir/.github/workflows/ci.yml"
  printf '%s\n' "$dir"
}

expect_pass() {
  local name=$1 dir=$2 out
  git -C "$dir" add -A
  if out=$("$guard" "$dir" 2>&1); then
    passed=$((passed + 1))
  else
    printf 'FAIL %s: the guard refused a clean tree\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  fi
}

expect_refused() {
  local name=$1 dir=$2 what=$3 out
  shift 3
  git -C "$dir" add -A
  if out=$("$guard" "$dir" "$@" 2>&1); then
    printf 'FAIL %s: the guard passed a bad tree\n' "$name" >&2
    failures=$((failures + 1))
  elif [[ $out != *"$what"* ]]; then
    printf 'FAIL %s: the refusal does not say "%s":\n%s\n' "$name" "$what" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

dir=$(tree)
expect_pass "a clean tree" "$dir"

dir=$(tree)
printf '[package]\nname = "oskac"\n\n[features]\ndefault = ["test-site"]\ntest-site = []\n' >"$dir/crates/oskac/Cargo.toml"
expect_refused "a default feature" "$dir" "default feature"

dir=$(tree)
printf '[package]\nname = "oskac"\n\n[features]\nfull = ["test-site"]\ntest-site = []\n' >"$dir/crates/oskac/Cargo.toml"
expect_refused "another feature enabling it" "$dir" "enables"

dir=$(tree)
printf '[package]\nname = "oskac"\n' >"$dir/crates/oskac/Cargo.toml"
expect_refused "the feature missing" "$dir" "no \`test-site = []\`"

dir=$(tree)
printf '      - run: cargo build --release --all-features -p oskac\n' >>"$dir/.github/workflows/ci.yml"
expect_refused "a release build with all features" "$dir" "all features"

dir=$(tree)
printf '      - run: cargo build -p oskac --features test-site\n' >>"$dir/.github/workflows/ci.yml"
expect_refused "a build with the feature" "$dir" "test-site"

dir=$(tree)
printf 'FROM rust\nRUN cargo install --path crates/oskac --features test-site\n' >"$dir/Dockerfile"
expect_refused "an install with the feature" "$dir" "Dockerfile"

dir=$(tree)
printf 'x OSKAC_TEST_SITE_PORT x' >"$work/test-build"
printf 'x plain x' >"$work/release-build"
expect_refused "a binary with the marker" "$dir" "test-site" "$work/test-build"
expect_pass "a clean tree" "$dir"
if "$guard" "$dir" "$work/release-build" >/dev/null 2>&1; then
  passed=$((passed + 1))
else
  printf 'FAIL a release binary: the guard refused it\n' >&2
  failures=$((failures + 1))
fi

dir=$(tree)
git -C "$dir" add -A
if out=$("$guard" --require-binary "$dir" 2>&1); then
  printf 'FAIL no binary where one is required: the guard passed\n' >&2
  failures=$((failures + 1))
elif [[ $out == *"requires one"* ]]; then
  passed=$((passed + 1))
else
  printf 'FAIL no binary where one is required: wrong refusal:\n%s\n' "$out" >&2
  failures=$((failures + 1))
fi
if "$guard" --require-binary "$dir" "$work/release-build" >/dev/null 2>&1; then
  passed=$((passed + 1))
else
  printf 'FAIL a required binary that is clean: the guard refused it\n' >&2
  failures=$((failures + 1))
fi
# A build command in a crate's own directory is checked too.
dir=$(tree)
mkdir -p "$dir/crates/oskac/ci"
printf 'cargo build --release --all-features\n' >"$dir/crates/oskac/ci/build.sh"
expect_refused "a build under crates" "$dir" "all features"

printf 'test-site-guard tests: %s passed, %s failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
