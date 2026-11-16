#!/usr/bin/env bash
# Which Rust tests read docs/, derived from their source, so a test that
# starts reading docs/ joins the set with no list to update. check.sh runs
# just these when docs/ is the only cargo input a branch changes.
#
#   scripts/docs-tests/list.sh [ROOT]             "PACKAGE TEST" per line
#   scripts/docs-tests/list.sh --compiled [ROOT]  docs/ files a crate's own
#                                                 code needs (`docs` if all)
#   scripts/docs-tests/list.sh --check [ROOT]     the drift guard (below)
#
# ROOT is the repository (default: this one). Comment lines are ignored.
#
# A test target is crates/C/tests/NAME.rs. It reads docs/ when its code both
# reaches outside its crate (CARGO_MANIFEST_DIR, include_str!/include_bytes!
# or `git ls-files`) and names docs/ in a string: "docs", or a string with
# `docs/` or `/docs` in it ("../../docs/x.md", "{}/docs/x.md"). A test that
# only puts "docs/x" in a fake host's files reaches nothing. A shared module
# (tests/DIR/NAME.rs, or tests/NAME/mod.rs) that does both makes every target
# that names `NAME::` a reader too.
#
# --compiled covers crates' own code (src/, build.rs): a docs/ file it
# includes changes the build, so check.sh runs all of cargo when one
# changes. An include it cannot resolve, or code there that reads docs/ at
# run time, prints `docs`, meaning every docs/ file.
#
# --check fails, naming them, on the files in crates/ that name docs/ in a
# string and do any file I/O (fs::, read_to_string, read_dir, File::open,
# Command::new, include_*!, CARGO_MANIFEST_DIR) but that the list does not
# find: a docs/ reader in a form it does not see, such as a root that comes
# from another file. Classify each: make the list see it, or add it to
# scripts/docs-tests/not-readers.txt as "PATH  REASON".

set -euo pipefail
export LC_ALL=C

usage() {
  echo "usage: scripts/docs-tests/list.sh [--compiled | --check] [ROOT]" >&2
  exit 2
}

mode=tests
case ${1:-} in
  --compiled) mode=compiled && shift ;;
  --check) mode=check && shift ;;
  -*) usage ;;
esac
case ${1:-} in -*) usage ;; esac
root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root"

reaches='CARGO_MANIFEST_DIR|include_(str|bytes)!|ls-files'
names_docs='"[^"]*(\bdocs/|/docs\b)[^"]*"|"docs"'
does_io='CARGO_MANIFEST_DIR|include_(str|bytes)!|ls-files|fs::|read_to_string|read_dir|File::open|Command::new'
allowlist=scripts/docs-tests/not-readers.txt
manifest_concat='^include_(str|bytes)!\(concat!\(env!\("CARGO_MANIFEST_DIR"\), *"([^"]+)"\)\)$'

# code FILE: FILE without its comment lines.
code() { grep -vE '^[[:space:]]*//' "$1" || true; }

# reads_docs TEXT [REACHES]: whether TEXT reaches out and names docs/.
reads_docs() {
  grep -qE "${2:-$reaches}" <<<"$1" && grep -qE "$names_docs" <<<"$1"
}

