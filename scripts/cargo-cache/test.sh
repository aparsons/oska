#!/usr/bin/env bash
# Tests for mtimes.sh, with real cargo builds of a one-crate project in a
# scratch git repository. A CI cache restore is played by copying a
# `target` directory, times kept, into a fresh clone. The script must let
# cargo reuse a build from the same tree, and must never let it reuse one
# from a changed file: that would test a stale build.
#
#   scripts/cargo-cache/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
mtimes="$here/mtimes.sh"
toolchain="$here/../../rust-toolchain.toml"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR

passed=0
failures=0

ok() {
  printf 'ok   %s\n' "$1"
  passed=$((passed + 1))
}

bad() {
  printf 'FAIL %s\n%s\n' "$1" "$2" >&2
  failures=$((failures + 1))
}

git_quiet() {
  git -c user.name=test -c user.email=test@example.invalid -c init.defaultBranch=main \
    -c advice.detachedHead=false "$@" >/dev/null 2>&1
}

# The origin: a binary crate that prints a number from its library.
origin="$work/origin"
mkdir -p "$origin/src"
cp "$toolchain" "$origin/rust-toolchain.toml"
cat >"$origin/Cargo.toml" <<'TOML'
[package]
name = "probe"
version = "0.1.0"
edition = "2021"
TOML
printf 'pub fn number() -> u32 {\n    1\n}\n' >"$origin/src/lib.rs"
printf 'fn main() {\n    println!("{}", probe::number());\n}\n' >"$origin/src/main.rs"
printf 'notes\n' >"$origin/notes.txt"
ln -s notes.txt "$origin/link.txt"
git_quiet -C "$origin" init
git_quiet -C "$origin" add -A
git_quiet -C "$origin" commit -m one

# A fresh clone of the origin's HEAD in `dir`: every file has the time of
# the clone, as after actions/checkout.
clone() {
  git_quiet clone "$origin" "$1"
}

# cargo build in `dir`; prints what cargo printed.
build() {
  (cd "$1" && cargo build --offline 2>&1)
}

# 1. No cached build: the script changes no time and writes the list.
first="$work/first"
clone "$first"
out=$(cd "$first" && "$mtimes" 2>&1)
old=$(cd "$first" && find . -path ./.git -prune -o -type f ! -newermt 2001-01-01 -print)
if [ -f "$first/target/.cargo-cache-sources" ] && [ -z "$old" ] &&
  printf '%s' "$out" | grep -q 'no cached build'; then
  ok "with no cached build, no file time changes and the list is written"
else
  bad "with no cached build, no file time changes and the list is written" "$out"
fi
if ! out=$(build "$first"); then
  bad "the first build succeeds" "$out"
fi

sleep 1

# 2. The same tree in a new clone, with the first build's target: cargo
# compiles nothing.
same="$work/same"
clone "$same"
cp -Rp "$first/target" "$same/target"
out=$(cd "$same" && "$mtimes" 2>&1)
if printf '%s' "$out" | grep -q 'cargo-cache: 6 of 6 tracked files unchanged'; then
  ok "it counts the unchanged files"
else
  bad "it counts the unchanged files" "$out"
fi
out=$(build "$same")
if ! printf '%s' "$out" | grep -q 'Compiling'; then
  ok "an unchanged tree reuses the cached build"
else
  bad "an unchanged tree reuses the cached build" "$out"
fi

# 3. A changed source in a new clone, with the first build's target: cargo
# compiles again, and the program prints the new number.
printf 'pub fn number() -> u32 {\n    2\n}\n' >"$origin/src/lib.rs"
printf 'changed notes\n' >"$origin/notes.txt"
git_quiet -C "$origin" commit -am two
changed="$work/changed"
clone "$changed"
cp -Rp "$first/target" "$changed/target"
(cd "$changed" && "$mtimes" >/dev/null 2>&1)
out=$(build "$changed")
if printf '%s' "$out" | grep -q 'Compiling probe'; then
  ok "a changed source builds again"
else
  bad "a changed source builds again" "$out"
fi
number=$("$changed/target/debug/probe")
if [ "$number" = 2 ]; then
  ok "the build from a changed source runs the new code"
else
  bad "the build from a changed source runs the new code" "printed $number"
fi

# 4. A symbolic link that did not change never passes the old time to its
# target, which did.
old_time=$(cd "$changed" && find notes.txt -newermt 2001-01-01 | wc -l | tr -d ' ')
if [ "$old_time" = 1 ]; then
  ok "an unchanged symbolic link leaves its changed target's time alone"
else
  bad "an unchanged symbolic link leaves its changed target's time alone" "notes.txt got the old time"
fi

# 5. The list is rewritten for the current tree.
if (cd "$changed" && git ls-files -s | LC_ALL=C sort | cmp -s - target/.cargo-cache-sources); then
  ok "the list names the current tree"
else
  bad "the list names the current tree" "$(cat "$changed/target/.cargo-cache-sources")"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
