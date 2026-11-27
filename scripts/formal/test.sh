#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for the formal step (scripts/formal/run.sh) that need neither Lean nor
# TLC: without the tools it skips (exit 3) outside CI and fails in CI; an
# unfinished proof or a model with no .cfg fails before any tool is looked
# for; a tla2tools.jar that is not the pinned one fails; a model broken on
# purpose must fail TLC with the invariant it names. Each case runs
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
for tool in bash env dirname basename grep cut sha256sum mktemp rm cat cp mkdir sed tee; do
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

for word in sorry admit axiom native_decide; do
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
printf -- '-- sorryless, admittedly, axioms and native_decided are fine\n' >>"$dir/lean/OskaFormal/Basic.lean"
run "$dir"
if [ "$status" -eq 3 ]; then
  ok "a word that only contains sorry, admit, axiom or native_decide passes the scan"
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

# A scan that fails (grep exits 2) fails the step; it never counts as clean.
dir=$(fixture grepfail)
mv "$work/bin/grep" "$work/grep.real"
printf '#!/usr/bin/env bash\nexit 2\n' >"$work/bin/grep"
chmod +x "$work/bin/grep"
run "$dir"
rm -f "$work/bin/grep"
mv "$work/grep.real" "$work/bin/grep"
if [ "$status" -eq 1 ] && grep -q 'scan failed' <<<"$out"; then
  ok "a failed unfinished-proof scan fails the step"
else
  bad "a failed unfinished-proof scan fails the step" "exit $status: $out"
fi

# shellcheck source=scripts/formal/pins.sh
. "$here/pins.sh"
rc=0
formal_scan_holes "$work/no-such-dir" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then
  ok "formal_scan_holes returns 2 when the scan fails"
else
  bad "formal_scan_holes returns 2 when the scan fails" "returned $rc"
fi

# formal_tlc_verdict judges each TLC run. A model broken on purpose (an
# expect-violation line in its .cfg) passes only when TLC finds that
# invariant violated, so a model that cannot fail is caught. TLC exits 12
# on a violated invariant.
mkdir "$work/verdict"
printf 'SPECIFICATION Spec\nINVARIANT Safe\n' >"$work/verdict/Good.cfg"
printf '\\* expect-violation: Safe\nSPECIFICATION Spec\nINVARIANT Safe\n' >"$work/verdict/Bad.cfg"
printf 'Model checking completed. No error has been found.\n' >"$work/verdict/pass.log"
printf 'Error: Invariant Safe is violated.\n' >"$work/verdict/safe.log"
printf 'Error: Invariant TypeOK is violated.\n' >"$work/verdict/other.log"
printf 'Error: Parsing or semantic analysis failed.\n' >"$work/verdict/parse.log"
# verdict CFG RC LOG WANT_STATUS PATTERN NAME
verdict() {
  status=0
  out=$(formal_tlc_verdict M "$work/verdict/$1" "$2" "$work/verdict/$3" 2>&1) || status=$?
  if [ "$status" -eq "$4" ] && grep -q "$5" <<<"$out"; then
    ok "$6"
  else
    bad "$6" "status $status: $out"
  fi
}
verdict Good.cfg 0 pass.log 0 '' "a model that must pass passes when TLC passes it"
verdict Good.cfg 12 safe.log 1 'TLC failed on M (exit 12)' "a model that must pass fails when TLC fails it"
verdict Bad.cfg 0 pass.log 1 'cannot fail' "a model broken on purpose that TLC passes fails the step"
verdict Bad.cfg 12 safe.log 0 'fails as it must' "a model broken on purpose passes when TLC finds its invariant violated"
verdict Bad.cfg 12 other.log 1 'not by finding Safe violated' "a model broken on purpose fails when TLC finds another invariant violated"
verdict Bad.cfg 255 parse.log 1 'not by finding Safe violated' "a model broken on purpose fails when TLC cannot parse it"

# An expectation that is not exactly one invariant, on the .cfg's first
# line, is refused: two lines would let a model that violates only one of
# them pass.
printf '\\* expect-violation: Safe\n\\* expect-violation: Other\nINVARIANT Safe\n' >"$work/verdict/Two.cfg"
printf '\\* expect-violation: Safe Other\nINVARIANT Safe\n' >"$work/verdict/Words.cfg"
printf '\\* expect-violation: Safe|Other\nINVARIANT Safe\n' >"$work/verdict/Alt.cfg"
printf 'SPECIFICATION Spec\n\\* expect-violation: Safe\nINVARIANT Safe\n' >"$work/verdict/Late.cfg"
for cfg in Two Words Alt Late; do
  status=0
  out=$(formal_expected_violation "$work/verdict/$cfg.cfg" 2>&1) || status=$?
  if [ "$status" -eq 1 ] && grep -q "$cfg.cfg" <<<"$out"; then
    ok "formal_expected_violation refuses $cfg.cfg"
  else
    bad "formal_expected_violation refuses $cfg.cfg" "status $status: $out"
  fi
