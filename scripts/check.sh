#!/usr/bin/env bash
# Every check a change must pass, in one command. CI's ci.yml runs this
# script, and .githooks/pre-push runs it before every push, so a green run
# here means a green CI test job.
#
#   scripts/check.sh
#
# Runs, stopping at the first failure:
#   1. repo rules that are mechanical: no crate from outside this repo, and
#      workflows use only official actions plus the SHA-pinned rust-toolchain;
#   2. cargo fmt, clippy with -D warnings, and cargo test;
#   3. the installer and release script tests;
#   4. shellcheck on the shell scripts, when shellcheck is installed.

set -euo pipefail

cd "$(dirname "$0")/.."

# Rust may be installed for interactive shells only.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi

step() { printf '\n== %s\n' "$1"; }
fail() {
  printf 'check.sh: %s\n' "$1" >&2
  exit 1
}

step "no external crates"
# Every package in the lock file comes from this workspace: a registry or git
# crate gets a `source =` line.
if grep -n '^source = ' Cargo.lock; then
  fail "Cargo.lock has a crate from outside this repo (above); dependencies must be path dependencies"
fi

step "only official actions, rust-toolchain pinned to a commit"
toolchain_pin='dtolnay/rust-toolchain@[0-9a-f]{40}'
bad_uses=$(grep -nE '^[[:space:]]*-?[[:space:]]*uses:' .github/workflows/*.yml |
  grep -vE "uses:[[:space:]]*(actions/[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+|$toolchain_pin)([[:space:]]|$)" || true)
if [ -n "$bad_uses" ]; then
  printf '%s\n' "$bad_uses" >&2
  fail "workflows may use only actions/* and dtolnay/rust-toolchain pinned to a full commit SHA"
fi

step "cargo fmt"
cargo fmt --all --check

step "cargo clippy"
cargo clippy --all-targets -- -D warnings

step "cargo test"
cargo test --all

step "installer tests"
bash scripts/install/test.sh

step "release script tests"
bash scripts/release/test.sh

step "shellcheck"
if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -S warning scripts/*.sh scripts/install/*.sh scripts/release/*.sh site/agent-setup/*.sh .githooks/pre-push
else
  echo "shellcheck not installed; skipped (CI runs it)"
fi

printf '\nAll checks passed.\n'
