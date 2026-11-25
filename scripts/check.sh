#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
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
#   scripts/check.sh --group G only the steps of group G: `tests` (the cargo
#                              test step) or `checks` (every other step);
#                              CI runs the groups in separate jobs. Never
#                              writes a pass stamp. See also OSKA_TEST_SHARD.
#
# --fast is the inner loop, and what the pre-push hook runs for a push to
# ticket/** branches only. It ends by saying it is NOT the full check: CI
# runs that. It is never slower than the full check for the same change.
# When it cannot tell what changed (no origin/main, no merge-base, a shallow
# clone) it runs every step, and still writes no stamp. Either way it checks
# the working tree, not the commit a push sends.
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
#               input changed, and only the --docs-tests step when just docs/
#               did
#   --fast-when PATTERN
#               the step belongs to the fast tier when a changed path matches
#               the shell pattern PATTERN (repeatable): a script suite's own
#               directory
#   --no-slot   the step is cheap and runs without a build slot
#   --cargo     a cargo step: skipped when cargo_plan says so (below)
#   --docs-tests  with --cargo: when only docs/ changed, the step still runs,
#               and its block runs just the tests that read docs/
#
# A step whose tool is missing calls `skip_step "REASON"` instead of failing;
# the run still writes the pass stamp, since running again here cannot do
# better and CI runs the step. `skip_step --no-stamp "REASON"` is for a skip
# the stamp cannot vouch for: the run then writes no stamp, and says which
# steps and why.
#
# Outside CI, the --cargo steps skip when no cargo input differs from the
# merge-base with origin/main; see cargo_plan below. That skip is decided
# from the tree the stamp names (its cargo inputs are main's), so the run
# still writes the stamp. When the only cargo inputs that differ are in
# docs/, the --docs-tests step runs just the tests that read docs/ and the
# other --cargo steps skip (as with --no-stamp): that narrowing rests on the
# docs-test list finding every reader, so the run writes no stamp. A skipped
# step prints its header and why, and waits for no build slot. CI ($CI or
# $GITHUB_ACTIONS set) always runs them all.
#
# Everything a run does happens inside a step block or in `step` and
# `finish` themselves, so --list can never run any of it.
#
# OSKA_BUILD_SLOTS=N (unset by default: no limit) makes the run wait for one
# of N machine-wide build slots before the first step that needs one; see
# scripts/build-slots/acquire.sh.

set -euo pipefail

# A failing test or a panic prints its stack trace, here and in CI alike, so
# the cause is in the first log read. A caller's own value wins.
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

cd "$(dirname "$0")/.."

usage() {
  echo "usage: scripts/check.sh [--fast] [--list]" >&2
  exit 2
}

mode=run  # run, or list: print the step names and run nothing
only_group= # set by --group: run only the steps of that group
tier=full # full, or fast: only steps tagged --fast or --fast-when
fast_run= # set by --fast; stays set if the run falls back to every step
while [ $# -gt 0 ]; do
  case $1 in
    --list) mode=list ;;
    --group)
      only_group=${2:-}
      case $only_group in tests | checks) ;; *) usage ;; esac
      shift
      ;;
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
unstamped=    # set by `skip_step --no-stamp` or a docs-narrowed step: the
#               run writes no pass stamp; "STEP: REASON" per line
current_step= # the step whose block is running, for `skip_step --no-stamp`
cargo_mode=   # unset until cargo_plan decides; then full, skip or docs
cargo_why=    # why the cargo steps skip (skip) or narrow (docs)
docs_tests=   # docs mode: the tests that read docs/, "PACKAGE TEST" per line
changes_why=  # unset until `changes` runs; then "known", or why it cannot tell
cargo_changed= # set by `changes` when it can tell: the paths that differ from
#                the merge-base with origin/main (changed and untracked), one per line
deleted=      # once known: the paths among them that were deleted
diff_base=    # once known: the merge-base with origin/main
ran_any=      # set once a step's block runs
fast_decided= # set once a --fast run has decided whether it can be fast

