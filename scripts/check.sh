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
#                              test step), `formal` (Lean and TLC), `lint`
#                              (the static checks of the source tree) or
#                              `scripts` (the script suites and the eval
#                              task set); CI runs the groups in separate
#                              jobs. `checks` is all of formal, lint and
#                              scripts. Never writes a pass stamp. See also
#                              OSKA_TEST_SHARD.
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
#   --concurrent  with --group scripts: the step may run beside other
#               suites (the audit in docs/agents/check-concurrency.md says
#               which); OSKA_CHECK_JOBS=1 runs one at a time
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
  echo "usage: scripts/check.sh [--fast] [--list] [--group tests|formal|lint|scripts|checks]" >&2
  exit 2
}

mode=run  # run, or list: print the step names and run nothing
only_group= # set by --group: run only the steps of that group
tier=full # full, or fast: only steps tagged --fast or --fast-when
fast_run= # set by --fast; stays set if the run falls back to every step
internal_step= # set by --internal-step: this process runs only that step
while [ $# -gt 0 ]; do
  case $1 in
    --list) mode=list ;;
    --group)
      only_group=${2:-}
      case $only_group in tests | formal | lint | scripts | checks) ;; *) usage ;; esac
      shift
      ;;
    --internal-step)
      # Used by a concurrent --group run: run just this step, in a child.
      internal_step=${2:-}
      [ -n "$internal_step" ] || usage
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

# OSKA_CHECK_JOBS=N: how many suites a `--group scripts` run may run at once
# (default: the number of cores); 1 runs them one at a time, as before.
jobs=${OSKA_CHECK_JOBS:-}
if [ -z "$jobs" ]; then
  jobs=$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
fi
case $jobs in
  '' | 0* | *[!0-9]*)
    echo "check.sh: OSKA_CHECK_JOBS must be a positive integer, got '$jobs'" >&2
    exit 2
    ;;
esac
concurrent_mode= # set when this run queues its --concurrent steps
if [ "$only_group" = scripts ] && [ "$mode" = run ] && [ -z "$internal_step" ] && [ "$jobs" -gt 1 ]; then
  concurrent_mode=1
fi
queue_names=() # concurrent mode: every step of the group, in order
queue_conc=()  # 1 for a --concurrent step, else empty (it runs alone)
queue_slot=    # set when a queued step wants a build slot

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
timed_step=   # the step whose header printed and whose time is running
timed_from=0  # SECONDS when that step's header printed
step_times=   # "SECONDS NAME" per finished step, for the slowest-steps summary

# begin_timing NAME / end_timing: time a step from its header to the next
# step's start (or `finish`), with the shell's SECONDS and no external tool.
# end_timing prints the step's elapsed seconds and records it.
begin_timing() {
  timed_step=$1
  timed_from=$SECONDS
}
end_timing() {
  [ -n "$timed_step" ] || return 0
  local took=$((SECONDS - timed_from))
  printf '(%ss)\n' "$took"
  step_times="$step_times$took $timed_step"$'\n'
  timed_step=
}
# slowest_steps: the five slowest steps of the run, longest first.
slowest_steps() {
  [ -n "$step_times" ] || return 0
  printf '\nSlowest steps:\n'
  printf '%s' "$step_times" | sort -rn -k1,1 -s | head -n 5 |
    while read -r took name; do printf '  %5ss  %s\n' "$took" "$name"; done
}

