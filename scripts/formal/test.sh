#!/usr/bin/env bash
# Tests for the formal step (scripts/formal/run.sh) that need neither Lean nor
# TLC: without the tools it skips (exit 3) outside CI and fails in CI; an
# unfinished proof or a model with no .cfg fails before any tool is looked
# for; a tla2tools.jar that is not the pinned one fails. Each case runs
# run.sh on a copy of formal/ with a PATH that holds only the basic
# utilities it needs (and, where a case says so, stub tools), and an empty
# cache, so the machine's own tools cannot leak in.
#
#   scripts/formal/test.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
ok() { printf 'ok   %s\n' "$1"; }
bad() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures=$((failures + 1))
}

# A PATH with the utilities run.sh uses and no lake, lean or java.
mkdir "$work/bin"
for tool in bash env dirname basename grep cut sha256sum mktemp rm cat cp mkdir sed; do
  found=$(command -v "$tool") || {
    echo "formal/test.sh: $tool not found" >&2
    exit 1
  }
  ln -s "$found" "$work/bin/$tool"
done

# fixture NAME: a fresh copy of formal/ at $work/NAME; prints its path.
fixture() {
  cp -R "$repo_root/formal" "$work/$1"
  rm -rf "$work/$1/lean/.lake"
  printf '%s\n' "$work/$1"
}

# run DIR [VAR=VALUE...]: run.sh on DIR with the bare PATH, no CI variables
# and an empty cache, plus the given variables. Sets `status` and `out`.
run() {
  local dir=$1
  shift
  status=0
  out=$(env -i PATH="$work/bin" HOME="$work/home" XDG_CACHE_HOME="$work/cache" \
    OSKA_FORMAL_DIR="$dir" "$@" bash "$repo_root/scripts/formal/run.sh" 2>&1) || status=$?
}

clean=$(fixture clean)

run "$clean"
if [ "$status" -eq 3 ] && grep -q 'not installed:.*lake.*java.*tla2tools.jar' <<<"$out"; then
  ok "without Lean and TLC outside CI it skips (exit 3) and says what is missing"
else
  bad "without tools outside CI it skips" "exit $status: $out"
fi

for var in CI=true GITHUB_ACTIONS=true; do
  run "$clean" "$var"
  if [ "$status" -eq 1 ] && grep -q 'missing in CI' <<<"$out"; then
    ok "without Lean and TLC with $var it fails"
  else
    bad "without tools with $var it fails" "exit $status: $out"
  fi
done

for word in sorry admit; do
  dir=$(fixture "$word")
  printf 'theorem unfinished : 1 + 1 = 2 := by\n  %s\n' "$word" >"$dir/lean/OskaFormal/Unfinished.lean"
  run "$dir"
  if [ "$status" -eq 1 ] && grep -q 'Unfinished.lean:2:' <<<"$out" && grep -q 'unfinished proof' <<<"$out"; then
    ok "a Lean file with $word fails, naming the line, before any tool is looked for"
  else
    bad "a Lean file with $word fails" "exit $status: $out"
  fi
done

dir=$(fixture comment)
printf -- '-- TODO: replace the sorry below\n' >>"$dir/lean/OskaFormal/Basic.lean"
run "$dir"
if [ "$status" -eq 1 ]; then
  ok "the word sorry in a comment fails too"
else
  bad "the word sorry in a comment fails" "exit $status: $out"
fi

# A word that only contains it is not an unfinished proof.
dir=$(fixture sorrowful)
printf -- '-- sorryless and admittedly fine\n' >>"$dir/lean/OskaFormal/Basic.lean"
run "$dir"
if [ "$status" -eq 3 ]; then
  ok "a word that only contains sorry or admit passes the scan"
else
  bad "a word containing sorry passes the scan" "exit $status: $out"
fi

dir=$(fixture nocfg)
printf -- '---- MODULE Lone ----\n====\n' >"$dir/tla/Lone.tla"
run "$dir"
if [ "$status" -eq 1 ] && grep -q 'Lone.tla has no' <<<"$out"; then
  ok "a .tla with no .cfg fails"
else
  bad "a .tla with no .cfg fails" "exit $status: $out"
fi

dir=$(fixture nomodel)
rm -f "$dir"/tla/*
run "$dir"
if [ "$status" -eq 1 ] && grep -q 'no TLA+ model' <<<"$out"; then
  ok "no TLA+ model at all fails"
else
  bad "no TLA+ model fails" "exit $status: $out"
fi

# With stub tools, a jar that is not the pinned tla2tools.jar fails, in and
# out of CI.
mkdir "$work/stubs"
for tool in lake lean java; do
  printf '#!/usr/bin/env bash\necho "stub %s $*"\n' "$tool" >"$work/stubs/$tool"
  chmod +x "$work/stubs/$tool"
  ln -s "$work/stubs/$tool" "$work/bin/$tool"
done
echo "not tla2tools" >"$work/fake.jar"
run "$clean" OSKA_TLA2TOOLS="$work/fake.jar"
if [ "$status" -eq 1 ] && grep -q 'not the pinned tla2tools.jar' <<<"$out"; then
  ok "a tla2tools.jar that is not the pinned one fails"
else
  bad "a jar that is not the pinned one fails" "exit $status: $out"
fi
rm -f "$work/bin/lake" "$work/bin/lean" "$work/bin/java"

# The toolchain file and the pin name the same Lean.
# shellcheck source=scripts/formal/pins.sh
. "$here/pins.sh"
toolchain=$(tr -d '[:space:]' <"$repo_root/formal/lean/lean-toolchain")
if [ "$toolchain" = "leanprover/lean4:v$lean_version" ]; then
  ok "formal/lean/lean-toolchain matches the pinned Lean $lean_version"
else
  bad "lean-toolchain matches the pin" "$toolchain, pinned v$lean_version"
fi

# The real formal/ has no unfinished proof (checked without tools here; the
# formal step checks it with them).
if grep -rnwE --include='*.lean' --exclude-dir=.lake 'sorry|admit' "$repo_root/formal/lean"; then
  bad "formal/lean has no sorry or admit" "above"
else
  ok "formal/lean has no sorry or admit"
fi

exit $((failures > 0))
