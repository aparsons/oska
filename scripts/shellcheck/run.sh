#!/usr/bin/env bash
# ShellCheck over every shell script in the repo, at ShellCheck's default
# severity (all of error, warning, info and style). scripts/check.sh and the
# installers workflow both run this, so a local pass is a CI pass.
#
# Releases report different findings, so the version is pinned in
# scripts/shellcheck/version (CI installs it with install.sh). Another version
# fails in CI and warns elsewhere.
set -euo pipefail
cd "$(dirname "$0")/../.."

want=$(tr -d '[:space:]' <scripts/shellcheck/version)
have=$(shellcheck --version | sed -n 's/^version: *//p')
if [ "$have" != "$want" ]; then
  echo "shellcheck/run.sh: ShellCheck $have found, $want pinned (scripts/shellcheck/version)" >&2
  if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ]; then
    exit 1
  fi
fi

shopt -s nullglob
shellcheck scripts/*.sh scripts/*/*.sh site/agent-setup/*.sh .githooks/pre-push .claude/skills/*/*.sh .claude/skills/*/scripts/*.sh
