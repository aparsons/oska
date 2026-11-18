#!/usr/bin/env bash
# The pass stamp: a record that a full run of scripts/check.sh passed on a
# tree, so the pre-push hook does not repeat a check that just passed.
#
#   scripts/pass-stamp/stamp.sh tree
#       Prints the tree of the working tree (tracked and untracked files,
#       not ignored ones: what `git add -A` would commit). scripts/check.sh
#       takes it before its first step.
#   scripts/pass-stamp/stamp.sh write TIER TREE
#       Records that a run of tier TIER (full or fast) passed on TREE, once
#       every step has passed, but only if the working tree is still exactly
#       TREE. If it changed during the run, or TREE is empty, it writes no
#       stamp and says so. It never fails: the stamp is an optimisation, not
#       part of the check.
#   scripts/pass-stamp/stamp.sh covers REMOTE [URL] < ref lines
#       Reads a pre-push hook's ref lines (githooks(5)) and exits 0 when the
#       push needs no check: the stamp is from a full run, the working tree
#       is clean, and every ref the push does not delete has the stamped tree
#       at its tip, whether or not it brings new commits (a rewind, or an
#       older commit pushed to a branch, counts too). For a tag that is the
#       tree of the commit it points to. A deletion pushes no tree, so it adds
#       no condition. Commits below a tip are not checked: the check only ever
#       runs on one tree, and CI tests the tip too. Exits 1 otherwise, and on
#       anything it cannot work out.
#
# The stamp is a file inside the clone's git directory
# (`git rev-parse --git-path oska-check-passed`), so each worktree has its
# own. It holds two lines, `tier=TIER` and `tree=TREE`. Only a stamp from a
# full run is honoured; a fast run's, or one in any other form, never is. It
# is never committed, so it cannot travel to another machine.

set -euo pipefail

stamp_name=oska-check-passed
zero=0000000000000000000000000000000000000000

stamp_file() { git rev-parse --git-path "$stamp_name"; }

# Prints the tree hash of the working tree, using a scratch index so the real
# one is untouched. read-tree fills the scratch index from HEAD with no file
# stat data, so `git add -A` hashes every file afresh: the result never rests
# on mtimes or git's racy-clean rules, and an edit of the same size made in
# the same instant as the last index write is not missed.
working_tree() {
  local scratch tree
  scratch=$(mktemp)
  trap 'rm -f "$scratch"' RETURN
  rm -f "$scratch"
  if git rev-parse -q --verify HEAD >/dev/null; then
    GIT_INDEX_FILE=$scratch git read-tree HEAD
  else
    GIT_INDEX_FILE=$scratch git read-tree --empty
  fi
  GIT_INDEX_FILE=$scratch git add -A
  tree=$(GIT_INDEX_FILE=$scratch git write-tree)
  printf '%s\n' "$tree"
}

# Names the files that make the working tree differ from HEAD (at most 20),
# and says where check logs belong. A message only: it changes no decision.
# A log written into the worktree is the usual cause.
say_dirty() {
  local files count
  files=$(git status --porcelain 2>/dev/null) || return 0
  [ -n "$files" ] || return 0
  count=$(printf '%s\n' "$files" | wc -l)
  printf '  untracked or modified files:\n'
  printf '%s\n' "$files" | head -n 20 | sed 's/^/    /'
  if [ "$count" -gt 20 ]; then
    printf '    ... and %d more (%d in all)\n' $((count - 20)) "$count"
  fi
  # shellcheck disable=SC2016 # the text names the variable, it is not expanded
  printf '  If any is a check log or result file, move it outside the worktree (for example under %s), delete it, and check again.\n' '${TMPDIR:-/tmp}'
}

write() {
  local tier=${1:-} checked=${2:-} file current
  case $tier in
    full | fast) ;;
    *)
      echo "pass stamp: not written, unknown tier '$tier'"
      return 0
      ;;
  esac
  if [ -z "$checked" ]; then
    echo "pass stamp: not written, the tree was not known when the check began"
    return 0
  fi
  if ! current=$(cd "$(git rev-parse --show-toplevel)" && working_tree) 2>/dev/null; then
    echo "pass stamp: not written, the working tree could not be read"
    return 0
  fi
  if [ "$current" != "$checked" ]; then
    echo "pass stamp: not written, files changed while the check ran"
    say_dirty
    return 0
  fi
  file=$(stamp_file)
  printf 'tier=%s\ntree=%s\n' "$tier" "$checked" >"$file.tmp"
  mv "$file.tmp" "$file"
}

# Prints the stamped tree when the stamp is exactly a full run's; fails
# otherwise.
full_run_tree() {
  local file tier tree
  file=$(stamp_file)
  [ -s "$file" ] || return 1
  {
    IFS= read -r tier || return 1
    IFS= read -r tree || return 1
    if IFS= read -r _; then return 1; fi
  } <"$file"
  [ "$tier" = tier=full ] || return 1
  tree=${tree#tree=}
  [[ $tree =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] || return 1
  printf '%s\n' "$tree"
}

# Whether the pre-push on stdin needs no check.
covers() {
  local stamped local_sha tree changes
  stamped=$(full_run_tree) || return 1
  # A failing git status must not read as a clean tree.
  changes=$(git status --porcelain) || return 1
  if [ -n "$changes" ]; then
    echo "pre-push: ignoring the pass stamp, the working tree is not clean" >&2
    say_dirty >&2
    return 1
  fi
  while read -r _ local_sha _ _; do
    # A deletion pushes no tree.
    [ "$local_sha" != "$zero" ] || continue
    # Any other ref, new commits or not, must point at the checked tree.
    # ^{tree} peels an annotated tag to its commit's tree.
    tree=$(git rev-parse -q --verify "$local_sha^{tree}") || return 1
    [ "$tree" = "$stamped" ] || return 1
  done
  return 0
}

case ${1:-} in
tree) (cd "$(git rev-parse --show-toplevel)" && working_tree) ;;
write)
  shift
  write "$@"
  ;;
covers)
  shift
  covers "$@"
  ;;
*)
  printf 'usage: %s tree | write TIER TREE | covers REMOTE [URL] < ref lines\n' "$0" >&2
  exit 2
  ;;
esac