# step NAME [--fast] [--fast-when PATTERN]... [--no-slot] [--cargo]
# [--docs-tests] [--group tests]: opens a step's block,
# `if step "NAME"; then ... fi`. In a run it prints the step's header and the
# block runs; with --list it prints NAME and the block is skipped. So the list
# is always the steps that run, in order. The first step of a run records the
# tree for the pass stamp; the first step without --no-slot that is not
# skipped waits for a build slot. In the fast tier a step runs only when its
# tags say so (see the top of this file).
step() {
  local name=$1 fast='' slot=1 cargo='' docs='' when=() group=checks
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
      --docs-tests) docs=1 ;;
      --group)
        group=${2:?}
        shift
        ;;
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
  # --group: another group's steps are not listed and do not run.
  if [ -n "$only_group" ] && [ "$only_group" != "$group" ]; then
    return 1
  fi
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
    if [ -n "$cargo" ]; then
      # The same plan as a full run: nothing for skip, only the docs tests
      # for docs, and otherwise only when a cargo input changed.
      cargo_plan
      case $cargo_mode in
        skip) return 1 ;;
        docs) [ -n "$docs" ] || return 1 ;;
        *) changed_cargo_input || return 1 ;;
      esac
    fi
  fi
  if [ "$mode" = list ]; then
    printf '%s\n' "$name"
    return 1
  fi
  ran_any=1
  current_step=$name
  if [ -z "$started" ] && [ -z "$fast_run" ]; then
    started=1
    checked_tree=$(scripts/pass-stamp/stamp.sh tree 2>/dev/null || true)
  fi
  if [ -n "$cargo" ]; then
    cargo_plan
    # No cargo input changed: the stamp's tree vouches for the skip.
    if [ "$cargo_mode" = skip ]; then
      printf '\n== %s\n' "$name"
      skip_step "skipped: $cargo_why; CI runs it"
      return 1
    fi
    if [ "$cargo_mode" = docs ] && [ -z "$docs" ]; then
      printf '\n== %s\n' "$name"
      skip_step --no-stamp "skipped: $cargo_why; CI runs it"
      return 1
    fi
    # A narrowed run vouches for less than the stamp says.
    if [ "$cargo_mode" = docs ]; then
      unstamped="$unstamped$name: narrowed to the tests that read docs/"$'\n'
    fi
  fi
  if [ -n "$slot" ] && [ -z "$have_slot" ]; then
    have_slot=1
    . scripts/build-slots/acquire.sh
  fi
  printf '\n== %s\n' "$name"
}