# compiled_in CRATE FILE: the docs/ files FILE, a crate's own code, needs,
# as "compiled PATH FILE" lines (PATH `docs` for all of it).
compiled_in() {
  local crate=$1 file=$2 text path resolved
  text=$(code "$file")
  # Run-time reads, leaving out the include lines.
  if reads_docs "$(grep -v 'include_' <<<"$text" || true)" 'CARGO_MANIFEST_DIR|ls-files'; then
    echo "compiled docs $file"
  fi
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    path=${path#*\"}
    resolved=$(realpath -m --relative-to=. "$(dirname "$file")/${path%\"*}")
    case $resolved in docs/*) echo "compiled $resolved $file" ;; esac
  done < <(grep -oE 'include_(str|bytes)!\("[^"]+"\)' <<<"$text" || true)
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    if [[ $path =~ $manifest_concat ]]; then
      resolved=$(realpath -m --relative-to=. "$crate/${BASH_REMATCH[2]}")
      case $resolved in docs/*) echo "compiled $resolved $file" ;; esac
    elif grep -qE "$names_docs" <<<"$path"; then
      echo "compiled docs $file"
    fi
  done < <(grep -oE 'include_(str|bytes)!\(concat!\(.*\)\)' <<<"$text" || true)
}

# classify: one line per docs/ reader found, ending with its file:
#   test PACKAGE NAME FILE   a test target
#   shared FILE              a shared test module
#   compiled PATH FILE       src/ or build.rs needs PATH (or all of `docs`)
classify() {
  local crate package shared file name text
  for crate in crates/*/; do
    crate=${crate%/}
    package=$(sed -nE 's/^name = "([^"]+)".*/\1/p' "$crate/Cargo.toml" 2>/dev/null | head -n 1)
    while IFS= read -r file; do
      [ -n "$file" ] && compiled_in "$crate" "$file"
    done < <(find "$crate" \( -path "$crate/src/*" -name '*.rs' \) -o -path "$crate/build.rs" | sort)
    [ -d "$crate/tests" ] || continue
    # The shared modules that read docs/, by module name.
    shared=
    while IFS= read -r file; do
      [ -n "$file" ] || continue
      if reads_docs "$(code "$file")"; then
        echo "shared $file"
        name=$(basename "$file" .rs)
        [ "$name" = mod ] && name=$(basename "$(dirname "$file")")
        shared="$shared $name"
      fi
    done < <(find "$crate/tests" -mindepth 2 -name '*.rs' | sort)
    for file in "$crate"/tests/*.rs; do
      [ -f "$file" ] || continue
      text=$(code "$file")
      if reads_docs "$text"; then
        echo "test $package $(basename "$file" .rs) $file"
        continue
      fi
      for name in $shared; do
        if grep -qE "\\b$name::" <<<"$text"; then
          echo "test $package $(basename "$file" .rs) $file"
          break
        fi
      done
    done
  done
}

found=$(classify)

case $mode in
  tests)
    { awk '$1 == "test" { print $2, $3 }' <<<"$found" || true; } | sort
    ;;
  compiled)
    paths=$({ awk '$1 == "compiled" { print $2 }' <<<"$found" || true; } | sort -u)
    # `docs` (all of it) subsumes the single files.
    if grep -qx docs <<<"$paths"; then echo docs; elif [ -n "$paths" ]; then printf '%s\n' "$paths"; fi
    ;;
  check)
    seen=$(awk '{ print $NF }' <<<"$found" | sort -u)
    allowed=
    if [ -f "$allowlist" ]; then
      while read -r path reason; do
        case $path in '' | '#'*) continue ;; esac
        if [ -z "$reason" ]; then
          echo "$allowlist: $path has no reason" >&2
          exit 1
        fi
        allowed="$allowed$path"$'\n'
      done <"$allowlist"
    fi
    unclassified=
    while IFS= read -r file; do
      text=$(code "$file")
      if grep -qE "$names_docs" <<<"$text" && grep -qE "$does_io" <<<"$text" &&
        ! grep -qxF "$file" <<<"$seen" && ! grep -qxF "$file" <<<"$allowed"; then
        unclassified="$unclassified  $file"$'\n'
      fi
    done < <(find crates -name '*.rs' | sort)
    if [ -n "$unclassified" ]; then
      printf 'docs-tests: these name docs/ and do file I/O, but list.sh does not find them as docs/ readers:\n%s' "$unclassified" >&2
      printf 'Make list.sh see them, or add each to %s as "PATH  REASON".\n' "$allowlist" >&2
      exit 1
    fi
    ;;
esac
