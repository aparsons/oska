#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Pin the Smallstep downloads of the live ACME test: download step-ca and the
# step CLI at the given versions (without the leading v), and write their
# SHA-256 to pin.env. Compare the hashes with the checksums on the release
# pages before committing them.
#
#   scripts/step-ca/pin.sh STEP_CA_VERSION STEP_CLI_VERSION

set -euo pipefail

[ $# -eq 2 ] || { echo "usage: scripts/step-ca/pin.sh STEP_CA_VERSION STEP_CLI_VERSION" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/step-ca/urls.sh
. "$here/urls.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ca_sha=$(curl -fsSL "$(step_ca_url "$1")" | tee "$work/ca.tgz" | sha256sum | cut -d' ' -f1)
cli_sha=$(curl -fsSL "$(step_cli_url "$2")" | tee "$work/cli.tgz" | sha256sum | cut -d' ' -f1)

{
  sed -n '1,/^STEP_CA_VERSION/p' "$here/pin.env" | sed '$d'
  printf 'STEP_CA_VERSION=%s\nSTEP_CA_SHA256=%s\nSTEP_CLI_VERSION=%s\nSTEP_CLI_SHA256=%s\n' "$1" "$ca_sha" "$2" "$cli_sha"
} >"$work/pin.env"
mv "$work/pin.env" "$here/pin.env"
cat "$here/pin.env"
