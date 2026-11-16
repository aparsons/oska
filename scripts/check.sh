#!/usr/bin/env bash
# Every check a change must pass, in one command. CI's ci.yml runs this
# script, and .githooks/pre-push runs it before every push, so a green run
# here means a green CI test job.
#
#   scripts/check.sh           run every step, stopping at the first failure
#   scripts/check.sh --list    print the step names, in order; run nothing
#
# The steps are the `if step "NAME"; then ... fi` blocks below, in the order
# they run. A block's name and command say what it checks; its comment, if
# any, says the rest. When every step passes, the run writes the pass stamp,
# if the tree did not change meanwhile, so the pre-push hook skips a repeat
# run on the same tree.
#
# To add a step, add one block where it should run, and nothing else:
#
#   if step "my thing tests"; then
#     bash scripts/my-thing/test.sh
#   fi
#
# Options after the name tag the step:
#
#   --fast      the step also belongs to the fast tier (every step runs in a
#               full run)
#   --no-slot   the step is cheap and runs without a build slot
#   --cargo     a cargo step: skipped when cargo_skip says so (below)
#
# A step whose tool is missing calls `skip_step "REASON"` instead of failing;
# the run still writes the pass stamp, since running again here cannot do
# better and CI runs the step. `skip_step --no-stamp "REASON"` is for a skip
# the stamp cannot vouch for: the run then writes no stamp.
#
# Outside CI, the --cargo steps skip (as with --no-stamp) when no cargo input
# differs from the merge-base with origin/main; see cargo_skip below. A
# skipped step prints its header and why, and waits for no build slot. CI
# ($CI or $GITHUB_ACTIONS set) always runs them.
#
# Everything a run does happens inside a step block or in `step` and
# `finish` themselves, so --list can never run any of it.
#
# OSKA_BUILD_SLOTS=N (unset by default: no limit) makes the run wait for one
# of N machine-wide build slots before the first step that needs one; see
# scripts/build-slots/acquire.sh.

set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  echo "usage: scripts/check.sh [--list]" >&2
  exit 2
}

mode=run  # run, or list: print the step names and run nothing
tier=full # full, or fast: only steps tagged --fast
while [ $# -gt 0 ]; do
  case $1 in
    --list) mode=list ;;
    *) usage ;;
  esac
  shift
done

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

started=      # set when the first step of a run starts
checked_tree= # the tree being checked, for the pass stamp `finish` writes
have_slot=    # set once the build slot is held; it is held until the run ends
unstamped=    # set by `skip_step --no-stamp`: the run writes no pass stamp
cargo_why=    # unset until cargo_skip decides; then why cargo skips, or "run"

# step NAME [--fast] [--no-slot]: opens a step's block,
# `if step "NAME"; then ... fi`. In a run it prints the step's header and the
# block runs; with --list it prints NAME and the block is skipped. So the list
# is always the steps that run, in order. The first step of a run records the
# tree for the pass stamp; the first step without --no-slot that is not
# skipped waits for a build slot.
step() {
  local name=$1 fast='' slot=1 cargo=''
  shift
  while [ $# -gt 0 ]; do
    case $1 in
      --fast) fast=1 ;;
      --no-slot) slot= ;;
      --cargo) cargo=1 ;;
      *)
        printf 'check.sh: step "%s": unknown option %s\n' "$name" "$1" >&2
        exit 2
        ;;
    esac
    shift
  done
  if [ "$tier" = fast ] && [ -z "$fast" ]; then
    return 1
  fi
  if [ "$mode" = list ]; then
    printf '%s\n' "$name"
    return 1
  fi
  if [ -z "$started" ]; then
    started=1
    checked_tree=$(scripts/pass-stamp/stamp.sh tree 2>/dev/null || true)
  fi
  if [ -n "$cargo" ] && cargo_skip; then
    printf '\n== %s\n' "$name"
    skip_step --no-stamp "$cargo_why"
    return 1
  fi
  if [ -n "$slot" ] && [ -z "$have_slot" ]; then
    have_slot=1
    . scripts/build-slots/acquire.sh
  fi
  printf '\n== %s\n' "$name"
}

# skip_step [--no-stamp] REASON: inside a step's block, says why the step did
# not run. With --no-stamp, the run writes no pass stamp.
skip_step() {
  if [ "$1" = --no-stamp ]; then
    unstamped=1
    shift
  fi
  printf '%s\n' "$1"
}

