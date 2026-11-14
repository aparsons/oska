#!/usr/bin/env bash
# Every check a change must pass, in one command. CI's ci.yml runs this
# script, and .githooks/pre-push runs it before every push, so a green run
# here means a green CI test job.
#
#   scripts/check.sh
#
# Runs, stopping at the first failure:
#   1. repo rules that are mechanical: no crate from outside this repo,
#      `unsafe` only in crates/oska-sys, std.rt intrinsics only in
#      std.rt's own source, workflows use only official
#      actions plus the SHA-pinned rust-toolchain, and every workflow job
#      has a `timeout-minutes` of 1 to 60;
#   2. cargo fmt, clippy with -D warnings, and cargo test, which runs in a
#      private temporary directory (scripts/temp-leak/run.sh) and fails,
#      naming the paths, if a test leaves anything in it;
#   3. the installer, release script, unsafe guard, intrinsic guard,
#      workflow guard, temp-leak wrapper and rust-stable tests;
#   4. shellcheck on the shell scripts, when shellcheck is installed.

set -euo pipefail

cd "$(dirname "$0")/.."

# A git hook (such as .githooks/pre-push) exports GIT_DIR and related
# variables. Scripts that run git on their own scratch repositories must not
# inherit them, or `git init` re-initialises this repository instead (it once
# set core.bare=true here).
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_COMMON_DIR GIT_PREFIX

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

step "unsafe only in oska-sys"
# The workspace forbids `unsafe_code`, and oska-sys alone may use it
# (ADR 0009). The guard compiles every other crate with `unsafe_code`
# forbidden on rustc's command line, refuses cargo configs and workflow
# flags that could lower lints, and scans source the build skips.
scripts/unsafe-guard/guard.sh

step "std.rt intrinsics only in std.rt"
# System-call and memory intrinsics (`rt_sys_*`, `rt_mem_*`) and `use std.rt`
# may appear only in the toolchain's own std.rt source (ADR 0013).
scripts/intrinsic-guard/guard.sh

step "only official actions, rust-toolchain pinned to a commit"
toolchain_pin='dtolnay/rust-toolchain@[0-9a-f]{40}'
bad_uses=$(grep -nE '^[[:space:]]*-?[[:space:]]*uses:' .github/workflows/*.yml |
  grep -vE "uses:[[:space:]]*(actions/[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+|$toolchain_pin)([[:space:]]|$)" || true)
if [ -n "$bad_uses" ]; then
  printf '%s\n' "$bad_uses" >&2
  fail "workflows may use only actions/* and dtolnay/rust-toolchain pinned to a full commit SHA"
fi

step "every workflow job has a timeout"
# GitHub lets a job run 360 minutes unless it says otherwise. The guard fails
# a job with no `timeout-minutes`, a non-literal value, or one above 60.
scripts/workflow-guard/guard.sh

step "cargo fmt"
cargo fmt --all --check

step "cargo clippy"
cargo clippy --all-targets -- -D warnings

step "cargo test (private temporary directory)"
# A test that leaves a file or directory in the temporary directory fails the
# check here, naming it, instead of slowly filling the shared /tmp.
scripts/temp-leak/run.sh cargo test --all

step "installer tests"
bash scripts/install/test.sh

step "release script tests"
bash scripts/release/test.sh

step "unsafe guard tests"
bash scripts/unsafe-guard/test.sh

step "intrinsic guard tests"
bash scripts/intrinsic-guard/test.sh

step "workflow guard tests"
bash scripts/workflow-guard/test.sh

step "temp-leak wrapper tests"
bash scripts/temp-leak/test.sh

step "rust-stable tests"
bash scripts/rust-stable/test.sh

step "shellcheck"
if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -S warning scripts/*.sh scripts/install/*.sh scripts/release/*.sh scripts/unsafe-guard/*.sh scripts/intrinsic-guard/*.sh scripts/workflow-guard/*.sh scripts/temp-leak/*.sh scripts/rust-stable/*.sh site/agent-setup/*.sh .githooks/pre-push
else
  echo "shellcheck not installed; skipped (CI runs it)"
fi

printf '\nAll checks passed.\n'
