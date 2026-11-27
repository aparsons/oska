#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Fetch the coverage report of the latest successful Coverage run on main
# (.github/workflows/coverage.yml), for https://oskalang.dev/coverage.html.
#
#   scripts/site/stage-coverage.sh fetch DIR   download it to DIR and copy
#                                              its table to site/coverage.md
#   scripts/site/stage-coverage.sh stage DIR SITE   copy lcov.info to
#                                              SITE/coverage/
#
# `fetch` runs before the generator, which reads site/coverage.md; `stage`
# runs after it, before the link check. Before the first Coverage run
# succeeds, or when its artifact has expired, `fetch` publishes no coverage
# page and says so: the site still deploys. Needs GH_TOKEN and GH_REPO.

set -euo pipefail

mode=${1:-}
dir=${2:-}
[ -n "$dir" ] || { echo "usage: stage-coverage.sh fetch DIR | stage DIR SITE" >&2; exit 2; }
cd "$(dirname "$0")/../.."

case $mode in
  fetch)
    run=$(gh run list --workflow coverage.yml --branch main --status success \
      --limit 1 --json databaseId --jq '.[0].databaseId // empty')
    if [ -z "$run" ]; then
      echo "stage-coverage.sh: no successful Coverage run on main yet; no coverage page"
      exit 0
    fi
    mkdir -p "$dir"
    if ! gh run download "$run" --name coverage --dir "$dir"; then
      echo "stage-coverage.sh: the artifact of run $run is gone; no coverage page"
      exit 0
    fi
    cp "$dir/summary.md" site/coverage.md
    echo "stage-coverage.sh: coverage of run $run"
    ;;
  stage)
    site=${3:?usage: stage-coverage.sh stage DIR SITE}
    [ -f "$dir/summary.md" ] || { echo "stage-coverage.sh: nothing fetched; nothing to stage"; exit 0; }
    mkdir -p "$site/coverage"
    cp "$dir/lcov.info" "$site/coverage/lcov.info"
    ;;
  *)
    echo "usage: stage-coverage.sh fetch DIR | stage DIR SITE" >&2
    exit 2
    ;;
esac
