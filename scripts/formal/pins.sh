# shellcheck shell=bash disable=SC2034 # read by the scripts that source this file
# The pinned versions and checksums of the formal step's tools (ADRs 0018,
# 0019), sourced by install.sh and run.sh. To move a pin, change it here and,
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