done
verdict Two.cfg 12 safe.log 1 'expect-violation' "a .cfg with two expect-violation lines fails even when TLC finds one of them violated"
verdict Words.cfg 12 safe.log 1 'one identifier' "a .cfg whose expect-violation names two words fails"

# The real models broken on purpose. Each is a model that must pass with
# one flag flipped, at the same bound, and expects the invariant named here
# violated: so the racy counts model fails because its counts are not
# atomic, not because of some other change (#432). A new model broken on
# purpose joins this list.
#   BROKEN GOOD FLAG INVARIANT
broken_models='SharedCountsRacy SharedCounts Atomic NoUseAfterFree
ScopedTasksLeakyScopes ScopedTasks ConfineScopes SpawnOnlyIntoOpenScopes
ChannelsRacySelect Channels LockArms SelectChoosesReady
ChannelsLossyClose Channels CloseKeepsBuffer NoLostMessage
StoreLogAckEarly StoreLog AckAfterSync AckedSurvive
StoreLogRenameFirst StoreLog SyncSnapshotFirst TruncateOnlyAfterDurableSnapshot
StoreLogNoChecksum StoreLog VerifyChecksum ChainValid'
tla=$repo_root/formal/tla
# constants CFG [FLAG]: the CONSTANTS assignments of CFG, trimmed and sorted,
# without FLAG's.
constants() {
  sed -n '/^CONSTANTS/,/^[A-Z]/{/^[[:space:]]/p;}' "$1" |
    sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | grep -v "^${2:-}[[:space:]]*=" | sort
}
while read -r broken good flag invariant; do
  want=$(formal_expected_violation "$tla/$broken.cfg") || want=
  if [ "$want" = "$invariant" ]; then
    ok "$broken must fail TLC with $invariant violated"
  else
    bad "$broken must fail TLC with $invariant violated" "its .cfg expects '$want'"
  fi
  if grep -qE "^[[:space:]]*(INVARIANTS?[[:space:]]+)?${invariant}[[:space:]]*\$" "$tla/$broken.cfg"; then
    ok "$broken.cfg checks $invariant"
  else
    bad "$broken.cfg checks $invariant" "$invariant is not listed as an invariant"
  fi
  if grep -qx "EXTENDS $good" "$tla/$broken.tla" &&
    grep -qx "  $flag = TRUE" "$tla/$good.cfg" && grep -qx "  $flag = FALSE" "$tla/$broken.cfg" &&
    [ -n "$(constants "$tla/$good.cfg" "$flag")" ] &&
    [ "$(constants "$tla/$good.cfg" "$flag")" = "$(constants "$tla/$broken.cfg" "$flag")" ]; then
    ok "$broken is $good with $flag = FALSE, at the same bound"
  else
    bad "$broken is $good with $flag = FALSE, at the same bound" \
      "$(constants "$tla/$good.cfg") / $(constants "$tla/$broken.cfg")"
  fi
  want=$(formal_expected_violation "$tla/$good.cfg") || want=error
  if [ -z "$want" ]; then
    ok "$good must pass TLC"
  else
    bad "$good must pass TLC" "its .cfg expects '$want' violated"
  fi
