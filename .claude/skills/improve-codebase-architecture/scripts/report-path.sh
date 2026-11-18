#!/usr/bin/env bash
# Make a new, private, empty file for the architecture report and print its
# absolute path.
#
#   scripts/report-path.sh
#
# The file is architecture-review-<UTC timestamp>-<random>.html in $TMPDIR
# (else /tmp), with mode 0600. The script refuses if it cannot make the file.
set -euo pipefail

if [[ $# -ne 0 ]]; then
  echo "report-path: takes no arguments" >&2
  exit 2
fi

for tool in mktemp date chmod; do
  if ! command -v "$tool" > /dev/null 2>&1; then
    echo "report-path: missing tool: $tool" >&2
    exit 1
  fi
done

dir=${TMPDIR:-/tmp}
dir=${dir%/}
if [[ ! -d $dir || ! -w $dir ]]; then
  echo "report-path: temporary directory is not writable: $dir" >&2
  exit 1
fi

umask 077
stamp=$(date -u +%Y%m%dT%H%M%SZ)
path=$(mktemp "$dir/architecture-review-$stamp-XXXXXX")
chmod 600 "$path"
final="$path.html"
mv -n "$path" "$final"
if [[ ! -f $final || -e $path ]]; then
  rm -f "$path"
  echo "report-path: could not create $final" >&2
  exit 1
fi

case $final in
  /*) printf '%s\n' "$final" ;;
  *) printf '%s/%s\n' "$(pwd -P)" "$final" ;;
esac
