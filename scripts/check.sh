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
#   --cargo     a cargo step: skipped when cargo_plan says so (below)
#   --docs-tests  with --cargo: when only docs/ changed, the step still runs,
#               and its block runs just the tests that read docs/
#
# A step whose tool is missing calls `skip_step "REASON"` instead of failing;
# the run still writes the pass stamp, since running again here cannot do
# better and CI runs the step. `skip_step --no-stamp "REASON"` is for a skip
# the stamp cannot vouch for: the run then writes no stamp.
#
# Outside CI, the --cargo steps skip (as with --no-stamp) when no cargo input
# differs from the merge-base with origin/main; see cargo_plan below. When
# the only cargo inputs that differ are in docs/, the --docs-tests step runs
# just the tests that read docs/ and the other --cargo steps skip; that run
# writes no stamp either. A skipped step prints its header and why, and waits
# for no build slot. CI ($CI or $GITHUB_ACTIONS set) always runs them all.
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
cargo_mode=   # unset until cargo_plan decides; then full, skip or docs
cargo_why=    # why the cargo steps skip (skip) or narrow (docs)
docs_tests=   # docs mode: the tests that read docs/, "PACKAGE TEST" per line
cargo_changed= # set by cargo_plan when it can tell: the paths that differ from
#                the merge-base with origin/main (changed and untracked), one per line

