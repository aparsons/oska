#!/usr/bin/env bash
# Tests for scripts/docs-tests/list.sh: the Rust tests that read docs/, and
# the docs/ files compiled into a crate. Most cases run the real list.sh on a
# scratch tree of small crates; the last ones run it on this repository, and
# check it finds the tests known to read docs/.
#
#   scripts/docs-tests/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

ok() {
  printf 'ok   %s\n' "$1"
  passed=$((passed + 1))
}
bad() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures=$((failures + 1))
}

# file PATH TEXT: writes TEXT to PATH under the scratch tree.
tree="$work/tree"
file() {
  mkdir -p "$(dirname "$tree/$1")"
  printf '%s\n' "$2" >"$tree/$1"
}

# The scratch tree: one crate, `kit`, whose directory name is not its
# package name, with a test of each kind.
file crates/kit-dir/Cargo.toml '[package]
name = "kit"'
file crates/kit-dir/src/lib.rs 'pub const HELP: &str = include_str!("../../../docs/help.md");'
file crates/kit-dir/src/other.rs '// see "docs/not-compiled.md"'
file crates/kit-dir/tests/includes.rs 'const D: &str = include_str!("../../../docs/design.md");'
file crates/kit-dir/tests/walks.rs 'fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn f() { read(root().join("docs")); }'
file crates/kit-dir/tests/fake_host.rs 'fn f() { FakeHost::new().with_file("docs/intro.md", "# Intro"); }'
file crates/kit-dir/tests/golden.rs 'fn f() { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"); }'
file crates/kit-dir/tests/common/mod.rs 'pub mod site;
pub mod plain;'
file crates/kit-dir/tests/common/site.rs 'pub fn real() { read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")); }'
file crates/kit-dir/tests/common/plain.rs 'pub fn helper() {}'
file crates/kit-dir/tests/uses_site.rs 'mod common;
use common::site;
fn f() { site::real(); }'
file crates/kit-dir/tests/uses_plain.rs 'mod common;
fn f() { common::plain::helper(); }'
file crates/kit-dir/tests/shared/mod.rs 'pub fn g() { git(["ls-files"]); read("docs/x.md"); }'
file crates/kit-dir/tests/uses_shared.rs 'mod shared;
fn f() { shared::g(); }'

# list ARGS...: runs list.sh on ROOT (default: the scratch tree). Sets
# `status` and `out`.
list() {
  local root=$tree
  if [ "${1:-}" = --repo ]; then
    root=$repo_root
    shift
  fi
  set +e
  out=$("$here/list.sh" "$@" "$root" 2>&1)
  status=$?
  set -e
}

list
if [ "$status" -eq 0 ]; then ok "list.sh exits 0"; else bad "list.sh exits 0" "exit $status: $out"; fi
expected='kit includes
kit uses_shared
kit uses_site
kit walks'
if [ "$out" = "$expected" ]; then
  ok "lists each test that reaches outside its crate and names docs/, sorted"
else
  bad "lists each test that reaches outside its crate and names docs/, sorted" "got: $out"
fi
for name in fake_host golden uses_plain; do
  if printf '%s\n' "$out" | grep -qx "kit $name"; then
    bad "does not list $name" "got: $out"
  else
    ok "does not list $name"
  fi
done

# A new test that reads docs/ joins the set with no list to update.
file crates/kit-dir/tests/new_reader.rs 'const G: &str = include_str!("../../../docs/guide/x.md");'
list
if printf '%s\n' "$out" | grep -qx "kit new_reader"; then
  ok "a new test that reads docs/ is listed"
else
  bad "a new test that reads docs/ is listed" "got: $out"
fi

list --compiled
if [ "$status" -eq 0 ] && [ "$out" = docs/help.md ]; then
  ok "--compiled lists the docs/ files a crate's own code includes"
else
  bad "--compiled lists the docs/ files a crate's own code includes" "exit $status: $out"
fi

# A crate's own code that reads docs/ at run time: every docs/ file counts.
file crates/kit-dir/src/runtime.rs 'fn f() { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs"); }'
list --compiled
if [ "$out" = docs ]; then
  ok "--compiled lists all of docs when a crate's code reads docs/ at run time"
else
  bad "--compiled lists all of docs when a crate's code reads docs/ at run time" "got: $out"
fi

list --bogus
if [ "$status" -eq 2 ]; then ok "an unknown option exits 2"; else bad "an unknown option exits 2" "exit $status: $out"; fi

# --- this repository ---------------------------------------------------------
# The tests known to read docs/ (issue #338), and two that do not.
list --repo
repo_set=$out
missing=
for test in "oska-syntax conformance" "oska-syntax fn_headers" "oska-check typed_corpus" \
  "oska-run docs" "oska-run site" "oskac catalog" "oskac agent_setup" "oskac guides"; do
  printf '%s\n' "$repo_set" | grep -qxF "$test" || missing="$missing '$test'"
done
if [ "$status" -eq 0 ] && [ -z "$missing" ]; then
  ok "finds this repository's known docs readers"
else
  bad "finds this repository's known docs readers" "exit $status, missing:$missing; got: $repo_set"
fi
for test in "oska-run file_capabilities" "oskac self_cli"; do
  if printf '%s\n' "$repo_set" | grep -qxF "$test"; then
    bad "does not list $test" "got: $repo_set"
  else
    ok "does not list $test"
  fi
done
list --repo --compiled
if [ "$out" = "docs/checker.md
docs/runtime.md" ]; then
  ok "finds the docs/ files oskac compiles in (oskac explain)"
else
  bad "finds the docs/ files oskac compiles in (oskac explain)" "got: $out"
fi

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
