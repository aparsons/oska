#!/usr/bin/env bash
# The formal step of scripts/check.sh (ADRs 0018, 0019): builds the Lean
# project in formal/lean and runs TLC on every TLA+ model in formal/tla.
#
#   scripts/formal/run.sh
#
# Exit 0: the proofs build and every model passes. Exit 1: a proof, a model
# or a check below failed. Exit 3: Lean or TLC is missing outside CI, which
# check.sh reports as a skip. With CI or GITHUB_ACTIONS set a missing tool
# exits 1 instead, so the gate cannot pass by skipping.
#
# Before it looks for any tool, it fails on an unfinished proof: the word
# `sorry` or `admit` anywhere in a .lean file, comments included. The build
# then runs with warnings as errors, so a `sorry` that a macro brings in
# fails too. It also fails on a .tla file with no .cfg beside it.
#
# Tools: `lake` and `lean` from PATH, else from ~/.cache/oska/formal/lean/bin
# (scripts/formal/install.sh puts them there); `java` from PATH; and the
# tla2tools.jar named by OSKA_TLA2TOOLS, else ~/.cache/oska/formal/
# tla2tools.jar. The jar must match its pinned checksum. A Lean of another
# version than formal/lean/lean-toolchain fails in CI and warns elsewhere
# (with elan, lake picks the pinned one itself). OSKA_FORMAL_DIR names
# another directory to check in place of formal/, for the tests.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.."
# shellcheck source=scripts/formal/pins.sh
. "$here/pins.sh"

root=${OSKA_FORMAL_DIR:-formal}
lean_dir=$root/lean
tla_dir=$root/tla
ci=${CI:-}${GITHUB_ACTIONS:-}

fail() {
  printf 'formal: %s\n' "$1" >&2
  exit 1
}

# --- checks that need no tool ---------------------------------------------

[ -f "$lean_dir/lakefile.toml" ] || fail "$lean_dir/lakefile.toml not found"
if unfinished=$(grep -rnwE --include='*.lean' --exclude-dir=.lake 'sorry|admit' "$lean_dir"); then
  printf '%s\n' "$unfinished" >&2
  fail "unfinished proof (sorry or admit) above; no proof may be left unfinished (ADR 0018)"
fi

models=()
for spec in "$tla_dir"/*.tla; do
  [ -e "$spec" ] || continue
  [ -f "${spec%.tla}.cfg" ] || fail "$spec has no ${spec%.tla}.cfg; every model needs one"
  models+=("$(basename "${spec%.tla}")")
done
[ ${#models[@]} -gt 0 ] || fail "no TLA+ model in $tla_dir"

# --- tools ------------------------------------------------------------------

if ! command -v lake >/dev/null 2>&1 && [ -x "$formal_cache/lean/bin/lake" ]; then
  PATH="$formal_cache/lean/bin:$PATH"
fi
jar=${OSKA_TLA2TOOLS:-$formal_cache/tla2tools.jar}
case $jar in /*) ;; *) jar=$PWD/$jar ;; esac
missing=
command -v lake >/dev/null 2>&1 || missing="$missing lake (Lean)"
command -v java >/dev/null 2>&1 || missing="$missing java"
[ -f "$jar" ] || missing="$missing $jar (TLC)"
if [ -n "$missing" ]; then
  if [ -n "$ci" ]; then
    fail "missing in CI:$missing; the CI job must run scripts/formal/install.sh"
  fi
  printf 'formal: not installed:%s (scripts/formal/install.sh installs Lean and TLC)\n' "$missing" >&2
  exit 3
fi

got=$(sha256sum "$jar" | cut -d' ' -f1)
[ "$got" = "$tla_sha256" ] || fail "$jar is not the pinned tla2tools.jar $tla_version (SHA-256 $got)"

# --- Lean -------------------------------------------------------------------

have=$(cd "$lean_dir" && lean --version 2>/dev/null) || have="no lean"
case $have in
  *"version $lean_version"[!0-9]* | *"version $lean_version") ;;
  *)
    printf 'formal: %s found, Lean %s pinned (formal/lean/lean-toolchain)\n' "$have" "$lean_version" >&2
    [ -z "$ci" ] || exit 1
    ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "lake build ($lean_dir)"
rc=0
(cd "$lean_dir" && lake build --wfail) >"$work/lake.out" 2>&1 || rc=$?
cat "$work/lake.out"
[ "$rc" -eq 0 ] || fail "lake build failed (exit $rc)"
if grep -q "declaration uses 'sorry'" "$work/lake.out"; then
  fail "a declaration uses sorry (above)"
fi

# --- TLC --------------------------------------------------------------------

# TLC writes its state and trace files beside the model, so each runs in a
# copy outside the checkout.
cp -R "$tla_dir" "$work/tla"
for model in "${models[@]}"; do
  echo "TLC $model ($tla_dir/$model.tla)"
  rc=0
  (cd "$work/tla" &&
    java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 \
      -metadir "$work/states/$model" -config "$model.cfg" "$model.tla") || rc=$?
  [ "$rc" -eq 0 ] || fail "TLC failed on $model (exit $rc)"
done
echo "formal: Lean build and ${#models[@]} TLA+ model(s) passed"
