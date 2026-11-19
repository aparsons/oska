# shellcheck shell=bash disable=SC2034 # read by the scripts that source this file
# The pinned versions and checksums of the formal step's tools (ADRs 0018,
# 0019), and the checks run.sh, install.sh and test.sh share, which source it. To move a pin, change it here and,
# for Lean, in formal/lean/lean-toolchain too (test.sh fails if they differ).
#
# Lean: the GitHub release assets of leanprover/lean4. Each SHA-256 is the
# digest GitHub publishes beside the asset on the release page.
lean_version=4.34.1
lean_sha256_x86_64=47bf4bbd78f70c2e9670598ab7124d92b6efb7330ff33e5fbb4030f6fd72e4e4
lean_sha256_aarch64=fdb974c2cdb4627e090d5d4007b913e09d13c4868720fb5594e22808b3de9e37

# TLC: tla2tools.jar from the tlaplus/tlaplus v1.7.4 release. The release
# notes publish only its SHA-1; the SHA-256 is the one nixpkgs pins for the
# same URL (pkgs/by-name/tl/tlaplus/package.nix). Both are checked.
tla_version=1.7.4
tla_sha1=bee4a54f3ee3d4afc347c3240ec2d9e93b075104
tla_sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88

# Where install.sh puts the tools by default, and where run.sh looks for
# them when they are not on PATH (Lean) or named by OSKA_TLA2TOOLS (TLC).
formal_cache=${XDG_CACHE_HOME:-$HOME/.cache}/oska/formal

# The words that leave a proof unfinished or unchecked, matched as whole
# words anywhere in a .lean file, comments included: `sorry` and `admit`
# (an unfinished proof), `axiom` (a claim assumed, not proved) and
# `native_decide` (a proof that trusts compiled code).
formal_holes_re='sorry|admit|axiom|native_decide'

# formal_scan_holes DIR: prints each line of a .lean file under DIR (Lake's
# .lake excluded) that holds one of those words. Returns 0 when there is
# none, 1 when there is one, and 2 when the scan itself failed (an
# unreadable or missing DIR): a failed scan never passes.
formal_scan_holes() {
  local rc=0
  grep -rnwE --include='*.lean' --exclude-dir=.lake "$formal_holes_re" "$1" || rc=$?
  case $rc in
    0) return 1 ;;
    1) return 0 ;;
    *) return 2 ;;
  esac
}

# formal_toolchain_matches FILE: whether the lean-toolchain FILE names the
# pinned Lean; if not, says so on stderr.
formal_toolchain_matches() {
  local toolchain
  toolchain=$(tr -d '[:space:]' <"$1") || return 1
  [ "$toolchain" = "leanprover/lean4:v$lean_version" ] && return 0
  printf '%s says %s, scripts/formal/pins.sh pins v%s\n' "$1" "${toolchain:-nothing}" "$lean_version" >&2
  return 1
}
