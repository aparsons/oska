#!/usr/bin/env bash
# Tests for guard.sh: each case builds a small workspace, a crate `app`
# and an `oska-sys` that uses `unsafe`, adds one way around the rule, and
# expects the guard to refuse it with the right reason. The cases include
# every bypass the spec #93 review found in the old grep-based guard.
#
#   scripts/unsafe-guard/test.sh

set -euo pipefail

# A git hook (such as .githooks/pre-push) exports GIT_DIR and related
# variables. Scripts that run git on their own scratch repositories must not
# inherit them, or `git init` re-initialises this repository instead (it once
# set core.bare=true here).
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_COMMON_DIR GIT_PREFIX

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard.sh"

if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

# A clean workspace in a new directory, which it prints.
workspace() {
  local dir
  dir=$(mktemp -d "$work/ws.XXXXXX")
  mkdir -p "$dir/crates/app/src" "$dir/crates/oska-sys/src" "$dir/.github/workflows"
  cat >"$dir/Cargo.toml" <<'EOF'
[workspace]
resolver = "2"
members = ["crates/app", "crates/oska-sys"]

[workspace.lints.rust]
unsafe_code = "forbid"
EOF
  cat >"$dir/crates/app/Cargo.toml" <<'EOF'
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
oska-sys = { path = "../oska-sys" }

[lints]
workspace = true
EOF
  printf 'pub fn one() -> u8 {\n    oska_sys::one()\n}\n' >"$dir/crates/app/src/lib.rs"
  cat >"$dir/crates/oska-sys/Cargo.toml" <<'EOF'
[package]
name = "oska-sys"
version = "0.1.0"
edition = "2021"
EOF
  cat >"$dir/crates/oska-sys/src/lib.rs" <<'EOF'
pub fn one() -> u8 {
    let x = 1u8;
    // SAFETY: `x` is a live local.
    unsafe { *(&x as *const u8) }
}
EOF
  printf 'name: CI\non: push\njobs: {}\n' >"$dir/.github/workflows/ci.yml"
  git -C "$dir" init -q
  printf '%s\n' "$dir"
}

# Expect the guard to pass on `dir`.
expect_pass() {
  local name=$1 dir=$2 out
  if out=$("$guard" "$dir" 2>&1); then
    passed=$((passed + 1))
  else
    printf 'FAIL %s: the guard refused a clean workspace\n%s\n' "$name" "$out" >&2
    failures=$((failures + 1))
  fi
}

# Expect the guard to refuse `dir`, saying each of the given reasons.
expect_refused() {
  local name=$1 dir=$2 out reason
  shift 2
  if out=$("$guard" "$dir" 2>&1); then
    printf 'FAIL %s: the guard passed\n' "$name" >&2
    failures=$((failures + 1))
    return
  fi
  for reason in "$@"; do
    if ! grep -qF -- "$reason" <<<"$out"; then
      printf 'FAIL %s: expected "%s" in\n%s\n' "$name" "$reason" "$out" >&2
      failures=$((failures + 1))
      return
    fi
  done
  passed=$((passed + 1))
}

compiled='does not compile with unsafe_code forbidden'
scanned='only crates/oska-sys may use unsafe'

dir=$(workspace)
expect_pass "a clean workspace" "$dir"

# The review's manifest: a decoy `[lints]` inside a multi-line string, and
# a table header written with spaces that allows unsafe code.
dir=$(workspace)
cat >"$dir/crates/app/Cargo.toml" <<'EOF'
[package]
name = "app"
version = "0.1.0"
edition = "2021"
description = """
[lints]
workspace = true
[end]"""

[ lints.rust ]
unsafe_code = "allow"
EOF
printf 'pub fn f() -> u8 {\n    let x = 1u8;\n    unsafe/**/{ *(&x as *const u8) }\n}\n' \
  >"$dir/crates/app/src/lib.rs"
expect_refused "a decoy [lints] and [ lints.rust ] allowing unsafe" "$dir" "$compiled" "$scanned"

dir=$(workspace)
printf 'pub fn f() -> u8 {\n    let x = 1u8;\n    unsafe/**/{ *(&x as *const u8) }\n}\n' \
  >"$dir/crates/app/src/lib.rs"
