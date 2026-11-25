#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# One shard of a CI job's cargo tests. CI runs the tests of the musl, Linux
# and macOS jobs as parallel shards, so a pull request waits for the slowest
# shard, not for every test. Every test runs in exactly one shard of its set,
# so splitting drops no test.
#
#   scripts/test-shard.sh SET SHARD           run the shard's tests
#   scripts/test-shard.sh SET SHARD --plan    print what it would run, run nothing
#   scripts/test-shard.sh --sets              print "SET SHARD", one pair a line
#
# A SET is one CI job's tests:
#   musl   oska-sys, oska-run and part of oskac, built for $TARGET (the musl
#          target; ci.yml sets it)
#   linux  every test of the workspace, for the host (the Linux test jobs)
#   macos  the tests of oskac (the macOS jobs)
#   windows  every test of oska-run and oskac (the Windows job)
#
# A plan is one line a target: "PACKAGE lib", "PACKAGE bins", "PACKAGE doc",
# "PACKAGE test NAME" or "PACKAGE test site/K" (K of the SITE_SPLIT slices of
# oska-run's tests/site.rs, the slowest test binary, by position in its sorted
# test list). The shard `rest` of a set holds, by exclusion, every target that
# no other shard names, so a new test file lands in `rest` with no change
# here. A set may name `rest-1`, `rest-2` instead (REST_SPLIT of them, Windows):
# each takes every REST_SPLIT-th of those leftover targets, by position. `all` runs the whole set in one shard (the macOS arm64 job).
# ci.yml's matrices name the same shards as `--sets`;
# scripts/test-shard/test.sh checks that every target runs exactly once.

set -euo pipefail

cd "$(dirname "$0")/.."

SITE_SPLIT=3
# Windows: the leftover targets are split into this many `rest-K` shards.
REST_SPLIT=2

# The shards of each set, then each shard's heavy targets, "PACKAGE:NAME".
shards_of() {
  case $1 in
    musl) echo "site-0 site-1 site-2 sites-1 sites-2 rest all" ;;
    linux) echo "site native rest all" ;;
    macos) echo "native-1 native-2 rest all" ;;
    windows) echo "native heavy rest-1 rest-2 all" ;;
    *) return 1 ;;
  esac
}
heavy_of() { # SET SHARD
  case "$1 $2" in
    "musl sites-1") echo "oska-run:site_api oska-run:site_home oska-run:site_meta" ;;
    "musl sites-2") echo "oska-run:site_links oska-run:site_corpus oska-run:site_compliance" ;;
    "linux site") echo "oska-run:site oska-run:site_api oska-run:site_links oska-run:site_corpus oska-run:site_compliance oska-run:site_home oska-run:site_meta" ;;
    "linux native") echo "oskac:native_tests oskac:native_prelude oskac:build_cache oskac:run_limits oskac:native_corpus oskac:run_limits_steps_native oskac:native_sshsig" ;;
    "windows native") echo "oskac:native_tests" ;;
    "windows heavy") echo "oskac:native_prelude oskac:native_corpus oskac:native_tls oskac:native_ints" ;;
    "macos native-1") echo "oskac:native_prelude oskac:native_data oskac:native_system_calls" ;;
    "macos native-2") echo "oskac:native_tests oskac:run_limits oskac:run_limits_steps_native" ;;
  esac
}
# Targets that a shard of a set splits by position: "PACKAGE:NAME".
split_of() { [ "$1" = musl ] && echo "oska-run:site"; }

