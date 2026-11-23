#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Fails when a tracked .rs, .osk or .sh file lacks its SPDX header, or when the
# tree has no NOTICE file (Apache 2.0 section 4).
#
#   scripts/spdx-guard/guard.sh [ROOT]
#
# ROOT is a git work tree (default: the repository this script is in). The
# header is `// SPDX-License-Identifier: Apache-2.0` in .rs and .osk files and
# `# SPDX-License-Identifier: Apache-2.0` in .sh files. It is the first line, or
# the second when the first is a shebang. Symlinks are skipped: the file they
# point to is checked where it lives.

set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
id='SPDX-License-Identifier: Apache-2.0'
bad=0

if [ ! -f "$root/NOTICE" ]; then
  printf 'spdx-guard: %s has no NOTICE file\n' "$root" >&2
  bad=1
fi

while IFS= read -r -d '' file; do
  [ -L "$root/$file" ] && continue
  case "$file" in
    *.sh) want="# $id" ;;
    *) want="// $id" ;;
  esac
  first=$(head -n 1 "$root/$file" || true)
  if [[ $first == '#!'* ]]; then
    line=$(sed -n 2p "$root/$file")
  else
    line=$first
  fi
  if [[ $first != '#!'* ]] && [[ $(sed -n 2p "$root/$file") == '#!'* ]]; then
    line='(a shebang on line 2)'
  fi
  # Tolerate a CR from a CRLF checkout.
  line=${line%$'\r'}
  if [ "$line" != "$want" ]; then
    printf 'spdx-guard: %s does not start with "%s"\n' "$file" "$want" >&2
    bad=1
  fi
done < <(git -C "$root" ls-files -z -- '*.rs' '*.osk' '*.sh')

if [ "$bad" -ne 0 ]; then
  exit 1
fi
