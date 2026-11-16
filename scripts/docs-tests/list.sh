#!/usr/bin/env bash
# Which Rust tests read docs/, derived from their source, so a test that
# starts reading docs/ joins the set with no list to update. check.sh runs
# just these when docs/ is the only cargo input a branch changes.
#
#   scripts/docs-tests/list.sh [ROOT]             "PACKAGE TEST" per line
#   scripts/docs-tests/list.sh --compiled [ROOT]  docs/ files a crate's own
#                                                 code needs (`docs` if all)
#
# ROOT is the repository (default: this one). Comment lines are ignored.
#
# A test target is crates/C/tests/NAME.rs. It reads docs/ when its code both
# reaches outside its crate (CARGO_MANIFEST_DIR, include_str!/include_bytes!
# or `git ls-files`) and names docs/ in a string ("docs", "docs/...",
# "../../docs/..."): a test that only puts "docs/x" in a fake host's files
# reaches nothing. A shared module (tests/DIR/NAME.rs, or tests/NAME/mod.rs)
# that does both makes every target that names `NAME::` a reader too.
#
# Not seen: a path to docs/ assembled from pieces with no "docs" string, and
# a walk over the whole repository. --compiled covers crates' own code
# (src/): a docs/ file it includes changes the build, so check.sh runs all
# of cargo when one changes; code there that reads docs/ at run time prints
# `docs`, meaning every docs/ file.

set -euo pipefail
export LC_ALL=C

compiled=
if [ "${1:-}" = --compiled ]; then
  compiled=1
  shift
fi
case ${1:-} in
  -*)
    echo "usage: scripts/docs-tests/list.sh [--compiled] [ROOT]" >&2
    exit 2
    ;;
esac
root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root"

reaches='CARGO_MANIFEST_DIR|include_(str|bytes)!|ls-files'
names_docs='"(\.\./)*docs(/[^"]*)?"'

# code FILE: FILE without its comment lines.
code() { grep -vE '^[[:space:]]*//' "$1" || true; }

# reads_docs FILE [REACHES]: whether FILE reaches out and names docs/.
reads_docs() {
  local text
  text=$(code "$1")
  grep -qE "${2:-$reaches}" <<<"$text" && grep -qE "$names_docs" <<<"$text"
}

if [ -n "$compiled" ]; then
  for file in $(find crates \( -path '*/src/*' -name '*.rs' \) -o -path 'crates/*/build.rs' | sort); do
    # Run-time reads: every docs/ file may matter.
    if reads_docs "$file" 'CARGO_MANIFEST_DIR|ls-files'; then
      echo docs
    fi
    { grep -oE 'include_(str|bytes)!\("[^"]+"\)' <<<"$(code "$file")" || true; } | sed -E 's/^[^"]*"([^"]+)".*/\1/' |
      while IFS= read -r path; do
        resolved=$(realpath -m --relative-to=. "$(dirname "$file")/$path")
        case $resolved in docs/*) echo "$resolved" ;; esac
      done
  done | sort -u | {
    # `docs` (all of it) subsumes the single files.
    found=$(cat)
    if grep -qx docs <<<"$found"; then echo docs; elif [ -n "$found" ]; then printf '%s\n' "$found"; fi
  }
  exit 0
fi

for crate in crates/*/; do
  crate=${crate%/}
  [ -d "$crate/tests" ] || continue
  package=$(sed -nE 's/^name = "([^"]+)".*/\1/p' "$crate/Cargo.toml" | head -n 1)
  # The shared modules that read docs/, by module name.
  shared=
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    if reads_docs "$file"; then
      name=$(basename "$file" .rs)
      [ "$name" = mod ] && name=$(basename "$(dirname "$file")")
      shared="$shared $name"
    fi
  done < <(find "$crate/tests" -mindepth 2 -name '*.rs' | sort)
  for file in "$crate"/tests/*.rs; do
    [ -f "$file" ] || continue
    reader=
    if reads_docs "$file"; then
      reader=1
    else
      for name in $shared; do
        if grep -qE "\\b$name::" <<<"$(code "$file")"; then
          reader=1
          break
        fi
      done
    fi
    if [ -n "$reader" ]; then echo "$package $(basename "$file" .rs)"; fi
  done
done | sort
