#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# One shard of CI's musl job: its share of the oska-sys, oska-run and oskac
# tests, built and run against the musl target. Every test runs in exactly one
# shard, so splitting the job cuts its wall time and drops no test.
#
#   scripts/musl-shard.sh SHARD    SHARD is one of the names below
#   scripts/musl-shard.sh --list   print the shard names, one a line
#
# The three `site-N` shards split tests/site.rs, the slowest test binary
# (about 450 s on musl), by position in its sorted test list: test i runs in
# shard i mod 3. The `sites-*` shards hold the other slow site_*.rs binaries.
# `rest` runs everything else, found by exclusion, so a new test file lands
# there with no change here. ci.yml's matrix must name every shard.
#
# The target comes from $TARGET (ci.yml sets it).

set -euo pipefail

cd "$(dirname "$0")/.."

shards="site-0 site-1 site-2 sites-1 sites-2 rest"
site_shards=3
sites_1="site_api site_home site_meta"
sites_2="site_links site_corpus site_compliance"

if [ "${1:-}" = "--list" ]; then
  printf '%s\n' $shards
  exit 0
fi
[ $# -eq 1 ] || { echo "usage: scripts/musl-shard.sh SHARD|--list" >&2; exit 2; }
shard=$1
case " $shards " in *" $shard "*) ;; *) echo "unknown shard: $shard" >&2; exit 2 ;; esac
: "${TARGET:?TARGET must name the musl target}"

cargo_test() { cargo test --locked --target "$TARGET" "$@"; }

# `--test a --test b` for the names given.
test_flags() { local n; for n in "$@"; do printf -- '--test\n%s\n' "$n"; done; }

case $shard in
  site-*)
    k=${shard#site-}
    # Test names of site.rs, sorted; this shard takes every third.
    mapfile -t all < <(cargo_test -p oska-run --test site -- --list 2>/dev/null |
      sed -n 's/: test$//p' | sort)
    names=()
    for i in "${!all[@]}"; do
      [ $((i % site_shards)) -eq "$k" ] && names+=("${all[$i]}")
    done
    [ ${#names[@]} -gt 0 ] || { echo "no site.rs tests listed for $shard" >&2; exit 1; }
    cargo_test -p oska-run --test site -- --exact "${names[@]}"
    ;;
  sites-1 | sites-2)
    var=${shard/-/_}
    # shellcheck disable=SC2086 # the lists are words
    mapfile -t flags < <(test_flags ${!var})
    cargo_test -p oska-run "${flags[@]}"
    ;;
  rest)
    heavy=" site $sites_1 $sites_2 "
    others=()
    for f in crates/oska-run/tests/*.rs; do
      n=$(basename "$f" .rs)
      case "$heavy" in *" $n "*) ;; *) others+=("$n") ;; esac
    done
    mapfile -t flags < <(test_flags "${others[@]}")
    cargo_test -p oska-sys
    # The library's unit tests, the doc tests and every other integration test.
    cargo_test -p oska-run --lib "${flags[@]}"
    cargo_test -p oska-run --doc
    # The differential suite, the native backend against the interpreter,
    # run by this `oskac`, so its interpreter runs on musl's allocator.
    cargo_test -p oskac --test native_files --test native_parity --test native_default
    # oskac's own unit tests, among them the check that the musl build, and
    # no other, uses that allocator (crates/oskac/src/heap.rs).
    cargo_test -p oskac --bin oskac
    ;;
esac
