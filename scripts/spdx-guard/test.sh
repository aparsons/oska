#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for guard.sh: each case builds a small git tree, and expects the guard
# to refuse it, naming the file, or (for a clean tree) to pass.
#
#   scripts/spdx-guard/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

# A clean tracked tree in a new directory, which it prints.
tree() {
  local dir
  dir=$(mktemp -d "$work/tree.XXXXXX")
  git -C "$dir" init -q
  mkdir -p "$dir/src"
  printf 'Oska\n' >"$dir/NOTICE"
  printf '// SPDX-License-Identifier: Apache-2.0\nfn main() {}\n' >"$dir/src/a.rs"
  printf '// SPDX-License-Identifier: Apache-2.0\nfn main() {}\n' >"$dir/src/a.osk"
  printf '#!/usr/bin/env bash\n# SPDX-License-Identifier: Apache-2.0\ntrue\n' >"$dir/run.sh"
  printf '# SPDX-License-Identifier: Apache-2.0\ntrue\n' >"$dir/lib.sh"
  printf 'not source\n' >"$dir/notes.txt"
  printf '%s\n' "$dir"
}

track() {
  git -C "$1" add -A
}

expect_pass() {
  local name=$1 dir=$2 out
  track "$dir"
  if out=$("$guard" "$dir" 2>&1); then
    passed=$((passed + 1))
  else
    printf 'FAIL %s: the guard refused a clean tree\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  fi
}

# Expect the guard to refuse `dir`, naming `what` in its output.
expect_refused() {
  local name=$1 dir=$2 what=$3 out
  track "$dir"
  if out=$("$guard" "$dir" 2>&1); then
    printf 'FAIL %s: the guard passed\n' "$name" >&2
    failures=$((failures + 1))
  elif ! grep -qF -- "$what" <<<"$out"; then
    printf 'FAIL %s: expected %s in\n%s\n' "$name" "$what" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

d=$(tree)
expect_pass "clean tree" "$d"

d=$(tree)
printf 'fn main() {}\n' >"$d/src/bare.rs"
expect_refused ".rs without the header" "$d" "src/bare.rs"

d=$(tree)
printf 'fn main() {}\n' >"$d/src/bare.osk"
expect_refused ".osk without the header" "$d" "src/bare.osk"

d=$(tree)
printf '#!/usr/bin/env bash\ntrue\n' >"$d/bare.sh"
expect_refused ".sh without the header" "$d" "bare.sh"

d=$(tree)
printf '# SPDX-License-Identifier: Apache-2.0\n#!/usr/bin/env bash\ntrue\n' >"$d/before.sh"
expect_refused ".sh with the header before the shebang" "$d" "before.sh"

d=$(tree)
printf '// SPDX-License-Identifier: MIT\nfn main() {}\n' >"$d/src/mit.rs"
expect_refused "a different license" "$d" "src/mit.rs"

d=$(tree)
printf 'fn main() {}\n' >"$d/src/late.rs"
printf '\n// SPDX-License-Identifier: Apache-2.0\n' >>"$d/src/late.rs"
expect_refused "header not at the top" "$d" "src/late.rs"

d=$(tree)
printf 'untracked.rs\n' >"$d/.gitignore"
printf 'fn main() {}\n' >"$d/untracked.rs"
expect_pass "an untracked file without the header is not checked" "$d"

d=$(tree)
rm "$d/NOTICE"
expect_refused "missing NOTICE" "$d" "NOTICE"

if [ "$failures" -ne 0 ]; then
  printf '%d failed, %d passed\n' "$failures" "$passed" >&2
  exit 1
fi
printf 'spdx-guard tests: %d passed\n' "$passed"
