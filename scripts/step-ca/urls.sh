# SPDX-License-Identifier: Apache-2.0
# shellcheck shell=bash
# Where the pinned Smallstep archives are downloaded from (Linux, x86-64).
# Sourced by run.sh and pin.sh.

step_ca_url() {
  printf 'https://github.com/smallstep/certificates/releases/download/v%s/step-ca_linux_%s_amd64.tar.gz\n' "$1" "$1"
}

step_cli_url() {
  printf 'https://github.com/smallstep/cli/releases/download/v%s/step_linux_%s_amd64.tar.gz\n' "$1" "$1"
}
