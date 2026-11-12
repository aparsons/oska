#!/usr/bin/env bash
# Benchmark the native backend against the interpreter, on the machine it
# runs on: maintainer-run, on the Raspberry Pi 5 (aarch64 Linux), and not
# part of scripts/check.sh. The results are recorded in docs/design.md
# ("Native backend benchmark").
#
#   scripts/bench-native.sh [runs]
#
# Builds a release oskac, then for each workload runs `oskac run
# --interpret` and the executable `oskac build` wrote, `runs` times each
# (default 5), and prints the best CPU time of each and the speedup. The interpreter's time includes checking the program, as
# `oskac run` does; the native time is the executable alone. The
# workloads:
#
#   loop        a 5M-iteration integer loop
#   text        200k lines built by interpolation
#   bulk-text   replace and trim over 520 KB of text, 40 times: the
#               interpreter does these in Rust, so expect about 1x
#   lists       1M pushes, a pass with `for` and 1M indexed reads
#   recursion   fib(30), about 2.7M calls
#   site        the site generator building the documentation site
#               (`site/build.osk`, run from the repository root)

set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(uname -sm)" != "Linux aarch64" ]; then
  echo "bench-native.sh: native executables run only on aarch64 Linux" >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
fi

runs="${1:-5}"
root="$(pwd)"
oskac="$root/target/release/oskac"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cargo build --release --quiet -p oskac

cat >"$work/loop.osk" <<'EOF'
fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0
    var i = 0
    while i < 5000000 {
        total = total + i % 7
        i = i + 1
    }
    io.println("{total}")
}
EOF

cat >"$work/text.osk" <<'EOF'
fn main(io: Stdout) -> Result<(), IoError> {
    var total = 0
    for i in 0..200000 {
        let double = i * 2
        let line = "item {i}: {double}"
        total = total + line.len()
    }
    io.println("{total}")
}
EOF

cat >"$work/bulk-text.osk" <<'EOF'
fn main(io: Stdout) -> Result<(), IoError> {
    let text = "  the quick brown fox jumps over the lazy dog  ".repeat(11000)
    var total = 0
    for i in 0..40 {
        let changed = text.replace("fox", "cat").trim()
        total = total + changed.byte_len()
    }
    io.println("{total}")
}
EOF

cat >"$work/lists.osk" <<'EOF'
fn main(io: Stdout) -> Result<(), IoError> {
    var items: List<Int> = List.new()
    for i in 0..1000000 {
        items.push(i % 1000)
    }
    var total = 0
    for item in items {
        total = total + item
    }
    var i = 0
    while i < 1000000 {
        total = total + items.at((i * 7) % 1000000)
        i = i + 1
    }
    io.println("{total}")
}
EOF

cat >"$work/recursion.osk" <<'EOF'
fn fib(n: Int) -> Int {
    if n < 2 {
        return n
    }
    fib(n - 1) + fib(n - 2)
}

fn main(io: Stdout) -> Result<(), IoError> {
    let n = fib(30)
    io.println("{n}")
}
EOF

# The best CPU time (user plus system), in milliseconds, of `runs` runs of
# the command, in the current directory, with its output discarded. CPU
# time, not wall-clock time: a Pi doing other work, or writing to its SD
# card, stretches the wall clock of a short run several times over.
best() {
  local best_ms="" ms TIMEFORMAT='%3U %3S'
  for _ in $(seq "$runs"); do
    ms=$({ time "$@" >/dev/null 2>&1; } 2>&1 | awk '{ printf "%d", ($1 + $2) * 1000 }')
    if [ -z "$best_ms" ] || [ "$ms" -lt "$best_ms" ]; then
      best_ms=$ms
    fi
  done
  echo "$best_ms"
}

# Benchmark the program `file`, from the current directory.
bench() {
  local name="$1" file="$2" exe="$work/$1.exe" interpreted native
  "$oskac" build "$file" -o "$exe"
  # Both backends must agree before their times mean anything.
  if ! diff <("$oskac" run --interpret "$file" 2>&1) <("$exe" 2>&1) >/dev/null; then
    echo "bench-native.sh: $name: the backends' output differs" >&2
    exit 1
  fi
  interpreted=$(best "$oskac" run --interpret "$file")
  native=$(best "$exe")
  awk -v n="$name" -v i="$interpreted" -v e="$native" \
    'BEGIN { printf "| %-10s | %7d ms | %6d ms | %6.1fx |\n", n, i, e, i / (e > 0 ? e : 1) }'
}

echo "oskac $("$oskac" --version | cut -d' ' -f2), best of $runs runs, $(uname -m)"
echo
echo "| Workload   | Interpreter | Native |  Speedup |"
echo "| ---------- | ----------: | -----: | -------: |"
(cd "$work" && for name in loop text bulk-text lists recursion; do bench "$name" "$name.osk"; done)
"$oskac" check site/build.osk >site/authority.txt || true
bench site site/build.osk
