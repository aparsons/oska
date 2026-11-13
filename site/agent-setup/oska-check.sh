#!/bin/sh
# An opt-in Claude Code hook (a PostToolUse hook): after Claude Code edits
# a `.osk` file, run `oskac check --format=json` on it, and hand the report
# back to Claude Code if it has a diagnostic. A module of a package is
# checked with its package: the nearest directory above the file with an
# `oska.toml` and the file under its `src/`, as `oskac` finds it. Checked
# alone, a module reports errors its package does not have.
#
# It reads one thing, the JSON that Claude Code writes to its standard
# input, only to find the edited file's name. It runs `oskac check`, which
# reads the file and writes nothing. It makes no network request, edits no
# file, and changes no permission.
#
# Save it as `.claude/hooks/oska-check.sh`. POSIX sh; Linux and macOS.

input=$(cat)
file=$(printf '%s' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)

case "$file" in
*.osk) ;;
*) exit 0 ;;
esac

oskac="${OSKA_HOME:-$HOME/.oska}/bin/oskac"
if [ ! -x "$oskac" ]; then
  oskac=oskac
fi

target=$file
dir=$(dirname "$file")
while :; do
  if [ -f "$dir/oska.toml" ] && [ ! -L "$dir/oska.toml" ]; then
    case "$file" in
    "$dir"/src/*)
      target=$dir
      break
      ;;
    esac
  fi
  parent=$(dirname "$dir")
  if [ "$parent" = "$dir" ]; then
    break
  fi
  dir=$parent
done

if report=$("$oskac" check --format=json "$target" 2>&1); then
  exit 0
fi

# Exit status 2 shows standard error to Claude Code.
printf '%s\n' "$report" >&2
exit 2