# step NAME [--fast] [--fast-when PATTERN]... [--no-slot] [--cargo]
# [--docs-tests] [--group G]: opens a step's block,
# `if step "NAME"; then ... fi`. In a run it prints the step's header and the
# block runs; with --list it prints NAME and the block is skipped. So the list
# is always the steps that run, in order. The first step of a run records the
# tree for the pass stamp; the first step without --no-slot that is not
# skipped waits for a build slot. In the fast tier a step runs only when its
# tags say so (see the top of this file).
step() {
  local name=$1 fast='' slot=1 cargo='' docs='' when=() group=lint conc=''
  end_timing
  shift
  while [ $# -gt 0 ]; do
    case $1 in
      --fast) fast=1 ;;
      --fast-when)
        when+=("${2:?}")
        shift
        ;;
      --no-slot) slot= ;;
      --concurrent) conc=1 ;;
      --cargo) cargo=1 ;;
      --docs-tests) docs=1 ;;
      --group)
        group=${2:?}
        case $group in tests | formal | lint | scripts) ;; *)
          printf 'check.sh: step "%s": unknown group %s\n' "$name" "$group" >&2
          exit 2
          ;;
        esac
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
  # --group: another group's steps are not listed and do not run. `checks`
  # is every group but `tests`. A step with no --group is in `lint`.
  if [ -n "$only_group" ] && [ "$only_group" != "$group" ]; then
    if [ "$only_group" != checks ] || [ "$group" = tests ]; then
      return 1
    fi
  fi
  if [ -n "$internal_step" ]; then
    # The parent run decided this step runs; any other step is not ours.
    [ "$name" = "$internal_step" ] || return 1
  elif [ -n "$fast_run" ] && [ -z "$fast_decided" ]; then
    fast_decided=1
    if ! changes; then
      tier=full
      printf 'check.sh --fast: cannot tell what changed (%s); running every step\n' "$changes_why" >&2
    fi
  fi
  if [ "$tier" = fast ] && [ -z "$internal_step" ]; then
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
  if [ -n "$concurrent_mode" ]; then
    # run_queue runs it, in a child, when the last step has been seen.
    queue_names+=("$name")
    queue_conc+=("$conc")
    [ -z "$slot" ] || queue_slot=1
    return 1
  fi
  if [ -z "$started" ] && [ -z "$fast_run" ]; then
    started=1
    checked_tree=$(scripts/pass-stamp/stamp.sh tree 2>/dev/null || true)
  fi
  if [ -n "$cargo" ]; then
    cargo_plan
    # No cargo input changed: the stamp's tree vouches for the skip.
    if [ "$cargo_mode" = skip ]; then
      printf '\n== %s\n' "$name"
      begin_timing "$name"
      skip_step "skipped: $cargo_why; CI runs it"
      return 1
    fi
    if [ "$cargo_mode" = docs ] && [ -z "$docs" ]; then
      printf '\n== %s\n' "$name"
      begin_timing "$name"
      skip_step --no-stamp "skipped: $cargo_why; CI runs it"
      return 1
    fi
    # A narrowed run vouches for less than the stamp says.
    if [ "$cargo_mode" = docs ]; then
      unstamped="$unstamped$name: narrowed to the tests that read docs/"$'\n'
    fi
  fi
  if [ -n "$slot" ] && [ -z "$have_slot" ] && [ -z "$internal_step" ]; then
    have_slot=1
    . scripts/build-slots/acquire.sh
  fi
  printf '\n== %s\n' "$name"
  begin_timing "$name"
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

# fail MESSAGE / fail_rc CODE MESSAGE: end the run. The failing step's seconds
# and the slowest steps print first, so a failure is timed like a pass.
fail() { fail_rc 1 "$1"; }
fail_rc() {
  end_timing
  [ -n "$internal_step" ] || slowest_steps
  step_times=
  printf 'check.sh: %s\n' "$2" >&2
  exit "$1"
}

# descendants PID: every process below PID, deepest first (ps works on Linux
# and macOS alike).
descendants() {
  local c
  for c in $(ps -A -o pid= -o ppid= 2>/dev/null | awk -v p="$1" '$2 == p { print $1 }'); do
    descendants "$c"
    printf '%s\n' "$c"
  done
}

