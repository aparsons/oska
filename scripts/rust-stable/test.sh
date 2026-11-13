#!/usr/bin/env bash
# Tests for latest.sh and sync-issue.sh. sync-issue.sh runs against a fake
# `gh` that serves a list of open issues and logs the calls that change any.
#
#   scripts/rust-stable/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

check() { # name expected actual
  if [ "$2" = "$3" ]; then
    passed=$((passed + 1))
  else
    failures=$((failures + 1))
    printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$1" "$2" "$3" >&2
  fi
}

# --- latest.sh
channel='date = "2026-11-09"
[pkg.cargo]
version = "1.99.0 (aaa 2026-10-01)"
[pkg.rustc]
version = "1.100.0 (0ef1bc386 2026-11-09)"
[pkg.rustc-docs]
version = "1.200.0 (zzz 2026-11-09)"
'
check "latest reads [pkg.rustc]" "1.100.0" "$(printf '%s' "$channel" | "$here/latest.sh")"
if printf 'date = "x"\n' | "$here/latest.sh" 2>/dev/null; then
  check "latest refuses a file with no rustc" "fail" "pass"
else
  check "latest refuses a file with no rustc" "fail" "fail"
fi

# --- sync-issue.sh, with a fake gh first on PATH
mkdir "$work/bin"
cat >"$work/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "issue list") cat "$FAKE_ISSUES" ;;
  *) printf '%s\n' "$*" >>"$FAKE_LOG" ;;
esac
EOF
chmod +x "$work/bin/gh"

# run <pin> <latest> <open issues, one "number<TAB>title" per line>; prints the logged gh calls.
run() {
  printf 'channel = "%s"\n' "$1" >"$work/toolchain.toml"
  printf '%b' "$3" >"$work/issues"
  : >"$work/log"
  PATH="$work/bin:$PATH" FAKE_ISSUES="$work/issues" FAKE_LOG="$work/log" \
    RUST_TOOLCHAIN_FILE="$work/toolchain.toml" "$here/sync-issue.sh" "$2" >/dev/null
  head -n 1 "$work/log" | sed 's/ is out.*//'
}

check "same version: nothing" "" "$(run 1.100.0 1.100.0 '')"
check "older latest: nothing" "" "$(run 1.100.0 1.99.0 '')"
check "1.9 is older than 1.10" "issue create --title Bump Rust to 1.10.0 --body Stable Rust 1.10.0" \
  "$(run 1.9.0 1.10.0 '')"
check "newer, no issue: opens one" \
  "issue create --title Bump Rust to 1.101.0 --body Stable Rust 1.101.0" "$(run 1.100.0 1.101.0 '7\tFix a bug\n')"
check "label is ready-for-agent" "yes" \
  "$(grep -q -- '--label ready-for-agent' "$work/log" && echo yes || echo no)"
check "open issue for it: nothing" "" "$(run 1.100.0 1.101.0 '9\tBump Rust to 1.101.0\n')"
check "older open issue: edited, not duplicated" \
  "issue edit 9 --title Bump Rust to 1.101.0 --body Stable Rust 1.101.0" \
  "$(run 1.100.0 1.101.0 '3\tOther\n9\tBump Rust to 1.100.1\n')"

printf '%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