expect_refused "unsafe followed by a block comment" "$dir" "$compiled" "$scanned"

dir=$(workspace)
cat >"$dir/crates/app/src/lib.rs" <<'EOF'
macro_rules! trust_me {
    ($b:block) => {
        unsafe $b
    };
}

pub fn f() -> u8 {
    let x = 1u8;
    trust_me!({ *(&x as *const u8) })
}
EOF
expect_refused "unsafe in a macro_rules! body" "$dir" "$compiled" "$scanned"

dir=$(workspace)
cat >"$dir/crates/app/src/lib.rs" <<'EOF'
pub fn f() -> u8 {
    let x = 1u8;
    let _q = '"'; let y = unsafe { *(&x as *const u8) }; let _r = '"';
    y
}
EOF
expect_refused "unsafe between two '\"' character literals" "$dir" "$compiled" "$scanned"

dir=$(workspace)
cat >"$dir/crates/app/src/lib.rs" <<'EOF'
#![allow(unsafe_code)]

pub fn f() -> u8 {
    let x = 1u8;
    unsafe { *(&x as *const u8) }
}
EOF
expect_refused "#![allow(unsafe_code)] in source" "$dir" "$compiled" "$scanned"

dir=$(workspace)
mkdir -p "$dir/crates/app/tests"
printf '#[test]\nfn t() {\n    let x = 1u8;\n    assert_eq!(unsafe { *(&x as *const u8) }, 1);\n}\n' \
  >"$dir/crates/app/tests/t.rs"
expect_refused "unsafe in an integration test" "$dir" "$compiled" "$scanned"

dir=$(workspace)
printf '#[cfg(windows)]\npub fn f() -> u8 {\n    unsafe { g() }\n}\n' >>"$dir/crates/app/src/lib.rs"
expect_refused "unsafe in code this platform does not compile" "$dir" "$scanned"

dir=$(workspace)
mkdir -p "$dir/.cargo"
printf '[build]\nrustflags = ["--cap-lints", "warn"]\n' >"$dir/.cargo/config.toml"
printf 'pub fn f() -> u8 {\n    let x = 1u8;\n    unsafe { *(&x as *const u8) }\n}\n' \
  >"$dir/crates/app/src/lib.rs"
expect_refused "a cargo config capping lints" "$dir" "a cargo config file" "$compiled"

dir=$(workspace)
printf 'name: CI\non: push\nenv:\n  RUSTFLAGS: --cap-lints=warn\njobs: {}\n' \
  >"$dir/.github/workflows/ci.yml"
expect_refused "RUSTFLAGS set in a workflow" "$dir" "a workflow (above) sets rustc flags"

dir=$(workspace)
printf 'name: CI\non: push\nenv:\n  CARGO_ENCODED_RUSTFLAGS: -Aunsafe_code\njobs: {}\n' \
  >"$dir/.github/workflows/ci.yml"
expect_refused "CARGO_ENCODED_RUSTFLAGS set in a workflow" "$dir" "a workflow (above) sets rustc flags"

dir=$(workspace)
printf 'pub fn f() -> u8 {\n    let x = 1u8;\n    unsafe { *(&x as *const u8) }\n}\n' \
  >"$dir/crates/app/src/lib.rs"
if out=$(RUSTFLAGS="--cap-lints warn" "$guard" "$dir" 2>&1); then
  printf 'FAIL RUSTFLAGS capping lints: the guard passed\n' >&2
  failures=$((failures + 1))
elif ! grep -qF -- "which would let unsafe code through" <<<"$out"; then
  printf 'FAIL RUSTFLAGS capping lints: expected the wrapper to refuse --cap-lints in\n%s\n' "$out" >&2
  failures=$((failures + 1))
else
  passed=$((passed + 1))
fi

dir=$(workspace)
sed -i.bak 's/unsafe_code = "forbid"/unsafe_code = "deny"/' "$dir/Cargo.toml"
expect_refused "the workspace lint lowered to deny" "$dir" 'must keep unsafe_code = "forbid"'

printf '%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