# skip_step [--no-stamp] REASON: inside a step's block, says why the step did
# not run. With --no-stamp, the run writes no pass stamp, and `finish` names
# the step and REASON.
skip_step() {
  if [ "$1" = --no-stamp ]; then
    shift
    unstamped="$unstamped$current_step: $1"$'\n'
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
# scripts/docs-tests/ picks the tests a docs/ change runs, scripts/eval-tasks/
# validates the eval task set, the oska-eval tests scan
# scripts/eval-batch/fixtures/ for home paths, and this file runs the cargo
# steps.
cargo_input() {
  case $1 in
    .github/workflows/pages.yml | scripts/install/* | scripts/temp-leak/* | scripts/docs-tests/* | scripts/eval-tasks/* | scripts/eval-batch/fixtures/* | scripts/check.sh) return 0 ;;
    CLAUDE.md | CODING_STANDARDS.md | LICENSE | .gitignore) return 1 ;;
    .githooks/* | .claude/* | .github/* | scripts/*) return 1 ;;
  esac
  return 0
}

# changes: whether this can tell what changed since the merge-base with
# origin/main, decided once per run; when it can, sets `cargo_changed` (every
# path that differs, untracked files included, sorted), `deleted` and
# `diff_base`. cargo_plan and the fast tier both read it. It
# cannot with no origin/main, no merge-base, or a shallow clone. The tree
# compared is the working tree, untracked files included, as the pass
# stamp's is. `git diff BASE` takes its paths from the index, so a file
# removed with `git rm --cached` counts as deleted though it is still on disk.
changes() {
  if [ -z "$changes_why" ]; then
    local changed untracked
    changes_why="no merge-base with origin/main, or a shallow clone"
    if [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = false ] &&
      diff_base=$(git merge-base HEAD refs/remotes/origin/main 2>/dev/null) &&
      deleted=$(git diff --no-renames --name-only --diff-filter=D "$diff_base" --) &&
      changed=$(git diff --no-renames --name-only "$diff_base" --) &&
      untracked=$(git ls-files --others --exclude-standard); then
      changes_why=known
      cargo_changed=$(printf '%s\n%s\n' "$changed" "$untracked" | sed '/^$/d' | sort -u)
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
  done <<<"$cargo_changed"
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
  done <<<"$cargo_changed"
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
  done <<<"$cargo_changed"
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

# cargo_plan: sets cargo_mode, once per run, to what the cargo steps do:
#
#   skip  no cargo input differs from the merge-base with origin/main
#   docs  the only inputs that differ are in docs/, and none is compiled into
#         a crate: just the tests that read docs/ run (docs_tests lists them)
#   full  anything else, in CI, and whenever this cannot tell: no origin/main,
#         no merge-base, a shallow clone, a docs-test list that fails or
#         finds no test, or a drift guard (list.sh --check) that fails
#
# cargo_why says why for skip and docs. The paths compared are `changes`'s,
# the one classifier that --fast shares.
cargo_plan() {
  [ -z "$cargo_mode" ] || return 0
  cargo_mode=full
  local path docs='' compiled
  if [ -z "${CI:-}${GITHUB_ACTIONS:-}" ] && changes; then
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
      cargo_why="no cargo input differs from origin/main (merge-base ${diff_base:0:12})"
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
    cargo_why="only docs/ differs from origin/main (merge-base ${diff_base:0:12})"
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
  if [ -n "$only_group" ]; then
    printf '\nGroup %s passed. This was NOT the full check: no pass stamp written; scripts/check.sh (and the other CI jobs) run the rest.\n' "$only_group"
    exit 0
  fi
  if [ -n "$fast_run" ] || [ "$tier" != full ]; then
    printf '\nFast checks passed. This was NOT the full check: no pass stamp written; scripts/check.sh (and CI) runs every step.\n'
    exit 0
  fi
  if [ -n "$unstamped" ]; then
    echo "pass stamp: not written, the stamp cannot vouch for these steps:"
    printf '%s' "$unstamped" | sed 's/^/  /'
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

if step "only allowed actions, pinned to a commit" --no-slot --fast-when '.github/workflows/*' --fast-when 'scripts/workflow-guard/*'; then
  # actions/*, dtolnay/rust-toolchain and the two OpenSSF Scorecard actions,
  # each pinned to a full commit SHA; the list is in the script.
  scripts/workflow-guard/allowlist.sh || fail "workflows may use only actions/*, dtolnay/rust-toolchain, ossf/scorecard-action and github/codeql-action/upload-sarif, each pinned to a full commit SHA"
fi

if step "SPDX headers and NOTICE" --no-slot --fast-when 'scripts/spdx-guard/*'; then
  # Every tracked .rs, .osk and .sh file starts with an SPDX-License-Identifier
  # header, and the tree has a NOTICE file (Apache 2.0 section 4).
  scripts/spdx-guard/guard.sh
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

if step "cargo test (private temporary directory)" --fast --cargo --docs-tests --group tests; then
  # A test that leaves a file or directory in the temporary directory fails the
  # check here, naming it, instead of slowly filling the shared /tmp.
  # The fast tier tests only the changed crates and their dependents
  # (fast_crates).
  # In CI, where the formal step needs Lean, the differential test against
  # the Lean reference (oska-run's lean_reference) fails without Lean
  # rather than skipping (ADR 0018).
  if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
    export OSKA_LEAN_REFERENCE=required
  fi
  if [ "$cargo_mode" = docs ]; then
    run_docs_tests
  elif [ "$tier" = fast ]; then
    # shellcheck disable=SC2046 # one word per package argument
    scripts/temp-leak/run.sh cargo test $(fast_crates)
  elif [ -n "${OSKA_TEST_SHARD:-}" ]; then
    # CI's parallel shards of this step (scripts/test-shard.sh): the set is
    # the host's, "linux" or "macos". Every shard of a set together is the
    # whole `cargo test --all`.
    scripts/temp-leak/run.sh scripts/test-shard.sh "${OSKA_TEST_SET:?OSKA_TEST_SHARD needs OSKA_TEST_SET}" "$OSKA_TEST_SHARD"
  else
    scripts/temp-leak/run.sh cargo test --all
  fi
fi

if step "eval task set" --cargo --fast-when 'eval/*' --fast-when 'crates/oska-eval/*' --fast-when 'scripts/eval-tasks/*'; then
  # The harness's `validate` over eval/tasks, with the oskac just built
  # (offline). The fast tier runs it when the task set or the harness changed;
  # a compiler change gets it from the full check and CI.
  scripts/eval-tasks/run.sh
fi

if step "eval batch driver tests" --fast-when 'scripts/eval-batch/*' --fast-when 'crates/oska-eval/*'; then
  # The live driver end to end with a stub `claude` on PATH: offline, no
  # model is called. The driver itself refuses to run live in CI.
  # The step takes the build slot in `step` before this block, whether or
  # not jq is here (scripts/check-steps/test.sh relies on it).
  if command -v jq >/dev/null 2>&1; then
    bash scripts/eval-batch/test.sh
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: jq is not installed, and CI must run the eval batch driver tests" >&2
      exit 1
    fi
    skip_step "jq not installed; skipped (CI runs it)"
  fi
fi

if step "installer tests" --fast-when 'scripts/install/*'; then
  bash scripts/install/test.sh
fi

if step "release script tests" --fast-when 'scripts/release/*'; then
  bash scripts/release/test.sh
fi

if step "release notes tests" --fast-when 'scripts/release/*'; then
  # scripts/release/notes.sh, the GitHub release body: the version's
  # CHANGELOG.md section and a footer; it fails closed.
  bash scripts/release/notes-test.sh
fi

if step "release image tests" --fast-when 'scripts/release/*'; then
  # The oskac container image builder (scripts/release/image.sh): the same
  # fake binaries give the same OCI bytes, and the layout holds only /oskac
  # and /tmp, run as user 65532. They read GNU tar's listing, and jq, as
  # the release runners have them.
  if grep -q 'GNU tar' <<<"$(tar --version 2>/dev/null || true)" && command -v jq >/dev/null 2>&1; then
    bash scripts/release/image-test.sh
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: GNU tar or jq is not installed, and CI must run the release image tests" >&2
      exit 1
    fi
    skip_step "GNU tar or jq not installed; skipped (CI runs it)"
  fi
fi

if step "release image push tests" --fast-when 'scripts/release/*'; then
  # scripts/release/push-image.sh, with a fake skopeo: it pushes only the
  # image the signed release rebuilds to, and fails when the registry
  # serves another digest than the signed one.
  bash scripts/release/push-image-test.sh
fi

if step "Linux build check tests" --fast-when 'scripts/release/*'; then
  # They run GNU readelf, as the release runners have it.
  if grep -q 'GNU readelf' <<<"$(readelf --version 2>/dev/null || true)"; then
    bash scripts/release/check-linux-build-test.sh
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: GNU readelf is not installed, and CI must run the Linux build check tests" >&2
      exit 1
    fi
    skip_step "GNU readelf not installed; skipped (CI runs it)"
  fi
fi

if step "unsafe guard tests" --fast-when 'scripts/unsafe-guard/*'; then
  bash scripts/unsafe-guard/test.sh
fi

if step "intrinsic guard tests" --fast-when 'scripts/intrinsic-guard/*'; then
  bash scripts/intrinsic-guard/test.sh
fi

if step "SPDX guard tests" --fast-when 'scripts/spdx-guard/*'; then
  bash scripts/spdx-guard/test.sh
fi

if step "kill guard tests" --fast-when 'scripts/kill-guard/*' --fast-when .claude/settings.json; then
  # The Claude Code hook (.claude/settings.json) that refuses pkill -f, killall
  # and other kills by pattern, name or session.
  bash scripts/kill-guard/test.sh
fi

if step "skill lint" --no-slot --fast-when '.claude/skills/*' --fast-when 'scripts/skill-lint/*'; then
  # The agent skills under .claude/skills/ against the mechanical rules of
  # docs/agents/skills.md: frontmatter, size, a Security section, links,
  # no remote assets, strict shell scripts.
  bash scripts/skill-lint/test.sh
fi

if step "claude plugin validate" --no-slot --fast-when '.claude-plugin/*' --fast-when 'plugins/*'; then
  # The Claude Code marketplace at the root and the `oska` plugin it lists
  # (spec #206), checked by Claude Code's own validator. Warnings fail too.
  # The JSON-shape tests in crates/oskac/tests/agent_setup.rs run everywhere;
  # this step adds the real validator where `claude` is installed.
  if command -v claude >/dev/null 2>&1; then
    claude plugin validate --strict .
    claude plugin validate --strict plugins/oska
  else
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: claude is not installed, and CI must validate the plugin" >&2
      exit 1
    fi
    skip_step "claude not installed; plugin validation skipped (CI runs it; the agent_setup JSON-shape tests still ran)"
  fi
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

if step "cargo cache tests" --fast-when 'scripts/cargo-cache/*'; then
  bash scripts/cargo-cache/test.sh
fi

if step "build slot tests" --fast-when 'scripts/build-slots/*'; then
  bash scripts/build-slots/test.sh
fi

if step "pass stamp tests" --fast-when 'scripts/pass-stamp/*' --fast-when '.githooks/*'; then
  bash scripts/pass-stamp/test.sh
fi

if step "docs-test list tests" --fast-when 'scripts/docs-tests/*'; then
  # The Rust tests that read docs/, which a docs-only change runs.
  bash scripts/docs-tests/test.sh
fi

if step "test shard tests" --fast-when 'scripts/test-shard*' --fast-when '.github/workflows/ci.yml' --fast-when 'crates/*/tests/*'; then
  # Each test of a CI job's set runs in exactly one shard (scripts/test-shard.sh).
  bash scripts/test-shard/test.sh
fi

if step "check step registry tests" --fast-when 'scripts/check-steps/*' --fast-when scripts/check.sh; then
  # --list prints exactly the steps a run runs, in order (this file's blocks).
  bash scripts/check-steps/test.sh
fi

if step "formal" --fast-when 'formal/*' --fast-when 'scripts/formal/*'; then
  # Builds the Lean proofs of Oska Core and runs TLC on every TLA+ model
  # (ADRs 0018, 0019; docs/agents/formal.md). An unfinished proof (`sorry`)
  # fails. A missing Lean or TLC skips here, but fails in CI, where the test
  # job installs the pinned ones. run.sh exits 1 there, not 3, and this block
  # refuses a skip in CI too, so neither alone can let the gate pass.
  rc=0
  scripts/formal/run.sh || rc=$?
  if [ "$rc" -eq 3 ]; then
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
      echo "check.sh: Lean or TLC is not installed, and CI must run the formal step" >&2
      exit 1
    fi
    skip_step "Lean or TLC not installed; skipped (CI runs it; scripts/formal/install.sh installs them)"
  elif [ "$rc" -ne 0 ]; then
    exit "$rc"
  fi
fi

if step "formal step tests" --no-slot --fast-when 'formal/*' --fast-when 'scripts/formal/*'; then
  bash scripts/formal/test.sh
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

if step "x509 roots generator tests" --no-slot --fast-when 'scripts/x509-roots/*' --fast-when 'crates/oska-check/src/std/x509_roots.osk'; then
  bash scripts/x509-roots/test.sh
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