# step NAME [--fast] [--no-slot]: opens a step's block,
# `if step "NAME"; then ... fi`. In a run it prints the step's header and the
# block runs; with --list it prints NAME and the block is skipped. So the list
# is always the steps that run, in order. The first step of a run records the
# tree for the pass stamp; the first step without --no-slot that is not
# skipped waits for a build slot.
step() {
  local name=$1 fast='' slot=1 cargo='' docs=''
  shift
  while [ $# -gt 0 ]; do
    case $1 in
      --fast) fast=1 ;;
      --no-slot) slot= ;;
      --cargo) cargo=1 ;;
      --docs-tests) docs=1 ;;
      *)
        printf 'check.sh: step "%s": unknown option %s\n' "$name" "$1" >&2
        exit 2
        ;;
    esac
    shift
  done
  if [ -n "$docs" ] && [ -z "$cargo" ]; then
    printf 'check.sh: step "%s": --docs-tests needs --cargo\n' "$name" >&2
    exit 2
  fi
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
  if [ -n "$cargo" ]; then
    cargo_plan
    if [ "$cargo_mode" = skip ] || { [ "$cargo_mode" = docs ] && [ -z "$docs" ]; }; then
      printf '\n== %s\n' "$name"
      skip_step --no-stamp "skipped: $cargo_why; CI runs it"
      return 1
    fi
    # A narrowed run vouches for less than the stamp says.
    if [ "$cargo_mode" = docs ]; then unstamped=1; fi
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
# scripts/docs-tests/ picks the tests a docs/ change runs, and this file runs
# the cargo steps.
cargo_input() {
  case $1 in
    .github/workflows/pages.yml | scripts/install/* | scripts/temp-leak/* | scripts/docs-tests/* | scripts/check.sh) return 0 ;;
    CLAUDE.md | CODING_STANDARDS.md | LICENSE | .gitignore) return 1 ;;
    .githooks/* | .claude/* | .github/* | scripts/*) return 1 ;;
  esac
  return 0
}

# cargo_plan: sets cargo_mode, once per run, to what the cargo steps do:
#
#   skip  no cargo input differs from the merge-base with origin/main
#   docs  the only inputs that differ are in docs/, and none is compiled into
#         a crate: just the tests that read docs/ run (docs_tests lists them)
#   full  anything else, in CI, and whenever this cannot tell: no origin/main,
#         no merge-base, a shallow clone, a docs-test list that fails or
#         finds no test, or a drift guard (list.sh --check) that fails
#
# cargo_why says why for skip and docs; cargo_changed holds the paths
# compared, for other steps to reuse. The tree compared is the working
# tree, untracked files included, as the pass stamp's is. `git diff BASE`
# takes its paths from the index, so a file removed with `git rm --cached`
# counts as deleted though it is still on disk.
cargo_plan() {
  [ -z "$cargo_mode" ] || return 0
  cargo_mode=full
  local base deleted changed untracked path docs='' compiled
  if [ -z "${CI:-}${GITHUB_ACTIONS:-}" ] &&
    [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = false ] &&
    base=$(git merge-base HEAD refs/remotes/origin/main 2>/dev/null) &&
    deleted=$(git diff --no-renames --name-only --diff-filter=D "$base" --) &&
    changed=$(git diff --no-renames --name-only "$base" --) &&
    untracked=$(git ls-files --others --exclude-standard); then
    cargo_changed=$(printf '%s\n%s\n' "$changed" "$untracked" | sed '/^$/d' | sort -u)
    # Rust tests read the tracked files' names, so a deletion is an input.
    [ -z "$deleted" ] || return 0
    while IFS= read -r path; do
      if [ -n "$path" ] && cargo_input "$path"; then
        case $path in
          docs/*) docs="$docs$path"$'\n' ;;
          *) return 0 ;;
        esac
      fi
    done <<<"$cargo_changed"
    if [ -z "$docs" ]; then
      cargo_mode=skip
      cargo_why="no cargo input differs from origin/main (merge-base ${base:0:12})"
      return 0
    fi
    # A docs/ file compiled into a crate changes the build: all of cargo.
    compiled=$(scripts/docs-tests/list.sh --compiled) || return 0
    while IFS= read -r path; do
      [ -n "$path" ] || continue
      if [ "$compiled" = docs ] || grep -qxF "$path" <<<"$compiled"; then
        return 0
      fi
    done <<<"$docs"
    # A docs/ reader the list may miss: all of cargo, saying why.
    if ! scripts/docs-tests/list.sh --check; then
      echo "cargo: only docs/ differs, but the docs-test list may miss a reader (above); running all of cargo"
      return 0
    fi
    docs_tests=$(scripts/docs-tests/list.sh) && [ -n "$docs_tests" ] || return 0
    cargo_mode=docs
    cargo_why="only docs/ differs from origin/main (merge-base ${base:0:12})"
  fi
}

# run_docs_tests: the --docs-tests step's block in docs mode. Says which
# tests run and why, then runs them, one cargo test per package.
run_docs_tests() {
  local package line
  printf 'narrowed: %s; running just the tests that read docs/ (found by scripts/docs-tests/list.sh); CI runs them all:\n' "$cargo_why"
  while IFS= read -r line; do printf '  %s\n' "$line"; done <<<"$docs_tests"
  for package in $(cut -d' ' -f1 <<<"$docs_tests" | uniq); do
    # shellcheck disable=SC2046 # one --test per word
    scripts/temp-leak/run.sh cargo test -p "$package" \
      $(sed -n "s/^$package /--test /p" <<<"$docs_tests")
  done
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

if step "oskac type-checks for Windows" --cargo; then
  # `cfg(windows)` code is not compiled on Linux, so a type error in it shows
  # up only on the Windows CI job. A check-only build needs no linker, and the
  # workspace has no external crates. The target is a one-time local
  # `rustup target add` (docs/agents/rust.md); the ci.yml test job adds it.
  if rustup target list --installed 2>/dev/null | grep -qx 'x86_64-pc-windows-gnu'; then
    cargo check -p oskac --tests --target x86_64-pc-windows-gnu
  elif [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
    echo "check.sh: x86_64-pc-windows-gnu target missing in CI; the ci.yml test job must run 'rustup target add x86_64-pc-windows-gnu'" >&2
    exit 1
  else
    skip_step "x86_64-pc-windows-gnu target not installed; skipped (run: rustup target add x86_64-pc-windows-gnu)"
  fi
fi

if step "cargo test (private temporary directory)" --cargo --docs-tests; then
  # A test that leaves a file or directory in the temporary directory fails the
  # check here, naming it, instead of slowly filling the shared /tmp.
  if [ "$cargo_mode" = docs ]; then
    run_docs_tests
  else
    scripts/temp-leak/run.sh cargo test --all
  fi
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

if step "docs-test list tests"; then
  # The Rust tests that read docs/, which a docs-only change runs.
  bash scripts/docs-tests/test.sh
fi

if step "check step registry tests"; then
  # --list prints exactly the steps a run runs, in order (this file's blocks).
  bash scripts/check-steps/test.sh
fi

if step "workflow validation" --no-slot; then
  # GitHub makes no jobs and no checks for a workflow it cannot parse, so a
  # broken file looks like a green PR (#329). CI must never skip this.
  rc=0
  bash scripts/actionlint/run.sh || rc=$?
  if [ "$rc" -eq 3 ]; then
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "actionlint is required in CI" >&2
      exit 1
    fi
    skip_step "actionlint unavailable; skipped (CI runs it)"
  elif [ "$rc" -ne 0 ]; then
    exit "$rc"
  fi
fi

if step "workflow validation tests" --no-slot; then
  bash scripts/actionlint/test.sh
fi

if step "expected workflows tests" --no-slot; then
  bash scripts/expected-workflows/test.sh
fi

if step "shellcheck"; then
  if command -v shellcheck >/dev/null 2>&1; then
    # Same files and severity as the installers workflow.
    scripts/shellcheck/run.sh
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: shellcheck is not installed, and CI must run it" >&2
      exit 1
    fi
    skip_step "shellcheck not installed; skipped (CI runs it)"
  fi
fi

if step "powershell parse"; then
  # Syntax-only parse of the installer scripts; only Windows CI runs them.
  if command -v pwsh >/dev/null 2>&1; then
    # shellcheck disable=SC2016 # PowerShell variables, not shell ones
    pwsh -NoProfile -NonInteractive -Command '
      $bad = 0
      foreach ($f in "scripts/install/install.ps1", "scripts/install/test.ps1") {
        $errors = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $f), [ref]$null, [ref]$errors)
        foreach ($e in $errors) { "${f}:$($e.Extent.StartLineNumber): $($e.Message)"; $bad++ }
      }
      if ($bad -gt 0) { exit 1 }'
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: pwsh is not installed, and CI must run it" >&2
      exit 1
    fi
    skip_step "pwsh not installed; skipped (CI runs it)"
  fi
fi

finish
