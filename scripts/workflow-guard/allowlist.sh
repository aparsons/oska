#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Keeps third-party actions out of the workflows. A workflow may use only the
# official actions (actions/*), dtolnay/rust-toolchain, and the two OpenSSF
# Scorecard actions named below, each pinned to a full 40-character commit SHA.
# A tag, a branch, a short SHA, another owner or another action of an allowed
# owner (ossf/scorecard-action is allowed, ossf/anything-else is not) fails.
#
#   scripts/workflow-guard/allowlist.sh [repo-root]
#
# Prints each refused `uses:` line and exits 1.

set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root"

# An official action may be one in a subdirectory of its repository, as
# actions/cache/restore: the same repository, at the same pinned commit.
sha_pin='@[0-9a-f]{40}'
allowed="(actions/[A-Za-z0-9_.-]+(/[A-Za-z0-9_-]+)?|dtolnay/rust-toolchain|ossf/scorecard-action|github/codeql-action/upload-sarif)"

bad=$(grep -rnE --include='*.yml' --include='*.yaml' '^[[:space:]]*-?[[:space:]]*uses:' .github/workflows |
  grep -vE "uses:[[:space:]]*$allowed$sha_pin([[:space:]]|$)" || true)
if [ -n "$bad" ]; then
  printf '%s\n' "$bad" >&2
  exit 1
fi
