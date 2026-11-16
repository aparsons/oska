#!/usr/bin/env bash
# The pass stamp: a record that scripts/check.sh passed on a tree, so the
# pre-push hook does not repeat a check that just passed.
#
#   scripts/pass-stamp/stamp.sh tree
#       Prints the tree of the working tree (tracked and untracked files,
#       not ignored ones: what `git add -A` would commit). scripts/check.sh
#       takes it before its first step.
#   scripts/pass-stamp/stamp.sh write TREE
#       Records TREE, the tree that was checked, once every step has passed,
#       but only if the working tree is still exactly TREE. If it changed
#       during the run, or TREE is empty, it writes no stamp and says so.
#       It never fails: the stamp is an optimisation, not part of the check.
#   scripts/pass-stamp/stamp.sh covers REMOTE [URL] < ref lines
#       Reads a pre-push hook's ref lines (githooks(5)) and exits 0 when the
#       push needs no check: the working tree is clean, and every ref that
#       pushes new commits has the stamped tree at its tip. Commits below a
#       tip are not checked: the check only ever runs on one tree, and CI
#       tests the tip too. Exits 1 otherwise, and on anything it cannot work
#       out.
#
# The stamp is one tree hash in a file inside the clone's git directory
# (`git rev-parse --git-path oska-check-passed`), so each worktree has its
# own. It is never committed, so it cannot travel to another machine.

set -euo pipefail

stamp_name=oska-check-passed
zero=0000000000000000000000000000000000000000

stamp_file() { git rev-parse --git-path "$stamp_name"; }

# Prints the tree hash of the working tree, using a scratch copy of the index
# so the real one is untouched. The copy keeps the index's mtime (`cp -p`):
# git rehashes a file whose index entry is as new as the index file ("racily
# clean"), and a copy stamped with the current time would hide that, so an
# edit of the same size made in the same instant as the last index write
# would be missed.
working_tree() {
  local index scratch tree
  index=$(git rev-parse --git-path index)
  scratch=$(mktemp)
  trap 'rm -f "$scratch"' RETURN
  if [ -f "$index" ]; then cp -p "$index" "$scratch"; else rm -f "$scratch"; fi
  GIT_INDEX_FILE=$scratch git add -A
  tree=$(GIT_INDEX_FILE=$scratch git write-tree)
  printf '%s\n' "$tree"
}

write() {
  local checked=${1:-} file current
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
    return 0
  fi
  file=$(stamp_file)
  printf '%s\n' "$checked" >"$file.tmp"
  mv "$file.tmp" "$file"
}

# Whether the pre-push on stdin needs no check.
covers() {
  local remote=${1:-} file stamped local_sha remote_sha commits
  file=$(stamp_file)
  [ -s "$file" ] || return 1
  stamped=$(cat "$file")
  [ -z "$(git status --porcelain)" ] || return 1
  while read -r _ local_sha _ remote_sha; do
    # Deleting a ref pushes no commits.
    [ "$local_sha" != "$zero" ] || continue
    if [ "$remote_sha" = "$zero" ]; then
      # A new ref: the commits the remote's tracking refs do not have.
      commits=$(git rev-list "$local_sha" --not --remotes="$remote") || return 1
    else
      git cat-file -e "$remote_sha^{commit}" 2>/dev/null || return 1
      commits=$(git rev-list "$local_sha" "^$remote_sha") || return 1
    fi
    # A ref with no new commits (a tag on a pushed commit, say) needs none.
    [ -n "$commits" ] || continue
    [ "$(git rev-parse "$local_sha^{tree}")" = "$stamped" ] || return 1
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
  printf 'usage: %s tree | write TREE | covers REMOTE [URL] < ref lines\n' "$0" >&2
  exit 2
  ;;
esac
