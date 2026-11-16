#!/usr/bin/env bash
# ShellCheck over every shell script in the repo, at ShellCheck's default
# severity (all of error, warning, info and style). scripts/check.sh and the
# installers workflow both run this, so a local pass is a CI pass.
set -euo pipefail
cd "$(dirname "$0")/.."
shellcheck scripts/*.sh scripts/*/*.sh site/agent-setup/*.sh .githooks/pre-push
