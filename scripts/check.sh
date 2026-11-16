#!/usr/bin/env bash
# Every check a change must pass, in one command. CI's ci.yml runs this
# script, and .githooks/pre-push runs it before every push, so a green run
# here means a green CI test job.
#
#   scripts/check.sh           run every step, stopping at the first failure
#   scripts/check.sh --fast    the fast tier: fmt, clippy and the tests of
#                              only what changed since the merge-base with
#                              origin/main (and the crates that depend on
#                              it); never writes a pass stamp
#   scripts/check.sh --list    print the step names, in order; run nothing
#
# --fast is the inner loop, and what the pre-push hook runs for a push to
# ticket/** branches only. It ends by saying it is NOT the full check: CI
# runs that. When it cannot tell what changed (no origin/main, no merge-base,
# a shallow clone) it runs every step, and still writes no stamp. Either
# way it checks the working tree, not the commit a push sends.
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
#               full run); a --fast --cargo step runs there only when a cargo
#               input changed
#   --fast-when PATTERN
#               the step belongs to the fast tier when a changed path matches
#               the shell pattern PATTERN (repeatable): a script suite's own
#               directory
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
  echo "usage: scripts/check.sh [--fast] [--list]" >&2
  exit 2
}

mode=run  # run, or list: print the step names and run nothing
tier=full # full, or fast: only steps tagged --fast or --fast-when
fast_run= # set by --fast; stays set if the run falls back to every step
while [ $# -gt 0 ]; do
  case $1 in
    --list) mode=list ;;
    --fast)
      tier=fast
      fast_run=1
      ;;
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

started=      # set when the first step of a run that may stamp starts
checked_tree= # the tree being checked, for the pass stamp `finish` writes
have_slot=    # set once the build slot is held; it is held until the run ends
unstamped=    # set by `skip_step --no-stamp`: the run writes no pass stamp
cargo_why=    # unset until cargo_skip decides; then why cargo skips, or "run"
changes_why=  # unset until `changes` runs; then "known", or why it cannot tell
changed=      # once known: the paths changed since the merge-base, one a line
deleted=      # once known: the paths among them that were deleted
diff_base=    # once known: the merge-base with origin/main
ran_any=      # set once a step's block runs (or a skipped cargo step)
fast_decided= # set once a --fast run has decided whether it can be fast