# cargo_input PATH: whether a change to PATH (from the repository root) can
# change what cargo fmt, clippy or test do. Everything is an input except the
# paths below, which nothing in Cargo.toml, crates/ or the Rust tests reads.
# The Rust tests read far outside crates/: all of docs/ (the site build, the
# guides, the release-key fingerprints, docs/agents/ included), examples/,
# site/, README.md, GLOSSARY.md, SECURITY.md, .github/workflows/pages.yml,
# scripts/install/, and `git ls-files`. scripts/temp-leak/ wraps cargo test,
# and this file runs the cargo steps.
cargo_input() {
  case $1 in
    .github/workflows/pages.yml | scripts/install/* | scripts/temp-leak/* | scripts/check.sh) return 0 ;;
    CLAUDE.md | CODING_STANDARDS.md | LICENSE | .gitignore) return 1 ;;
    .githooks/* | .claude/* | .github/* | scripts/*) return 1 ;;
  esac
  return 0
}

# cargo_skip: whether the cargo steps can skip, decided once per run; when
# they can, cargo_why says why. They run in CI, and whenever this cannot
# tell: no origin/main, no merge-base, or a shallow clone. The tree compared
# is the working tree, untracked files included, as the pass stamp's is.
# `git diff BASE` takes its paths from the index, so a file removed with
# `git rm --cached` counts as deleted though it is still on disk.
cargo_skip() {
  if [ -z "$cargo_why" ]; then
    cargo_why=run
    local base deleted changed untracked path
    if [ -z "${CI:-}${GITHUB_ACTIONS:-}" ] &&
      [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = false ] &&
      base=$(git merge-base HEAD refs/remotes/origin/main 2>/dev/null) &&
      deleted=$(git diff --no-renames --name-only --diff-filter=D "$base" --) &&
      changed=$(git diff --no-renames --name-only "$base" --) &&
      untracked=$(git ls-files --others --exclude-standard) &&
      # Rust tests read the tracked files' names, so a deletion is an input.
      [ -z "$deleted" ]; then
      cargo_why="skipped: no cargo input differs from origin/main (merge-base ${base:0:12}); CI runs it"
      while IFS= read -r path; do
        if [ -n "$path" ] && cargo_input "$path"; then
          cargo_why=run
          break
        fi
      done <<<"$changed
$untracked"
    fi
  fi
  [ "$cargo_why" != run ]
}

fail() {
  printf 'check.sh: %s\n' "$1" >&2
  exit 1
}

# finish: after the last block. In a run, every step passed: stamp the tier
# and the tree checked, if the tree did not change meanwhile and no step was
# skipped with --no-stamp (then there is no stamp at all). The pre-push hook
# honours only a full run's stamp.
finish() {
  if [ "$mode" = list ]; then
    exit 0
  fi
  if [ -n "$unstamped" ]; then
    echo "pass stamp: not written, a step was skipped that the stamp cannot vouch for"
  else
    scripts/pass-stamp/stamp.sh write "$tier" "$checked_tree"
  fi
  printf '\nAll checks passed.\n'
}

if step "no external crates" --no-slot; then
  # Every package in the lock file comes from this workspace: a registry or git
  # crate gets a `source =` line.
  if grep -n '^source = ' Cargo.lock; then
    fail "Cargo.lock has a crate from outside this repo (above); dependencies must be path dependencies"
  fi
fi

if step "unsafe only in oska-sys" --no-slot; then
  # The workspace forbids `unsafe_code`, and oska-sys alone may use it
  # (ADR 0009). The guard compiles every other crate with `unsafe_code`
  # forbidden on rustc's command line, refuses cargo configs and workflow
  # flags that could lower lints, and scans source the build skips.
  scripts/unsafe-guard/guard.sh
fi

if step "std.rt intrinsics only in std.rt" --no-slot; then
  # System-call and memory intrinsics (`rt_sys_*`, `rt_mem_*`) and `use std.rt`
  # may appear only in the toolchain's own std.rt source (ADR 0013).
  scripts/intrinsic-guard/guard.sh
fi

if step "only official actions, rust-toolchain pinned to a commit" --no-slot; then
  toolchain_pin='dtolnay/rust-toolchain@[0-9a-f]{40}'
  bad_uses=$(grep -rnE --include='*.yml' --include='*.yaml' '^[[:space:]]*-?[[:space:]]*uses:' .github/workflows |
    grep -vE "uses:[[:space:]]*(actions/[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+|$toolchain_pin)([[:space:]]|$)" || true)
  if [ -n "$bad_uses" ]; then
    printf '%s\n' "$bad_uses" >&2
    fail "workflows may use only actions/* and dtolnay/rust-toolchain pinned to a full commit SHA"
  fi
fi

if step "every workflow job has a timeout" --no-slot; then
  # GitHub lets a job run 360 minutes unless it says otherwise. The guard fails
  # a job with no `timeout-minutes`, a non-literal value, or one above 60.
  scripts/workflow-guard/guard.sh
fi

if step "cargo fmt" --fast --cargo; then
  cargo fmt --all --check
fi

if step "cargo clippy" --fast --cargo; then
  cargo clippy --all-targets -- -D warnings
fi

if step "cargo test (private temporary directory)" --cargo; then
  # A test that leaves a file or directory in the temporary directory fails the
  # check here, naming it, instead of slowly filling the shared /tmp.
  scripts/temp-leak/run.sh cargo test --all
fi

if step "installer tests"; then
  bash scripts/install/test.sh
fi

if step "release script tests"; then
  bash scripts/release/test.sh
fi

if step "unsafe guard tests"; then
  bash scripts/unsafe-guard/test.sh
fi

if step "intrinsic guard tests"; then
  bash scripts/intrinsic-guard/test.sh
fi

if step "kill guard tests"; then
  # The Claude Code hook (.claude/settings.json) that refuses pkill -f, killall
  # and other kills by pattern, name or session.
  bash scripts/kill-guard/test.sh
fi

if step "workflow guard tests"; then
  bash scripts/workflow-guard/test.sh
fi

if step "temp-leak wrapper tests"; then
  bash scripts/temp-leak/test.sh
fi

if step "rust-stable tests"; then
  bash scripts/rust-stable/test.sh
fi

if step "cross-build manifest tests"; then
  bash scripts/cross-build/test.sh
fi

if step "build slot tests"; then
  bash scripts/build-slots/test.sh
fi

if step "pass stamp tests"; then
  bash scripts/pass-stamp/test.sh
fi

if step "check step registry tests"; then
  # --list prints exactly the steps a run runs, in order (this file's blocks).
  bash scripts/check-steps/test.sh
fi

if step "shellcheck"; then
  if command -v shellcheck >/dev/null 2>&1; then
    shellcheck -S warning scripts/*.sh scripts/*/*.sh site/agent-setup/*.sh .githooks/pre-push
  else
    skip_step "shellcheck not installed; skipped (CI runs it)"
  fi
fi

finish