# The concurrent run's state, global so the EXIT trap can clean up.
q_tmp=      # the private buffer directory
q_pids=()   # the launch_step subshell of each queued step, by place
# queued_pids: the launch_step subshell of each unfinished queued step and the
# runner it started (its pid file), then everything below them, deepest first.
# A step with a result file has ended, and its pid may belong to another
# process by now, so it is left alone.
queued_pids() {
  local i p
  [ -n "$q_tmp" ] && [ "${#q_pids[@]}" -gt 0 ] || return 0
  for i in "${!q_pids[@]}"; do
    [ ! -f "$q_tmp/$i.rc" ] || continue
    p=${q_pids[i]}
    descendants "$p"
    printf '%s\n' "$p"
    p=$(cat "$q_tmp/$i.pid" 2>/dev/null) || continue
    [ -n "$p" ] || continue
    descendants "$p"
    printf '%s\n' "$p"
  done
}
# unfinished_steps: the places of the queued steps that have no result yet.
unfinished_steps() {
  local i
  [ -n "$q_tmp" ] && [ "${#q_pids[@]}" -gt 0 ] || return 0
  for i in "${!q_pids[@]}"; do
    [ -f "$q_tmp/$i.rc" ] || printf '%s ' "$i"
  done
}
# queue_cleanup: stops every process the queued steps started (their runners
# and everything below), waits for them, then removes the buffer directory, so
# nothing outlives the run or writes into a deleted directory. The process
# tree is read again after each kill, because a process that was starting when
# it was read is not in the first list, and a runner whose launch subshell is
# gone has been adopted by init: its pid file still names it.
queue_cleanup() {
  local p pid round seen='' now left
  if [ -n "$(unfinished_steps)" ] && ! ps -A -o pid= -o ppid= >/dev/null 2>&1; then
    printf 'check.sh: cannot list processes (ps -A -o pid= -o ppid= failed); suites started by this run may keep running\n' >&2
  fi
  for round in 1 2 3; do
    now=$(queued_pids | tr '\n' ' ')
    [ -n "${now// /}" ] || break
    seen="$seen $now"
    # shellcheck disable=SC2086 # a list of pids
    kill $now 2>/dev/null || true
    sleep 0.1
  done
  for p in ${q_pids[@]+"${q_pids[@]}"}; do wait "$p" 2>/dev/null || true; done
  for p in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    left=
    for pid in $seen; do kill -0 "$pid" 2>/dev/null && left=1; done
    [ -n "$left" ] || break
    sleep 0.1
  done
  # shellcheck disable=SC2086
  [ -z "$left" ] || kill -9 $seen 2>/dev/null || true
  q_pids=()
  [ -z "$q_tmp" ] || rm -rf "$q_tmp"
  q_tmp=
}

# on_exit: the EXIT trap. A run that ends non-zero without `fail` (a step's
# command failed under set -e) still prints the step's seconds and the slowest
# steps; an interrupt (130, 143) prints neither.
on_exit() {
  local rc=$?
  trap - EXIT
  queue_cleanup
  case $rc in 0 | 130 | 143) ;; *)
    end_timing
    [ -n "$internal_step" ] || slowest_steps
    ;;
  esac
  exit "$rc"
}
trap on_exit EXIT

