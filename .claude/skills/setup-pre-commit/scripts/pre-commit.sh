#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# A fast git pre-commit hook for a Rust workspace such as Oska. Install it as
# .githooks/pre-commit (executable), with `git config core.hooksPath .githooks`.
#
# It runs only checks that build nothing:
#   - `cargo fmt --all --check` on the workspace;
#   - `shellcheck` on each staged *.sh file, when shellcheck is installed.
# The full check stays in the pre-push hook (scripts/check.sh); this hook does
# not repeat it.
#
# It fails closed: a missing cargo or rustfmt refuses the commit. A missing
# ShellCheck is only a note, because scripts/check.sh and CI run it.
# It checks the files in the work tree, so stage the whole file to check what
# you commit. It never prints environment variables.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"

if ! command -v cargo >/dev/null 2>&1; then
  echo "pre-commit: cargo is not installed, so the format check cannot run; refusing the commit" >&2
  exit 1
fi
if ! cargo fmt --all --check; then
  echo "pre-commit: cargo fmt found unformatted code; run \`cargo fmt --all\`, stage it, and commit again" >&2
  exit 1
fi

scripts=()
while IFS= read -r -d '' f; do
  scripts+=("$f")
done < <(git diff --cached --name-only --diff-filter=ACMR -z -- '*.sh')

if [ "${#scripts[@]}" -gt 0 ]; then
  if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck -- "${scripts[@]}"; then
      echo "pre-commit: shellcheck found problems in the staged scripts (above)" >&2
      exit 1
    fi
  else
    echo "pre-commit: shellcheck is not installed; skipped it for ${#scripts[@]} staged script(s) (scripts/check.sh and CI run it)" >&2
  fi
fi
