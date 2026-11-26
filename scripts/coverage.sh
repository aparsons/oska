#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Line coverage of the workspace's tests, from rustc's own source-based
# instrumentation and the LLVM tools of the pinned toolchain. No third-party
# crate or tool: Oska has no external dependencies.
#
#   scripts/coverage.sh [OUT_DIR] [-- CARGO_TEST_ARGS...]
#
# Runs `cargo test` (the whole workspace unless CARGO_TEST_ARGS say
# otherwise) in target/coverage, so the normal target directory keeps its
# uninstrumented build. Writes OUT_DIR (default target/coverage/report):
#   summary.md   a Markdown table, a row per crate, then the total
#   lcov.info    the lcov file, for any viewer
#   html/        the browsable report
# CI (.github/workflows/coverage.yml) runs it on every push to main and
# posts summary.md to the job summary. Needs the `llvm-tools` rustup
# component.

set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD

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
export LLVM_PROFILE_FILE=$root/target/coverage/raw/oska-%p-%m.profraw
rm -rf "$root/target/coverage/raw" "${root:?}/${out:?}"
mkdir -p "$root/target/coverage/raw" "$root/$out"

if [ $# -gt 0 ]; then
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
common=("$bin/llvm-cov")
args=(-instr-profile="$root/target/coverage/oska.profdata" -ignore-filename-regex="$ignore" "${objects[@]}")

"${common[@]}" export -format=lcov "${args[@]}" >"$root/$out/lcov.info"
"${common[@]}" show -format=html -output-dir="$root/$out/html" -show-line-counts-or-regions "${args[@]}" >/dev/null

# One row a crate: sum the lcov LF (lines found) and LH (lines hit).
awk -v root="$root/" '
  /^SF:/ { f = substr($0, 4); sub(root, "", f); split(f, p, "/"); crate = p[1] == "crates" ? p[2] : p[1] }
  /^LF:/ { lf[crate] += substr($0, 4); tf += substr($0, 4) }
  /^LH:/ { lh[crate] += substr($0, 4); th += substr($0, 4) }
  END {
    print "| Crate | Lines | Covered | Coverage |"
    print "| --- | ---: | ---: | ---: |"
    n = 0
    for (c in lf) names[++n] = c
    for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (names[j] < names[i]) { t = names[i]; names[i] = names[j]; names[j] = t }
    for (i = 1; i <= n; i++) { c = names[i]; printf "| %s | %d | %d | %.1f%% |\n", c, lf[c], lh[c], lf[c] ? 100 * lh[c] / lf[c] : 0 }
    printf "| **Total** | %d | %d | **%.1f%%** |\n", tf, th, tf ? 100 * th / tf : 0
  }' "$root/$out/lcov.info" >"$root/$out/summary.md"

cat "$root/$out/summary.md"