# Every target of a set, one "PACKAGE KIND NAME" line each: what `all` and,
# less the heavy ones, `rest` run.
universe() { # SET
  local dir pkg f
  case $1 in
    musl) set -- oska-sys oska-run ;;
    macos) set -- oskac ;;
    windows) set -- oska-run oskac ;;
    *) mapfile -t pkgs < <(ls crates); set -- "${pkgs[@]}" ;;
  esac
  for pkg in "$@"; do
    dir=crates/$pkg
    [ -f "$dir/src/lib.rs" ] && printf '%s lib\n%s doc\n' "$pkg" "$pkg"
    [ -f "$dir/src/main.rs" ] && printf '%s bins\n' "$pkg"
    for f in "$dir"/tests/*.rs; do
      [ -e "$f" ] && printf '%s test %s\n' "$pkg" "$(basename "$f" .rs)"
    done
  done
}

plan() { # SET SHARD
  local set=$1 shard=$2 h pkg kind name
  case $shard in
    site-[0-9]*)
      echo "oska-run test site/${shard#site-}"
      return
      ;;
  esac
  case $shard in all | rest | rest-[0-9]*) is_rest=1 ;; *) is_rest= ;; esac
  if [ -n "$is_rest" ]; then
    local all s heavy=" " n=0
    for s in $(shards_of "$set"); do
      case $s in all | rest | rest-[0-9]*) continue ;; esac
      for h in $(heavy_of "$set" "$s"); do heavy="$heavy$h "; done
    done
    for h in $(split_of "$set"); do heavy="$heavy$h "; done
    all=$(universe "$set")
    if [ "$set" = musl ]; then
      # oskac: only the differential suite and its own unit tests run on musl.
      all="$all"$'\n'"oskac test native_files"$'\n'"oskac test native_parity"$'\n'"oskac test native_default"$'\n'"oskac bins"
    fi
    while read -r pkg kind name; do
      [ -n "$pkg" ] || continue
      if [ "$kind" = test ] && [ "$shard" != all ]; then
        case "$heavy" in *" $pkg:$name "*) continue ;; esac
      fi
      if [ "$shard" != all ] && [ "$shard" != rest ]; then
        # rest-K: the K-th of the N slices of what is left, by position.
        n=$((n + 1))
        [ $(((n - 1) % REST_SPLIT)) -eq $((${shard#rest-} - 1)) ] || continue
      fi
      echo "$pkg $kind${name:+ $name}"
    done <<<"$all"
    return
  fi
  for h in $(heavy_of "$set" "$shard"); do
    echo "${h%%:*} test ${h#*:}"
  done
}

usage() { echo "usage: scripts/test-shard.sh SET SHARD [--plan] | --sets" >&2; exit 2; }

if [ "${1:-}" = --sets ]; then
  for set in musl linux macos windows; do
    for shard in $(shards_of "$set"); do echo "$set $shard"; done
  done
  exit 0
fi
[ $# -ge 2 ] || usage
set=$1 shard=$2 mode=${3:-run}
[ "$mode" = run ] || [ "$mode" = --plan ] || usage
case " $(shards_of "$set" 2>/dev/null || true) " in *" $shard "*) ;; *) echo "unknown set or shard: $set $shard" >&2; exit 2 ;; esac

lines=$(plan "$set" "$shard")
if [ "$mode" = --plan ]; then
  printf '%s\n' "$lines"
  exit 0
fi

cargo_test() {
  if [ "$set" = musl ]; then
    : "${TARGET:?TARGET must name the musl target}"
    cargo test --locked --no-fail-fast --target "$TARGET" "$@"
  else
    cargo test --locked --no-fail-fast "$@"
  fi
}

# One cargo invocation for each package's lib, bins and test files; doc tests
# (which cannot share an invocation) and the position-split site.rs apart.
run_package() { # PACKAGE LINES
  local pkg=$1 lines=$2 flags=() p kind name
  while read -r p kind name; do
    [ "$p" = "$pkg" ] || continue
    case $kind in
      lib) flags+=(--lib) ;;
      bins) flags+=(--bins) ;;
      test)
        case $name in
          site/*) run_site "${name#site/}" ;;
          *) flags+=(--test "$name") ;;
        esac
        ;;
      doc) cargo_test -p "$pkg" --doc ;;
    esac
  done <<<"$lines"
  [ ${#flags[@]} -eq 0 ] || cargo_test -p "$pkg" "${flags[@]}"
}

run_site() { # K
  local k=$1 all names=() i
  mapfile -t all < <(cargo_test -p oska-run --test site -- --list 2>/dev/null | sed -n 's/: test$//p' | sort)
  for i in "${!all[@]}"; do
    [ $((i % SITE_SPLIT)) -eq "$k" ] && names+=("${all[$i]}")
  done
  [ ${#names[@]} -gt 0 ] || { echo "no site.rs tests listed for slice $k" >&2; exit 1; }
  cargo_test -p oska-run --test site -- --exact "${names[@]}"
}

for pkg in $(printf '%s\n' "$lines" | awk '{print $1}' | awk '!s[$0]++'); do
  run_package "$pkg" "$lines"
done
