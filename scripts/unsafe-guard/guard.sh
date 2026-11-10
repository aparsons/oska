#!/usr/bin/env bash
# Prove that only crates/oska-sys uses `unsafe` (ADR 0009). check.sh runs
# it on this repo; test.sh runs it on small workspaces that try each way
# around it.
#
#   scripts/unsafe-guard/guard.sh [workspace]
#
# Checks, reporting every failure before it exits non-zero:
#   1. the workspace Cargo.toml keeps `unsafe_code = "forbid"`;
#   2. no cargo config file is checked in, and no workflow sets RUSTFLAGS,
#      a rustc wrapper or `--cap-lints`, any of which can lower lint
#      levels where this script cannot see it;
#   3. the proof: every workspace crate but oska-sys, every target, is
#      compiled with `-F unsafe_code` last on rustc's command line
#      (rustc.sh), so no manifest, source attribute or flag can allow it;
#   4. a backstop for code this platform does not compile, such as
#      `#[cfg(windows)]` blocks: Rust source outside crates/oska-sys,
#      with comments, string and character literals removed, must not
#      contain the word `unsafe` (a word in backticks is prose).

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cd "${1:-$here/../..}"

# Rust may be installed for interactive shells only.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi

failed=0
problem() {
  printf 'unsafe guard: %s\n' "$1" >&2
  failed=1
}

if ! grep -qx 'unsafe_code = "forbid"' Cargo.toml; then
  problem 'the workspace Cargo.toml must keep unsafe_code = "forbid"'
fi

configs=$(git ls-files -co --exclude-standard -- ':(glob)**/.cargo/config' ':(glob)**/.cargo/config.toml')
if [ -n "$configs" ]; then
  printf '%s\n' "$configs" >&2
  problem "a cargo config file (above) can set rustflags or a wrapper that lowers lint levels; remove it"
fi

lint_overrides='RUSTFLAGS|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|cap-lints'
if [ -d .github/workflows ]; then
  overrides=$(grep -nE "$lint_overrides" .github/workflows/*.yml || true)
  if [ -n "$overrides" ]; then
    printf '%s\n' "$overrides" >&2
    problem "a workflow (above) sets rustc flags or a wrapper, which can lower lint levels"
  fi
fi

# A target directory of its own: the wrapper changes every fingerprint,
# and would otherwise rebuild the main one each time.
if ! RUSTC_WORKSPACE_WRAPPER="$here/rustc.sh" \
  cargo check --quiet --workspace --all-targets --all-features --target-dir target/unsafe-guard; then
  problem "a crate other than oska-sys does not compile with unsafe_code forbidden (above)"
fi

unsafe_word='(^|[^[:alnum:]_`])unsafe([^[:alnum:]_`]|_code|$)'
strip="s:/\\*([^*]|\\*+[^*/])*\\*+/::g; s/'([^'\\\\]|\\\\.[^']*)'//g; s/\"([^\"\\\\]|\\\\.)*\"//g; s://.*\$::"
found=$(git ls-files -co --exclude-standard -- '*.rs' ':(exclude)crates/oska-sys/*' |
  while IFS= read -r file; do
    sed -E "$strip" "$file" | grep -nE "$unsafe_word" | sed "s|^|$file:|" || true
  done)
if [ -n "$found" ]; then
  printf '%s\n' "$found" >&2
  problem "only crates/oska-sys may use unsafe or name the unsafe_code lint (above)"
fi

exit "$failed"
