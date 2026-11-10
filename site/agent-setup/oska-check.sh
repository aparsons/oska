#!/bin/sh
# An opt-in Claude Code hook (a PostToolUse hook): after Claude Code edits
# a `.osk` file, run `oskac check --format=json` on it, and hand the report
# back to Claude Code if it has a diagnostic.
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

if report=$("$oskac" check --format=json "$file" 2>&1); then
  exit 0
fi

# Exit status 2 shows standard error to Claude Code.
printf '%s\n' "$report" >&2
exit 2
