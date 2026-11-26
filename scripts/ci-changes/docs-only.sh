#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Says whether a pull request changes only files that no test reads, so CI
# can skip the slow test jobs (spec #711, ticket #718).
#
#   scripts/ci-changes/docs-only.sh < changed-files
#
# Reads the changed paths, one a line, and prints `docs_only=true` or
# `docs_only=false` (the line a workflow step appends to $GITHUB_OUTPUT).
# It is true only when there is at least one path and every path is on the
# list below. Anything else, an empty list included, is false: when in doubt
# the tests run.
#
# The list is short on purpose. Tests read almost every other file:
#   docs/guide/**       guides.rs and native_tests.rs run its programs;
#   docs/** (the rest)  the real-site tests (oska-run site*.rs) build the
#                       site from docs/, GLOSSARY.md, README.md and
#                       CHANGELOG.md and check its links, and the checker
#                       and runtime docs are read by docs.rs and conformance;
#   site/**             that build's own inputs;
#   *.md in the root    README.md, GLOSSARY.md, CHANGELOG.md, CLAUDE.md and
#                       CONTRIBUTING.md
#                       are test inputs (all but CODING_STANDARDS.md);
#   .claude/**          the plugin and agent-setup tests read it.
# The site leaves out `docs/agents/` (site/build.osk `unpublished`), so no
# site test reads it. The lint and scripts jobs still check those files.

set -euo pipefail

docs_only=false
seen=0
while IFS= read -r path; do
  [ -n "$path" ] || continue
  seen=1
  case $path in
    docs/agents/* | CODING_STANDARDS.md) ;;
    *)
      printf 'docs_only=false\n'
      exit 0
      ;;
  esac
done
[ "$seen" -eq 1 ] && docs_only=true
printf 'docs_only=%s\n' "$docs_only"
