#!/usr/bin/env bash
# Tests for guard.sh: each case builds a small tree with a `std/rt.osk`, adds
# one way to use an intrinsic elsewhere, and expects the guard to refuse it
# (or, for the clean tree, to pass).
#
#   scripts/intrinsic-guard/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

# A clean tree in a new directory, which it prints.
tree() {
  local dir
  dir=$(mktemp -d "$work/tree.XXXXXX")
  mkdir -p "$dir/crates/oska-check/src/std" "$dir/app/src"
  printf 'pub fn rt_sys_write(fd: Int, buf: Int, count: Int) -> Int\n' \
    >"$dir/crates/oska-check/src/std/rt.osk"
  printf 'fn main() {}\n' >"$dir/app/src/main.osk"
  printf '%s\n' "$dir"
}

expect_pass() {
  local name=$1 dir=$2 out
  if out=$("$guard" "$dir" 2>&1); then
    passed=$((passed + 1))
  else
    printf 'FAIL %s: the guard refused a clean tree\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  fi
}

# Expect the guard to refuse `dir`, naming `file` in its output.
expect_refused() {
  local name=$1 dir=$2 file=$3 out
  if out=$("$guard" "$dir" 2>&1); then
    printf 'FAIL %s: the guard passed\n' "$name" >&2
    failures=$((failures + 1))
  elif ! grep -qF -- "$file" <<<"$out"; then
    printf 'FAIL %s: expected %s in\n%s\n' "$name" "$file" "$out" >&2
    failures=$((failures + 1))
  else
    passed=$((passed + 1))
  fi
}

dir=$(tree)
expect_pass "a clean tree" "$dir"

dir=$(tree)
printf 'fn main() {\n    rt_sys_exit_group(0)\n}\n' >"$dir/app/src/main.osk"
expect_refused "a system-call intrinsic in user code" "$dir" "app/src/main.osk:2:"

dir=$(tree)
printf 'fn f(a: Int) -> Int {\n    rt_mem_load64(a)\n}\n' >"$dir/app/src/mem.osk"
expect_refused "a memory intrinsic in a module" "$dir" "app/src/mem.osk:2:"

dir=$(tree)
printf 'use std.rt\n\nfn main() {}\n' >"$dir/app/src/main.osk"
expect_refused "use std.rt" "$dir" "app/src/main.osk:1:"

dir=$(tree)
printf 'use std . rt.{rt_sys_read}\n' >"$dir/app/src/main.osk"
expect_refused "use std . rt with spaces" "$dir" "app/src/main.osk:1:"

dir=$(tree)
printf '// call rt_sys_write here\nfn main() {}\n' >"$dir/app/src/main.osk"
expect_refused "a mention in a comment" "$dir" "app/src/main.osk:1:"

dir=$(tree)
printf 'pub fn x() -> Int {\n    rt_sys_read(0, 0, 0)\n}\n' >"$dir/crates/oska-check/src/std/other.osk"
expect_refused "another std module" "$dir" "crates/oska-check/src/std/other.osk:2:"

dir=$(tree)
mkdir -p "$dir/target/debug"
printf 'rt_sys_write\n' >"$dir/target/debug/built.osk"
expect_pass "a file under target/ is build output" "$dir"

dir=$(tree)
printf 'fn my_rt_sys_note() {}\nfn art_system() {}\nuse std.rtx\n' >"$dir/app/src/main.osk"
expect_pass "names that merely contain the letters" "$dir"

dir=$(tree)
rm "$dir/crates/oska-check/src/std/rt.osk"
expect_refused "std.rt's source missing" "$dir" "rt.osk is missing"

printf '%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
