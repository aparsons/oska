#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Print every built compliance page to a PDF beside it.
#
#   scripts/site/make-pdfs.sh [SITE_DIR]      SITE_DIR defaults to _site
#
# Run it on a built site (`oskac run site/build.osk`), before
# `site/check-links.osk`, which fails on a compliance page that links a PDF
# this did not make. The Pages workflow runs it; it runs locally too, when
# Chrome or Chromium is installed (set CHROME to name the program).
#
# Pages link their stylesheet by an absolute path, so the site is served from
# 127.0.0.1 for the print. Chrome resolves no other host: a request to any
# other origin fails, so the print fetches nothing from the network.
set -euo pipefail

site=${1:-_site}
dir="$site/compliance"

if [ ! -d "$dir" ]; then
  echo "make-pdfs.sh: $dir does not exist; the site has no compliance section, so there is nothing to print"
  exit 0
fi

chrome=${CHROME:-}
if [ -z "$chrome" ]; then
  for candidate in google-chrome google-chrome-stable chromium chromium-browser; do
    if command -v "$candidate" >/dev/null 2>&1; then
      chrome=$candidate
      break
    fi
  done
fi
if [ -z "$chrome" ]; then
  echo "make-pdfs.sh: no Chrome or Chromium found (set CHROME)" >&2
  exit 1
fi

work=$(mktemp -d)
server=
cleanup() {
  if [ -n "$server" ]; then
    kill "$server" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

# A static server on a free port of the loopback address only. It writes the
# port to "$work/port" once it listens.
cat >"$work/serve.py" <<'PY'
import functools
import http.server
import sys


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


handler = functools.partial(Quiet, directory=sys.argv[1])
httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
with open(sys.argv[2], "w") as out:
    out.write(str(httpd.server_address[1]))
httpd.serve_forever()
PY
python3 "$work/serve.py" "$site" "$work/port" &
server=$!

for _ in $(seq 1 50); do
  [ -s "$work/port" ] && break
  sleep 0.1
done
[ -s "$work/port" ] || { echo "make-pdfs.sh: the local server did not start" >&2; exit 1; }
port=$(cat "$work/port")

count=0
for html in "$dir"/*.html; do
  [ -e "$html" ] || continue
  name=$(basename "$html" .html)
  pdf="$dir/$name.pdf"
  rm -f "$pdf"
  # --no-sandbox: the runner's user namespaces may be restricted, and the
  # only page loaded is one this build just wrote.
  "$chrome" --headless --disable-gpu --no-sandbox \
    --user-data-dir="$work/profile" \
    --host-resolver-rules='MAP * ~NOTFOUND, EXCLUDE 127.0.0.1' \
    --no-pdf-header-footer \
    --print-to-pdf="$pdf" \
    "http://127.0.0.1:$port/compliance/$name.html" >"$work/chrome.log" 2>&1 || {
    cat "$work/chrome.log" >&2
    echo "make-pdfs.sh: Chrome failed on $html" >&2
    exit 1
  }
  if [ ! -s "$pdf" ]; then
    cat "$work/chrome.log" >&2
    echo "make-pdfs.sh: Chrome made no PDF for $html" >&2
    exit 1
  fi
  count=$((count + 1))
done

if [ "$count" -eq 0 ]; then
  echo "make-pdfs.sh: $dir holds no page to print" >&2
  exit 1
fi
echo "make-pdfs.sh: printed $count PDFs under $dir"