# step NAME [--fast] [--fast-when PATTERN]... [--no-slot] [--cargo]: opens a
# step's block,
# `if step "NAME"; then ... fi`. In a run it prints the step's header and the
# block runs; with --list it prints NAME and the block is skipped. So the list
# is always the steps that run, in order. The first step of a run records the
# tree for the pass stamp; the first step without --no-slot that is not
# skipped waits for a build slot. In the fast tier a step runs only when its
# tags say so (see the top of this file).
step() {
  local name=$1 fast='' slot=1 cargo='' when=()
  shift
  while [ $# -gt 0 ]; do
    case $1 in
      --fast) fast=1 ;;
      --fast-when)
        when+=("${2:?}")
        shift
        ;;
      --no-slot) slot= ;;
      --cargo) cargo=1 ;;
      *)
        printf 'check.sh: step "%s": unknown option %s\n' "$name" "$1" >&2
        exit 2
        ;;
    esac
    shift
  done
  if [ -n "$fast_run" ] && [ -z "$fast_decided" ]; then
    fast_decided=1
    if ! changes; then
      tier=full
      printf 'check.sh --fast: cannot tell what changed (%s); running every step\n' "$changes_why" >&2
    fi
  fi
  if [ "$tier" = fast ]; then
    [ -n "$fast" ] || [ ${#when[@]} -gt 0 ] || return 1
    if [ ${#when[@]} -gt 0 ]; then changed_match "${when[@]}" || return 1; fi
    if [ -n "$cargo" ]; then changed_cargo_input || return 1; fi
  fi
  if [ "$mode" = list ]; then
    printf '%s\n' "$name"
    return 1
  fi
  ran_any=1
  if [ -z "$started" ] && [ -z "$fast_run" ]; then
    started=1
    checked_tree=$(scripts/pass-stamp/stamp.sh tree 2>/dev/null || true)
  fi
  if [ -n "$cargo" ] && [ "$tier" = full ] && cargo_skip; then
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

# changes: whether this can tell what changed since the merge-base with
# origin/main, decided once per run; when it can, sets `changed` (every path
# that differs, untracked files included), `deleted` and `diff_base`. It
# cannot with no origin/main, no merge-base, or a shallow clone. The tree
# compared is the working tree, untracked files included, as the pass
# stamp's is. `git diff BASE` takes its paths from the index, so a file
# removed with `git rm --cached` counts as deleted though it is still on disk.
changes() {
  if [ -z "$changes_why" ]; then
    local untracked
    changes_why="no merge-base with origin/main, or a shallow clone"
    if [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = false ] &&
      diff_base=$(git merge-base HEAD refs/remotes/origin/main 2>/dev/null) &&
      deleted=$(git diff --no-renames --name-only --diff-filter=D "$diff_base" --) &&
      changed=$(git diff --no-renames --name-only "$diff_base" --) &&
      untracked=$(git ls-files --others --exclude-standard); then
      changes_why=known
      changed="$changed
$untracked"
    fi
  fi
  [ "$changes_why" = known ]
}

# changed_match PATTERN...: whether a changed path matches a shell pattern.
changed_match() {
  local path pattern
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    for pattern in "$@"; do
      # shellcheck disable=SC2254 # PATTERN is a pattern
      case $path in $pattern) return 0 ;; esac
    done
  done <<<"$changed"
  return 1
}

# changed_cargo_input: whether a changed path is a cargo input. A deletion
# always is: Rust tests read the tracked files' names.
changed_cargo_input() {
  local path
  [ -z "$deleted" ] || return 0
  while IFS= read -r path; do
    if [ -n "$path" ] && cargo_input "$path"; then
      return 0
    fi
  done <<<"$changed"
  return 1
}

# fast_crates: cargo test's package arguments in the fast tier. The fast
# tier must never be slower than the full check for the same change, so when
# the packages to test are half the workspace or more, it prints --all: the
# full step's own command, in one invocation that shares its build. Otherwise
# it prints fast_packages' `-p` list, again one invocation.
fast_crates() {
  local packages total picked
  packages=$(fast_packages)
  if [ "$packages" != --all ]; then
    total=$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null | jq '.packages | length' 2>/dev/null) || total=0
    picked=$(grep -o -- '-p ' <<<"$packages" | wc -l)
    if [ $((picked * 2)) -ge "${total:-0}" ]; then
      packages=--all
    fi
  fi
  echo "$packages"
}

# fast_packages: `-p NAME` for each package with a changed file and each
# package that depends on one, directly or not (dev-dependencies included),
# in workspace order. Package names and directories come from `cargo
# metadata`, never from directory names. It prints --all when it cannot
# narrow: a cargo input outside the packages changed (Cargo.toml, docs/,
# ...), a file was deleted, or cargo metadata or jq failed.
fast_packages() {
  local graph path dir name deps dep found set=' ' grew=1 out=''
  if ! changes || [ -n "$deleted" ] ||
    ! graph=$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null |
      jq -r '.workspace_root as $root | .packages[] |
        [(.manifest_path | ltrimstr($root + "/") | rtrimstr("/Cargo.toml")), .name,
          ([.dependencies[] | select(.path != null) | .name] | join(" "))] | @tsv' 2>/dev/null) ||
    [ -z "$graph" ]; then
    echo --all
    return
  fi
  while IFS= read -r path; do
    if [ -z "$path" ] || ! cargo_input "$path"; then
      continue
    fi
    found=
    while IFS=$'\t' read -r dir name deps; do
      case $path in
        "$dir"/*) found=$name ;;
      esac
    done <<<"$graph"
    if [ -z "$found" ]; then
      echo --all
      return
    fi
    set="$set$found "
  done <<<"$changed"
  # Add every package that depends on one in the set, until none is added.
  while [ -n "$grew" ]; do
    grew=
    while IFS=$'\t' read -r dir name deps; do
      case $set in *" $name "*) continue ;; esac
      for dep in $deps; do
        case $set in
          *" $dep "*)
            set="$set$name "
            grew=1
            break
            ;;
        esac
      done
    done <<<"$graph"
  done
  while IFS=$'\t' read -r dir name deps; do
    case $set in *" $name "*) out="$out -p $name" ;; esac
  done <<<"$graph"
  out=${out# }
  echo "${out:---all}"
}

# cargo_skip: whether the cargo steps of a full run can skip, decided once
# per run; when they can, cargo_why says why. They run in CI, and whenever
# `changes` cannot tell what changed.
cargo_skip() {
  if [ -z "$cargo_why" ]; then
    cargo_why=run
    if [ -z "${CI:-}${GITHUB_ACTIONS:-}" ] && changes && ! changed_cargo_input; then
      cargo_why="skipped: no cargo input differs from origin/main (merge-base ${diff_base:0:12}); CI runs it"
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
# honours only a full run's stamp. A --fast run never stamps, even when it
# ran every step.
finish() {
  if [ "$mode" = list ]; then
    exit 0
  fi
  if [ -n "$fast_run" ] && [ -z "$ran_any" ]; then
    printf '\n--fast: no steps ran; nothing they check changed. This was NOT the full check: no pass stamp written; scripts/check.sh (and CI) runs every step.\n'
    exit 0
  fi
  if [ -n "$fast_run" ] || [ "$tier" != full ]; then
    printf '\nFast checks passed. This was NOT the full check: no pass stamp written; scripts/check.sh (and CI) runs every step.\n'
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

if step "cargo test (private temporary directory)" --fast --cargo; then
  # A test that leaves a file or directory in the temporary directory fails the
  # check here, naming it, instead of slowly filling the shared /tmp. The fast
  # tier tests only the changed crates and their dependents (fast_crates).
  packages=--all
  if [ "$tier" = fast ]; then packages=$(fast_crates); fi
  # shellcheck disable=SC2086 # one word per package argument
  scripts/temp-leak/run.sh cargo test $packages
fi

if step "installer tests" --fast-when 'scripts/install/*'; then
  bash scripts/install/test.sh
fi

if step "release script tests" --fast-when 'scripts/release/*'; then
  bash scripts/release/test.sh
fi

if step "unsafe guard tests" --fast-when 'scripts/unsafe-guard/*'; then
  bash scripts/unsafe-guard/test.sh
fi

if step "intrinsic guard tests" --fast-when 'scripts/intrinsic-guard/*'; then
  bash scripts/intrinsic-guard/test.sh
fi

if step "kill guard tests" --fast-when 'scripts/kill-guard/*' --fast-when .claude/settings.json; then
  # The Claude Code hook (.claude/settings.json) that refuses pkill -f, killall
  # and other kills by pattern, name or session.
  bash scripts/kill-guard/test.sh
fi

if step "workflow guard tests" --fast-when 'scripts/workflow-guard/*'; then
  bash scripts/workflow-guard/test.sh
fi

if step "temp-leak wrapper tests" --fast-when 'scripts/temp-leak/*'; then
  bash scripts/temp-leak/test.sh
fi

if step "rust-stable tests" --fast-when 'scripts/rust-stable/*'; then
  bash scripts/rust-stable/test.sh
fi

if step "cross-build manifest tests" --fast-when 'scripts/cross-build/*'; then
  bash scripts/cross-build/test.sh
fi

if step "build slot tests" --fast-when 'scripts/build-slots/*'; then
  bash scripts/build-slots/test.sh
fi

if step "pass stamp tests" --fast-when 'scripts/pass-stamp/*' --fast-when '.githooks/*'; then
  bash scripts/pass-stamp/test.sh
fi

if step "check step registry tests" --fast-when 'scripts/check-steps/*' --fast-when scripts/check.sh; then
  # --list prints exactly the steps a run runs, in order (this file's blocks).
  bash scripts/check-steps/test.sh
fi

if step "workflow validation" --no-slot --fast-when '.github/workflows/*' --fast-when 'scripts/actionlint/*'; then
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

if step "workflow validation tests" --no-slot --fast-when 'scripts/actionlint/*'; then
  bash scripts/actionlint/test.sh
fi

if step "expected workflows tests" --no-slot --fast-when 'scripts/expected-workflows/*'; then
  bash scripts/expected-workflows/test.sh
fi

if step "shellcheck" --fast-when '*.sh' --fast-when .githooks/pre-push --fast-when 'scripts/shellcheck/*'; then
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

if step "powershell parse" --fast-when '*.ps1'; then
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
