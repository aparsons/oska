#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# The live ACME test (spec #824, #839): start a Smallstep CA with an ACME
# provisioner on loopback, then run live.osk, which issues a certificate by
# HTTP-01 and renews it with std.acme. No public DNS and no rate limits. Let's
# Encrypt staging stays a manual check and never runs in CI.
#
#   scripts/step-ca/run.sh OSKAC
#
# Linux only. It needs sudo for port 80 (the CA checks HTTP-01 there) and for
# one /etc/hosts line, python3 and openssl. The downloads are pinned in
# pin.env and checked against their SHA-256 before anything is unpacked.

set -euo pipefail

[ $# -eq 1 ] || { echo "usage: scripts/step-ca/run.sh OSKAC" >&2; exit 2; }
oskac=$(realpath "$1")
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/step-ca/pin.env
. "$here/pin.env"
# shellcheck source=scripts/step-ca/urls.sh
. "$here/urls.sh"

for v in STEP_CA_SHA256 STEP_CLI_SHA256 STEP_CA_VERSION STEP_CLI_VERSION; do
  if [ "${!v}" = UNSET ]; then
    echo "step-ca: $v is UNSET in scripts/step-ca/pin.env; run scripts/step-ca/pin.sh" >&2
    exit 1
  fi
done

work=$(mktemp -d)
pids=()
cleanup() {
  for p in "${pids[@]}"; do
    kill "$p" 2>/dev/null || true
  done
  # The web server runs as root (port 80), so it is stopped through sudo.
  [ -z "${web_pid:-}" ] || sudo kill "$web_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

fetch() { # URL SHA256 DEST
  curl -fsSL --retry 3 -o "$3" "$1"
  echo "$2  $3" | sha256sum -c - >/dev/null || { echo "step-ca: the download of $1 does not match its pinned SHA-256" >&2; exit 1; }
}
fetch "$(step_ca_url "$STEP_CA_VERSION")" "$STEP_CA_SHA256" "$work/ca.tgz"
fetch "$(step_cli_url "$STEP_CLI_VERSION")" "$STEP_CLI_SHA256" "$work/cli.tgz"
mkdir "$work/ca" "$work/cli"
tar -xzf "$work/ca.tgz" -C "$work/ca"
tar -xzf "$work/cli.tgz" -C "$work/cli"
step_ca=$(find "$work/ca" -type f -name step-ca | head -n 1)
step=$(find "$work/cli" -type f -name step | head -n 1)
[ -n "$step_ca" ] && [ -n "$step" ] || { echo "step-ca: the archives hold no step-ca or step" >&2; exit 1; }
chmod +x "$step_ca" "$step"

# The CA: its files stay in $work, never in ~/.step.
export STEPPATH="$work/step"
printf 'ci-only-password' >"$work/password"
"$step" ca init --deployment-type=standalone --name=oska-ci --dns=localhost \
  --address=127.0.0.1:9000 --provisioner=admin --password-file="$work/password" \
  --provisioner-password-file="$work/password" >/dev/null
"$step" ca provisioner add acme-ci --type=ACME --ca-config="$STEPPATH/config/ca.json"
"$step_ca" "$STEPPATH/config/ca.json" --password-file="$work/password" >"$work/step-ca.log" 2>&1 &
pids+=("$!")

ready=0
for _ in $(seq 1 60); do
  if curl -fs --cacert "$STEPPATH/certs/root_ca.crt" https://localhost:9000/health >/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != 1 ]; then
  echo "step-ca: the CA did not come up" >&2
  cat "$work/step-ca.log" >&2
  exit 1
fi

# The name the certificate is for, and the HTTP-01 server the CA calls.
mkdir -p "$work/run/www/.well-known/acme-challenge" "$work/run/out"
cp "$STEPPATH/certs/root_ca.crt" "$work/run/root.crt"
echo '127.0.0.1 acme-ci.test' | sudo tee -a /etc/hosts >/dev/null
sudo python3 -m http.server 80 --bind 127.0.0.1 --directory "$work/run/www" >"$work/web.log" 2>&1 &
web_pid=$!
sleep 1

status=0
(cd "$work/run" && "$oskac" run "$here/live.osk") >"$work/live.out" 2>&1 || status=$?
cat "$work/live.out"
if [ "$status" != 0 ] || ! grep -qx 'issued 1' "$work/live.out" || ! grep -qx 'renewed 2' "$work/live.out"; then
  echo "step-ca: the live test failed; the CA's log:" >&2
  cat "$work/step-ca.log" >&2
  exit 1
fi

# The chains are what the CA's root vouches for, and name the host.
for n in 1 2; do
  openssl verify -CAfile "$work/run/root.crt" "$work/run/out/cert$n.pem" >/dev/null
  openssl x509 -in "$work/run/out/cert$n.pem" -noout -ext subjectAltName | grep -q 'acme-ci.test'
done
echo "step-ca: issued and renewed a certificate for acme-ci.test; both verify against the CA root"
