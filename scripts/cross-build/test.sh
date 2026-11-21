#!/usr/bin/env bash
# Tests for manifest.sh: each case builds a scratch corpus with its own
# list of the programs expected refused, and runs the script with a fake
# `oskac` that refuses every program with `refused` in its name. A
# refusal the list does not expect must fail the script and show oskac's
# error: two hosts that refuse the same program would otherwise write the
# same manifest, and the comparison would pass.
#
#   scripts/cross-build/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
manifest="$here/manifest.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

# The fake `oskac build PROGRAM --target TARGET -o OUT`: OUT holds the
# program and the target.
oskac="$work/oskac"
cat >"$oskac" <<'SH'
#!/usr/bin/env bash
case "$2" in
  *refused*)
    printf 'error[E0390]: cannot compile `%s` for %s yet\n' "$2" "$4" >&2
    exit 2
    ;;
esac
printf '%s %s\n' "$2" "$4" >"$6"
SH
chmod +x "$oskac"

# A corpus in a new directory, which it prints: an example, an example
# package, a CLI fixture and the site generator, plus each program named
# after `expected`, the list of programs expected refused (printf %b).
tree() {
  local expected=$1 dir program
  shift
  dir=$(mktemp -d "$work/tree.XXXXXX")
  mkdir -p "$dir/examples/pkg" "$dir/crates/oskac/tests/fixtures/packages" "$dir/site" \
    "$dir/crates/oskac/tests/fixtures/secret_crypto" "$dir/scripts/cross-build"
  touch "$dir/examples/hello.osk" "$dir/examples/pkg/oska.toml" \
    "$dir/crates/oskac/tests/fixtures/tested.osk" "$dir/site/build.osk"
  for program in "$@"; do
    touch "$dir/$program"
  done
  printf '%b' "$expected" >"$dir/scripts/cross-build/refused.txt"
  printf '%s\n' "$dir"
}

# Expect the script to pass on `dir` with `line` in its manifest.
expect_pass() {
  local name=$1 dir=$2 line=$3 out
  if ! out=$("$manifest" "$oskac" "$dir" 2>&1); then
    printf 'FAIL %s: the script failed\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  elif ! grep -qxF -- "$line" <<<"$out"; then
    printf 'FAIL %s: expected the line "%s" in\n%s\n' "$name" "$line" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

# Expect the script to fail on `dir` with every fixed string after `dir`
# in its output.
expect_fail() {
  local name=$1 dir=$2 out message
  shift 2
  if out=$("$manifest" "$oskac" "$dir" 2>&1); then
    printf 'FAIL %s: the script passed\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
    return
  fi
  for message in "$@"; do
    if ! grep -qF -- "$message" <<<"$out"; then
      printf 'FAIL %s: expected "%s" in\n%s\n' "$name" "$message" "$out" >&2
      failures=$((failures + 1))
      return
    fi
  done
  passed=$((passed + 1))
}

# The hash the fake oskac's build of `program` for `target` has.
hash_of() {
  printf '%s %s\n' "$1" "$2" | sha256sum | cut -d' ' -f1
}

dir=$(tree '')
expect_pass "a source file is built for every target" "$dir" \
  "linux-x86_64 examples/hello.osk $(hash_of examples/hello.osk linux-x86_64)"
expect_pass "a package is built from its directory" "$dir" \
  "linux-aarch64 examples/pkg $(hash_of examples/pkg linux-aarch64)"

dir=$(tree '' crates/oskac/tests/fixtures/secret_crypto/sha256_secret.osk)
expect_pass "a secret crypto fixture is built for every target" "$dir" \
  "windows-x86_64 crates/oskac/tests/fixtures/secret_crypto/sha256_secret.osk $(hash_of crates/oskac/tests/fixtures/secret_crypto/sha256_secret.osk windows-x86_64)"

dir=$(tree '' examples/refused.osk)
# shellcheck disable=SC2016 # the backticks are literal error text
expect_fail "a refusal the list does not expect fails, showing the error" "$dir" \
  "examples/refused.osk" \
  'error[E0390]: cannot compile `examples/refused.osk` for linux-x86_64 yet'

dir=$(tree 'crates/oskac/tests/fixtures/refused.osk E0390\n' \
  examples/refused.osk crates/oskac/tests/fixtures/refused.osk)
expect_fail "an expected refusal does not excuse another" "$dir" "examples/refused.osk"

dir=$(tree '# a comment, then a blank line\n\nexamples/refused.osk E0390\n' examples/refused.osk)
expect_pass "an expected refusal is recorded by its exit code" "$dir" \
  "linux-aarch64 examples/refused.osk refused:2"

dir=$(tree 'examples/refused.osk E0201\n' examples/refused.osk)
expect_fail "a refusal with another code than the expected one fails" "$dir" \
  "examples/refused.osk" "E0201" "error[E0390]"

dir=$(tree 'examples/hello.osk E0201\n')
expect_fail "a program expected refused that builds fails" "$dir" \
  "examples/hello.osk" "refused.txt"

dir=$(tree 'examples/gone.osk E0201\n')
expect_fail "an expected refusal of no corpus program fails" "$dir" "examples/gone.osk"

dir=$(tree '')
rm "$dir/scripts/cross-build/refused.txt"
expect_fail "a missing list fails" "$dir" "refused.txt"

printf '%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
