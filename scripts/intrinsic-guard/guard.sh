#!/usr/bin/env bash
# Keeps the system-call and memory intrinsics inside `std.rt` (ADR 0013), the
# way scripts/unsafe-guard keeps `unsafe` inside oska-sys. The checker rejects
# them elsewhere with E0209; this scan holds even if the checker has a hole.
#
#   scripts/intrinsic-guard/guard.sh [repo-root]
#
# An intrinsic is any identifier starting `rt_sys_` or `rt_mem_` (the prefixes
# are reserved; see crates/oska-check/src/toolchain.rs). Every `.osk` file in
# the tree except std.rt's own source must contain none, and none may import
# `std.rt`. The scan reads raw text, so a mention in a comment or a string
# counts too. It does not depend on git, so untracked files are scanned.
# It skips build output (target/) and .git.

set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root"

allowed=crates/oska-check/src/std/rt.osk
names='(^|[^A-Za-z0-9_])rt_(sys|mem)_[A-Za-z0-9_]*'
import='(^|[^A-Za-z0-9_])std[[:space:]]*\.[[:space:]]*rt([^A-Za-z0-9_]|$)'

if [ ! -f "$allowed" ]; then
  printf 'intrinsic-guard: %s is missing: std.rt is where the intrinsics live\n' "$allowed" >&2
  exit 1
fi

found=0
while IFS= read -r -d '' file; do
  [ "$file" = "./$allowed" ] && continue
  if grep -nE -e "$names" -e "$import" -- "$file" | sed "s#^#${file#./}:#"; then
    found=1
  fi
done < <(find . \( -name target -o -name .git \) -prune -o -type f -name '*.osk' -print0)

if [ "$found" -ne 0 ]; then
  printf 'intrinsic-guard: the lines above name a std.rt intrinsic or import std.rt; only %s may (ADR 0013)\n' "$allowed" >&2
  exit 1
fi
