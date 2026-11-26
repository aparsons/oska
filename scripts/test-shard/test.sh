#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for scripts/test-shard.sh: in every set, each test target of the set
# runs in exactly one shard (the position-split site.rs counts once, with all
# its slices), so splitting a CI job drops no test and runs none twice; and
# the shards that ci.yml names are the ones the script has.
#
#   scripts/test-shard/test.sh

set -euo pipefail

cd "$(dirname "$0")/../.."

passed=0
failures=0
ok() { printf 'ok   %s\n' "$1"; passed=$((passed + 1)); }
bad() { printf 'FAIL %s: %s\n' "$1" "$2"; failures=$((failures + 1)); }

split=$(grep '^SITE_SPLIT=' scripts/test-shard.sh | cut -d= -f2)

for set in musl linux macos windows; do
  # Each shard but `all`, one "PACKAGE KIND NAME" line a target; a site slice
  # is "oska-run test site/K".
  got=$(for shard in $(scripts/test-shard.sh --sets | awk -v s="$set" '$1 == s && $2 != "all" {print $2}'); do
    scripts/test-shard.sh "$set" "$shard" --plan
  done | sort)
  # `all` is the whole set: what every shard must add up to. Its site.rs is
  # one target, so the slices are folded back into it.
  want=$(scripts/test-shard.sh "$set" all --plan | sort)
  folded=$(printf '%s\n' "$got" | sed 's|^\(oska-run test site\)/[0-9]*$|\1|' | sort | uniq -c | awk '{c=$1; $1=""; sub(/^ /,""); print c "\t" $0}')
  slices=$(printf '%s\n' "$got" | grep -c '^oska-run test site/' || true)
  extra=$(printf '%s\n' "$folded" | awk -F'\t' -v n="$split" -v set="$set" '
    { want = ($2 == "oska-run test site" && set == "musl") ? n : 1; if ($1 != want) print $0 }')
  if [ -n "$extra" ]; then
    bad "$set: each target runs once" "run more or fewer times than once: $extra"
  else
    ok "$set: each target runs exactly once"
  fi
  seen=$(printf '%s\n' "$folded" | awk -F'\t' '{print $2}' | sort)
  if [ "$seen" = "$want" ]; then
    ok "$set: the shards run every target of the set"
  else
    bad "$set: shards cover the set" "differs: $(diff <(printf '%s\n' "$want") <(printf '%s\n' "$seen") | tr '\n' ' ')"
  fi
  if [ "$set" = musl ] && [ "$slices" -ne "$split" ]; then
    bad "musl: the site.rs slices" "$slices slices, want $split"
  fi
done

# Every test file of oska-sys, oska-run and oskac (macos, linux) is in a plan.
# The plan is read once: `grep -q` closing a pipe early made the writer die of
# SIGPIPE, which `pipefail` reports as a missing target (about 1 run in 10).
linux_plan=$(scripts/test-shard.sh linux all --plan)
for f in crates/oska-run/tests/*.rs crates/oska-sys/tests/*.rs crates/oskac/tests/*.rs; do
  n=$(basename "$f" .rs)
  if ! grep -q " test $n\$" <<<"$linux_plan"; then
    bad "linux: every test file is a target" "$n is in no plan"
  fi
done
ok "linux: every test file of the crates is a target"

# ci.yml (windows.yml for windows) names each shard of every set, and none the script lacks: a line
# `# shards SET: a b c` above each matrix lists them.
for set in musl linux macos windows; do
  want=$(scripts/test-shard.sh --sets | awk -v s="$set" '$1 == s && $2 != "all" {print $2}' | sort | tr '\n' ' ')
  wf=.github/workflows/ci.yml
  [ "$set" = windows ] && wf=.github/workflows/windows.yml
  have=$(sed -n "s/^ *# shards $set: //p" "$wf" | tr ' ' '\n' | sort | tr '\n' ' ')
  if [ "$want" = "$have" ]; then
    ok "$set: its workflow names every shard"
  else
    bad "$set: its workflow names every shard" "script: $want; workflow: $have"
  fi
done
if scripts/test-shard.sh bogus rest --plan 2>/dev/null; then
  bad "a bad set fails" "it ran"
else
  ok "a bad set fails"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
