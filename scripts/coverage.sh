#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Line coverage of the workspace's tests, from rustc's own source-based
# instrumentation and the LLVM tools of the pinned toolchain. No third-party
# crate or tool: Oska has no external dependencies.
#
#   scripts/coverage.sh [--shard NAME] [OUT_DIR] [-- CARGO_TEST_ARGS...]
#
# Runs the tests in target/coverage, so the normal target directory keeps its
# uninstrumented build, and writes OUT_DIR (default target/coverage/report):
#   lcov.info    the lcov file of the run
#   summary.md   a table a crate and a table a file (a full run only)
# With --shard NAME it runs one linux shard of scripts/test-shard.sh (site,
# native or rest), or, when OSKA_SHARD_LINES holds a plan, just that plan
# (scripts/coverage-plan.py cuts the shards finer), and writes only
# lcov.info: the Coverage workflow runs the pieces in parallel jobs, because
# instrumented one shard takes longer than GitHub allows, and
# scripts/coverage-merge.py merges the files.
# Without it, it runs `cargo test --workspace` (or CARGO_TEST_ARGS) and
# writes both files. Needs the `llvm-tools` rustup component.

set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD

shard=
if [ "${1:-}" = --shard ]; then
  shard=${2:?usage: coverage.sh --shard NAME [OUT_DIR]}
  shift 2
fi
out=target/coverage/report
if [ $# -gt 0 ] && [ "$1" != -- ]; then
  out=$1
  shift
fi
[ "${1:-}" = -- ] && shift

host=$(rustc -vV | sed -n 's/^host: //p')
bin=$(rustc --print sysroot)/lib/rustlib/$host/bin
for tool in llvm-profdata llvm-cov; do
  [ -x "$bin/$tool" ] || {
    echo "coverage.sh: $bin/$tool is missing; run: rustup component add llvm-tools" >&2
    exit 1
  }
done

export CARGO_TARGET_DIR=$root/target/coverage/build
export RUSTFLAGS="${RUSTFLAGS:-} -C instrument-coverage"
# %8m: each process merges its counters into one of eight files for its
# binary as it exits. A file for each process (%p) would be thousands of
# profraw files, one for every oskac the tests run.
export LLVM_PROFILE_FILE=$root/target/coverage/raw/oska-%8m.profraw
rm -rf "$root/target/coverage/raw" "${root:?}/${out:?}"
mkdir -p "$root/target/coverage/raw" "$root/$out"

if [ -n "$shard" ]; then
  scripts/test-shard.sh linux "$shard"
elif [ $# -gt 0 ]; then
  cargo test "$@"
else
  cargo test --workspace
fi

"$bin/llvm-profdata" merge -sparse -o "$root/target/coverage/oska.profdata" \
  "$root"/target/coverage/raw/*.profraw

# Every executable cargo built: test binaries, and the oskac and oska-eval
# binaries the tests spawn. Cargo's target layout differs by version, so
# search the whole target directory rather than name a deps/ directory.
objects=()
while IFS= read -r f; do
  objects+=(-object "$f")
done < <(find "$CARGO_TARGET_DIR/debug" -type f -perm -u+x \
  ! -name '*.so' ! -name '*.d' ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.sh' \
  ! -path '*/incremental/*' ! -path '*/examples/*' ! -name 'build[-_]script*' | sort -u)

ignore='/\.cargo/|/rustc/|/tests/|/target/|/build\.rs$'
"$bin/llvm-cov" export -format=lcov \
  -instr-profile="$root/target/coverage/oska.profdata" \
  -ignore-filename-regex="$ignore" "${objects[@]}" >"$root/$out/lcov.info"

if [ -z "$shard" ]; then
  python3 -I scripts/coverage-merge.py "$root/$out" "$root/$out/lcov.info"
  cat "$root/$out/summary.md"
fi
