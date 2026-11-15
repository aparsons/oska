#!/usr/bin/env bash
# The pass stamp: a record that scripts/check.sh passed on a tree, so the
# pre-push hook does not repeat a check that just passed.
#
#   scripts/pass-stamp/stamp.sh write
#       Records the tree of the working tree (tracked and untracked files,
#       not ignored ones: what `git add -A` would commit). scripts/check.sh
#       calls this once every step has passed.
#   scripts/pass-stamp/stamp.sh covers REMOTE [URL] < ref lines
#       Reads a pre-push hook's ref lines (githooks(5)) and exits 0 when the
#       push needs no check: the working tree is clean, and every commit
#       being pushed has the stamped tree. Exits 1 otherwise, and on anything
#       it cannot work out.
#
# The stamp is one tree hash in a file inside the clone's git directory
# (`git rev-parse --git-path oska-check-passed`). It is never committed, so
# it cannot travel to another machine.

set -euo pipefail

stamp_name=oska-check-passed
zero=0000000000000000000000000000000000000000

stamp_file() { git rev-parse --git-path "$stamp_name"; }

# Prints the tree hash of the working tree, using a scratch copy of the index
# so the real one is untouched.
working_tree() {
  local index scratch tree
  index=$(git rev-parse --git-path index)
  scratch=$(mktemp)
  trap 'rm -f "$scratch"' RETURN
  if [ -f "$index" ]; then cp "$index" "$scratch"; else rm -f "$scratch"; fi
  GIT_INDEX_FILE=$scratch git add -A
  tree=$(GIT_INDEX_FILE=$scratch git write-tree)
  printf '%s\n' "$tree"
}

write() {
  local file tree
  file=$(stamp_file)
  tree=$(cd "$(git rev-parse --show-toplevel)" && working_tree)
  printf '%s\n' "$tree" >"$file.tmp"
  mv "$file.tmp" "$file"
}

# Whether the pre-push on stdin needs no check.
covers() {
  local remote=${1:-} file stamped local_sha remote_sha commits commit
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
    for commit in $commits; do
      [ "$(git rev-parse "$commit^{tree}")" = "$stamped" ] || return 1
    done
  done
  return 0
}

case ${1:-} in
write) write ;;
covers)
  shift
  covers "$@"
  ;;
*)
  printf 'usage: %s write | covers REMOTE [URL] < ref lines\n' "$0" >&2
  exit 2
  ;;
esac
