#!/usr/bin/env bash
# Keeps one open "Bump Rust to X.Y.Z" issue for the latest stable Rust when
# the pin in rust-toolchain.toml is older. Uses only `gh` (with GH_TOKEN).
#
#   scripts/rust-stable/sync-issue.sh <latest-stable-version>
#
#   pin is not older than latest        nothing
#   an open issue already names latest  nothing
#   an open issue names an older one    its title and body move to latest
#   otherwise                           opens one, labelled ready-for-agent

set -euo pipefail

cd "$(dirname "$0")/../.."

latest=${1:?usage: sync-issue.sh <latest-stable-version>}
[[ "$latest" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "sync-issue.sh: '$latest' is not a version" >&2
  exit 2
}

pinned=$(sed -n -E 's/^channel = "([0-9]+\.[0-9]+\.[0-9]+)"/\1/p' "${RUST_TOOLCHAIN_FILE:-rust-toolchain.toml}")
[ -n "$pinned" ] || {
  echo "sync-issue.sh: no X.Y.Z channel in rust-toolchain.toml" >&2
  exit 1
}

# Newer only if latest sorts after the pin.
if [ "$latest" = "$pinned" ] || [ "$(printf '%s\n%s\n' "$pinned" "$latest" | sort -V | tail -n 1)" != "$latest" ]; then
  echo "Rust $pinned is pinned; latest stable is $latest. Nothing to do."
  exit 0
fi

title="Bump Rust to $latest"
body="Stable Rust $latest is out; the repository pins $pinned.

Run \`scripts/bump-rust.sh $latest\` and open a PR. It sets the version in \`rust-toolchain.toml\`, regenerates the Unicode tables in \`crates/oska-check/src/std/rt.osk\` and runs \`scripts/check.sh\`. If a newer Clippy fails the check, fix the code it names. See \"Upgrading Rust\" in \`docs/agents/rust.md\`."

open=$(gh issue list --state open --limit 100 --search "Bump Rust to in:title" \
  --json number,title --jq '.[] | "\(.number)\t\(.title)"')

stale=""
while IFS=$'\t' read -r number existing; do
  [ -n "$number" ] || continue
  if [ "$existing" = "$title" ]; then
    echo "Issue #$number already asks for Rust $latest."
    exit 0
  fi
  if [[ "$existing" =~ ^Bump\ Rust\ to\ [0-9]+\.[0-9]+\.[0-9]+$ ]] && [ -z "$stale" ]; then
    stale=$number
  fi
done <<<"$open"

if [ -n "$stale" ]; then
  gh issue edit "$stale" --title "$title" --body "$body"
  echo "Issue #$stale now asks for Rust $latest."
else
  gh issue create --title "$title" --body "$body" --label ready-for-agent
fi