done <<<"$broken_models"
for cfg in "$tla"/*.cfg; do
  want=$(formal_expected_violation "$cfg") || want=error
  [ -n "$want" ] || continue
  name=$(basename "$cfg" .cfg)
  if grep -q "^$name " <<<"$broken_models"; then
    ok "$name is a known model broken on purpose"
  else
    bad "$name is a known model broken on purpose" "add it to broken_models in scripts/formal/test.sh"
  fi
done

# The single-line form of the invariant list counts too.
printf '\\* expect-violation: Safe\nSPECIFICATION Spec\nINVARIANT Safe\n' >"$work/verdict/One.cfg"
if grep -qE "^[[:space:]]*(INVARIANTS?[[:space:]]+)?Safe[[:space:]]*\$" "$work/verdict/One.cfg"; then
  ok "a single-line INVARIANT NAME lists the invariant"
else
  bad "a single-line INVARIANT NAME lists the invariant" "not matched"
fi

# check.sh's formal block refuses a skip in CI itself. It runs here with
# step and skip_step stubbed and a run.sh that exits 3 (tools missing).
block=$(sed -n '/^if step "formal" /,/^fi$/p' "$repo_root/scripts/check.sh")
mkdir -p "$work/checkblock/scripts/formal"
printf '#!/usr/bin/env bash\nexit 3\n' >"$work/checkblock/scripts/formal/run.sh"
chmod +x "$work/checkblock/scripts/formal/run.sh"
# check_block [VAR=VALUE...]: sets `status` and `out`.
check_block() {
  status=0
  out=$(cd "$work/checkblock" && env -u CI -u GITHUB_ACTIONS "$@" bash -c '
    set -euo pipefail
    step() { return 0; }
    skip_step() { echo "skip_step: $*"; }
    '"$block" 2>&1) || status=$?
}
check_block
if [ -n "$block" ] && [ "$status" -eq 0 ] && grep -q '^skip_step: ' <<<"$out"; then
  ok "check.sh's formal block skips a missing tool outside CI"
else
  bad "check.sh's formal block skips outside CI" "exit $status: $out"
fi
for var in CI=true GITHUB_ACTIONS=true; do
  check_block "$var"
  if [ "$status" -ne 0 ] && ! grep -q '^skip_step: ' <<<"$out"; then
    ok "check.sh's formal block refuses to skip with $var"
  else
    bad "check.sh's formal block refuses to skip with $var" "exit $status: $out"
  fi
done

# install.sh refuses a download whose SHA-256 differs and installs nothing,
# and refuses a lean-toolchain that differs from the pin before it
# downloads. A stub curl serves bogus bytes, so this runs offline.
mkdir "$work/ibin"
# shellcheck disable=SC2016 # written out, expanded when the stub runs
printf '%s\n' '#!/usr/bin/env bash' 'echo "curl $*" >>"$CURL_LOG"' \
  'while [ $# -gt 0 ]; do if [ "$1" = -o ]; then echo bogus >"$2"; shift; fi; shift; done' >"$work/ibin/curl"
printf '#!/usr/bin/env bash\nexit 0\n' >"$work/ibin/zstd"
chmod +x "$work/ibin/curl" "$work/ibin/zstd"
# install DIR FORMAL_DIR: sets `status` and `out`.
install_run() {
  status=0
  out=$(PATH="$work/ibin:$PATH" CURL_LOG="$work/curl.log" OSKA_FORMAL_DIR="$2" \
    bash "$repo_root/scripts/formal/install.sh" "$1" 2>&1) || status=$?
}
: >"$work/curl.log"
install_run "$work/inst1" "$clean"
if [ "$status" -ne 0 ] && grep -q 'does not match its pinned SHA-256' <<<"$out" &&
  [ ! -e "$work/inst1" ] && [ -s "$work/curl.log" ]; then
  ok "install.sh refuses a download whose SHA-256 differs and installs nothing"
else
  bad "install.sh refuses a bad download" "exit $status: $out"
fi
dir=$(fixture badpin)
echo 'leanprover/lean4:v4.0.0' >"$dir/lean/lean-toolchain"
: >"$work/curl.log"
install_run "$work/inst2" "$dir"
if [ "$status" -ne 0 ] && grep -q 'v4.0.0' <<<"$out" && [ ! -s "$work/curl.log" ] && [ ! -e "$work/inst2" ]; then
  ok "install.sh refuses a lean-toolchain that differs from the pin, before downloading"
else
  bad "install.sh refuses a lean-toolchain that differs from the pin" "exit $status: $out"
fi

# The step also refuses such a toolchain (formal_toolchain_matches).
if ! formal_toolchain_matches "$dir/lean/lean-toolchain" 2>/dev/null; then
  ok "formal_toolchain_matches refuses a toolchain that differs from the pin"
else
  bad "formal_toolchain_matches refuses a toolchain that differs from the pin" "it matched"
fi

# The real toolchain file and the pin name the same Lean.
if formal_toolchain_matches "$repo_root/formal/lean/lean-toolchain"; then
  ok "formal/lean/lean-toolchain matches the pinned Lean $lean_version"
else
  bad "lean-toolchain matches the pin" "above"
fi

# The real formal/ has no proof hole (checked without tools here; the formal
# step checks it with them). A failed scan is a failure.
rc=0
formal_scan_holes "$repo_root/formal/lean" || rc=$?
case $rc in
  0) ok "formal/lean has no sorry, admit, axiom or native_decide" ;;
  1) bad "formal/lean has no proof hole" "above" ;;
  *) bad "formal/lean has no proof hole" "the scan failed" ;;
esac

exit $((failures > 0))