# run_queue: concurrent mode. Runs the queued steps, each in a child
# `check.sh --group scripts --internal-step NAME` with its output in a private
# (0700) directory, up to $jobs at a time. A step without --concurrent runs
# alone among the serial steps (never beside another one, though beside
# concurrent ones). Output prints as one block per step, in step order, as
# soon as every step before it has finished. The first failing step (in order)
# prints whole and fails the run, after the children still running finish.
run_queue() {
  local n=${#queue_names[@]} i rc took
  [ "$n" -gt 0 ] || return 0
  if [ -n "$queue_slot" ] && [ -z "$have_slot" ]; then
    have_slot=1
    . scripts/build-slots/acquire.sh
  fi
  q_tmp=$(mktemp -d "${TMPDIR:-/tmp}/oska-check.XXXXXX") || fail "cannot make a temporary directory"
  chmod 700 "$q_tmp"
  # states are indexed by the step's place in the queue.
  local state=() running=0 serial_running='' next=0 failed='' failed_rc=1 moved
  for ((i = 0; i < n; i++)); do state[i]=pending; done
  trap 'exit 130' INT
  trap 'exit 143' TERM
  while [ "$next" -lt "$n" ]; do
    moved=
    for ((i = 0; i < n; i++)); do
      [ "${state[i]}" = running ] || continue
      if [ ! -f "$q_tmp/$i.rc" ]; then
        # No result and no runner left (killed, out of memory): a failure,
        # not something to wait for. The result is checked again in case the
        # runner wrote it just before it ended.
        kill -0 "${q_pids[i]}" 2>/dev/null && continue
        if [ ! -f "$q_tmp/$i.rc" ]; then
          printf 'check.sh: the runner of step "%s" ended without a result\n' "${queue_names[i]}" >>"$q_tmp/$i.out"
          echo "1 0" >"$q_tmp/$i.rc"
        fi
      fi
      state[i]="done"
      running=$((running - 1))
      [ -z "${queue_conc[i]}" ] && serial_running=
      moved=1
    done
    while [ "$next" -lt "$n" ] && [ "${state[next]}" = "done" ]; do
      read -r rc took <"$q_tmp/$next.rc"
      cat "$q_tmp/$next.out"
      step_times="$step_times$took ${queue_names[next]}"$'\n'
      if [ "$rc" -ne 0 ] && [ -z "$failed" ]; then
        failed=${queue_names[next]}
        failed_rc=$rc
      fi
      [ -z "$failed" ] || break
      next=$((next + 1))
      moved=1
    done
    if [ -n "$failed" ]; then
      # Let the running children end, so none outlives the run.
      for ((i = 0; i < n; i++)); do
        [ "${state[i]}" = running ] && wait "${q_pids[i]}" 2>/dev/null || true
      done
      fail_rc "$failed_rc" "step \"$failed\" failed (its output is above)"
    fi
    for ((i = 0; i < n && running < jobs; i++)); do
      [ "${state[i]}" = pending ] || continue
      if [ -z "${queue_conc[i]}" ] && [ -n "$serial_running" ]; then continue; fi
      state[i]=running
      running=$((running + 1))
      [ -n "${queue_conc[i]}" ] || serial_running=1
      launch_step "$q_tmp" "$i" "${queue_names[i]}" &
      q_pids[i]=$!
      moved=1
    done
    [ -n "$moved" ] || sleep 0.2
  done
  trap - INT TERM
  queue_cleanup
}

# launch_step DIR I NAME: run in the background by run_queue. Writes DIR/I.out
# and, last, DIR/I.rc: "EXIT-CODE SECONDS".
launch_step() {
  local dir=$1 i=$2 name=$3 start=$SECONDS rc=0
  # run_queue's cleanup stops this subshell and everything below it; the
  # runner's pid is written down so it is found even if this subshell is gone.
  bash scripts/check.sh --group scripts --internal-step "$name" >"$dir/$i.out" 2>&1 &
  local runner=$!
  echo "$runner" >"$dir/$i.pid"
  wait "$runner" || rc=$?
  echo "$rc $((SECONDS - start))" >"$dir/$i.rc.tmp"
  mv "$dir/$i.rc.tmp" "$dir/$i.rc"
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
  run_queue
  end_timing
  if [ -n "$internal_step" ]; then
    exit 0
  fi
  slowest_steps
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

if step "shellcheck" --no-slot --fast-when '*.sh' --fast-when .githooks/pre-push --fast-when 'scripts/shellcheck/*'; then
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

if step "powershell parse" --no-slot --fast-when '*.ps1'; then
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
    scripts/temp-leak/run.sh cargo test --no-fail-fast $(fast_crates)
  elif [ -n "${OSKA_TEST_SHARD:-}" ]; then
    # CI's parallel shards of this step (scripts/test-shard.sh): the set is
    # the host's, "linux" or "macos". Every shard of a set together is the
    # whole `cargo test --all`.
    scripts/temp-leak/run.sh scripts/test-shard.sh "${OSKA_TEST_SET:?OSKA_TEST_SHARD needs OSKA_TEST_SET}" "$OSKA_TEST_SHARD"
  else
    scripts/temp-leak/run.sh cargo test --all --no-fail-fast
  fi
fi

if step "eval task set" --cargo --fast-when 'eval/*' --fast-when 'crates/oska-eval/*' --fast-when 'scripts/eval-tasks/*' --group scripts; then
  # The harness's `validate` over eval/tasks, with the oskac just built
  # (offline). The fast tier runs it when the task set or the harness changed;
  # a compiler change gets it from the full check and CI.
  scripts/eval-tasks/run.sh
fi

if step "eval batch driver tests" --fast-when 'scripts/eval-batch/*' --fast-when 'crates/oska-eval/*' --group scripts; then
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

if step "installer tests" --fast-when 'scripts/install/*' --concurrent --group scripts; then
  bash scripts/install/test.sh
fi

if step "release script tests" --fast-when 'scripts/release/*' --group scripts; then
  bash scripts/release/test.sh
fi

if step "release notes tests" --fast-when 'scripts/release/*' --concurrent --group scripts; then
  # scripts/release/notes.sh, the GitHub release body: the version's
  # CHANGELOG.md section and a footer; it fails closed.
  bash scripts/release/notes-test.sh
fi

if step "release image tests" --fast-when 'scripts/release/*' --concurrent --group scripts; then
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

if step "release image push tests" --fast-when 'scripts/release/*' --concurrent --group scripts; then
  # scripts/release/push-image.sh, with a fake skopeo: it pushes only the
  # image the signed release rebuilds to, and fails when the registry
  # serves another digest than the signed one.
  bash scripts/release/push-image-test.sh
fi

if step "Linux build check tests" --fast-when 'scripts/release/*' --concurrent --group scripts; then
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

if step "unsafe guard tests" --fast-when 'scripts/unsafe-guard/*' --concurrent --group scripts; then
  bash scripts/unsafe-guard/test.sh
fi

if step "intrinsic guard tests" --fast-when 'scripts/intrinsic-guard/*' --concurrent --group scripts; then
  bash scripts/intrinsic-guard/test.sh
fi

if step "SPDX guard tests" --fast-when 'scripts/spdx-guard/*' --concurrent --group scripts; then
  bash scripts/spdx-guard/test.sh
fi

if step "kill guard tests" --fast-when 'scripts/kill-guard/*' --fast-when .claude/settings.json --concurrent --group scripts; then
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

if step "workflow guard tests" --fast-when 'scripts/workflow-guard/*' --concurrent --group scripts; then
  bash scripts/workflow-guard/test.sh
fi

if step "temp-leak wrapper tests" --fast-when 'scripts/temp-leak/*' --concurrent --group scripts; then
  bash scripts/temp-leak/test.sh
fi

if step "rust-stable tests" --fast-when 'scripts/rust-stable/*' --concurrent --group scripts; then
  bash scripts/rust-stable/test.sh
fi

if step "cross-build manifest tests" --fast-when 'scripts/cross-build/*' --concurrent --group scripts; then
  bash scripts/cross-build/test.sh
fi

if step "cargo cache tests" --fast-when 'scripts/cargo-cache/*' --concurrent --group scripts; then
  bash scripts/cargo-cache/test.sh
fi

if step "build slot tests" --fast-when 'scripts/build-slots/*' --concurrent --group scripts; then
  bash scripts/build-slots/test.sh
fi

if step "pass stamp tests" --fast-when 'scripts/pass-stamp/*' --fast-when '.githooks/*' --concurrent --group scripts; then
  bash scripts/pass-stamp/test.sh
fi

if step "docs-test list tests" --fast-when 'scripts/docs-tests/*' --concurrent --group scripts; then
  # The Rust tests that read docs/, which a docs-only change runs.
  bash scripts/docs-tests/test.sh
fi

if step "test shard tests" --fast-when 'scripts/test-shard*' --fast-when '.github/workflows/ci.yml' --fast-when 'crates/*/tests/*' --concurrent --group scripts; then
  # Each test of a CI job's set runs in exactly one shard (scripts/test-shard.sh).
  bash scripts/test-shard/test.sh
fi

if step "ci changes tests" --no-slot --fast-when 'scripts/ci-changes/*' --concurrent --group scripts; then
  # Which pull requests skip the slow test jobs (scripts/ci-changes/docs-only.sh).
  bash scripts/ci-changes/test.sh
fi

if step "check step registry tests" --fast-when 'scripts/check-steps/*' --fast-when scripts/check.sh --concurrent --group scripts; then
  # --list prints exactly the steps a run runs, in order (this file's blocks).
  bash scripts/check-steps/test.sh
fi

if step "formal" --fast-when 'formal/*' --fast-when 'scripts/formal/*' --group formal; then
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

if step "formal step tests" --no-slot --fast-when 'formal/*' --fast-when 'scripts/formal/*' --group formal; then
  bash scripts/formal/test.sh
fi

if step "workflow validation tests" --no-slot --fast-when 'scripts/actionlint/*' --concurrent --group scripts; then
  bash scripts/actionlint/test.sh
fi

if step "x509 roots generator tests" --no-slot --fast-when 'scripts/x509-roots/*' --fast-when 'crates/oska-check/src/std/x509_roots.osk' --concurrent --group scripts; then
  bash scripts/x509-roots/test.sh
fi

if step "expected workflows tests" --no-slot --fast-when 'scripts/expected-workflows/*' --concurrent --group scripts; then
  bash scripts/expected-workflows/test.sh
fi

finish
