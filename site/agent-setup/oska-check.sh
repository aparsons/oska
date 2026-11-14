#!/bin/sh
# An opt-in Claude Code hook (a PostToolUse hook): after Claude Code edits
# a `.osk` file or an `oska.toml`, check it and hand the diagnostics back
# to Claude Code. A module of a package is checked with its package: the
# nearest directory above the file with an `oska.toml` and the file under
# its `src/`, as `oskac` finds it. Checked alone, a module reports errors
# its package does not have.
#
# The work is `oskac hook claude`'s, which runs the same on Linux, macOS
# and Windows: this script only calls it, for a project that saved the
# script earlier. The published settings call `oskac` directly, so a new
# setup needs no script. The command reads one thing, the JSON that Claude
# Code writes to its standard input, and the program's files. It writes no
# file, starts no program, makes no network request, and changes no
# permission.
#
# Save it as `.claude/hooks/oska-check.sh`. POSIX sh; `oskac` on PATH.

exec oskac hook claude --event=post-edit
