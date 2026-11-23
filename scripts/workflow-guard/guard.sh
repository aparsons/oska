#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Keeps every CI job bounded and every action pinned. GitHub gives a job 360
# minutes unless the workflow says otherwise, and a hung test once held a
# runner for six hours. A tag or branch can move after review, so a workflow
# could run an action nobody reviewed (#290).
#
#   scripts/workflow-guard/guard.sh [repo-root]
#
# Reads every workflow file (.yml or .yaml) in .github/workflows under the
# root, finds each job under `jobs:`, and refuses, printing `file: job:
# reason` and exiting 1, any job that has no `timeout-minutes` at job level,
# whose value is not a plain whole number from 1 to 60, or that calls a
# reusable workflow (`uses:` at job level), a form that cannot carry the key.
# It reads the files as text and looks only at indentation, so it needs no
# YAML parser, no network and no installed actions tooling. A job's own keys
# are the lines at the indentation of its first key; a `timeout-minutes` in a
# step is deeper and does not count. The 60-minute ceiling is the
# maintainer's decision (spec #208).
#
# It also refuses, printing `file:line: reason`, any `uses:` that does not pin
# a full 40-character commit SHA with the version in a trailing comment, as
# `uses: actions/checkout@<sha> # v4.4.0`. A tag (`@v4`), a branch (`@main`),
# a short SHA or no ref at all fails. A local action (`./path`) needs no pin.

set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root"

max=60

files=()
for file in .github/workflows/*.yml .github/workflows/*.yaml; do
  [ -f "$file" ] && files+=("$file")
done

if [ "${#files[@]}" -eq 0 ]; then
  printf 'workflow-guard: no workflow files in .github/workflows under %s\n' "$root" >&2
  exit 1
fi

# For one file, prints a line per job: "job<TAB>problem", with an empty
# problem when the job passes, or "-<TAB>problem" for a file with no jobs.
scan() {
  awk -v max="$max" '
    function finish_job() {
      if (job == "") return
      job_count++
      if (uses) {
        print job "\tcalls a reusable workflow (uses: at job level), which cannot carry timeout-minutes; run the work as steps in a job"
      } else if (!found) {
        print job "\thas no timeout-minutes at job level (add timeout-minutes: N, a whole number from 1 to " max ")"
      } else if (value !~ /^[1-9][0-9]{0,3}$/ || value + 0 > max) {
        print job "\ttimeout-minutes is \"" value "\"; it must be a plain whole number from 1 to " max ", not an expression, zero, negative or text"
      } else {
        print job "\t"
      }
      job = ""
    }
    /^[[:space:]]*(#.*)?$/ { next }
    {
      line = $0
      sub(/\r$/, "", line)
      ind = line
      sub(/[^ ].*$/, "", ind)
      ind = length(ind)
    }
    ind == 0 {
      finish_job()
      in_jobs = (line ~ /^jobs:[[:space:]]*(#.*)?$/)
      next
    }
    !in_jobs { next }
    job_indent == "" { job_indent = ind }
    ind == job_indent {
      finish_job()
      name = line
      sub(/^[[:space:]]*/, "", name)
      sub(/:.*$/, "", name)
      gsub(/^["\047]|["\047]$/, "", name)
      job = name
      found = 0; uses = 0; value = ""; key_indent = ""
      next
    }
    job != "" && ind > job_indent {
      if (key_indent == "") key_indent = ind
      if (ind == key_indent) {
        if (line ~ /^[[:space:]]*timeout-minutes:/) {
          found = 1
          value = line
          sub(/^[[:space:]]*timeout-minutes:[[:space:]]*/, "", value)
          sub(/[[:space:]]+#.*$/, "", value)
          sub(/[[:space:]]+$/, "", value)
        } else if (line ~ /^[[:space:]]*uses:/) {
          uses = 1
        }
      }
    }
    END {
      finish_job()
      if (job_count == 0) print "-\thas no jobs under jobs:"
    }
  ' "$1"
}

# For one file, prints "line<TAB>problem" for each `uses:` that is not pinned
# to a full commit SHA with a version comment.
pins() {
  awk '
    {
      line = $0
      sub(/\r$/, "", line)
    }
    line !~ /^[[:space:]]*(-[[:space:]]+)?uses:/ { next }
    {
      ref = line
      sub(/^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]*/, "", ref)
      comment = ""
      if (match(ref, /[[:space:]]+#/)) {
        comment = substr(ref, RSTART + RLENGTH)
        ref = substr(ref, 1, RSTART - 1)
      }
      sub(/[[:space:]]+$/, "", ref)
      gsub(/^["\047]|["\047]$/, "", ref)
      sub(/^[[:space:]]+/, "", comment)
      if (ref ~ /^\.\//) next
      if (ref !~ /@[0-9a-f]{40}$/) {
        print NR "\tuses: " ref " is not pinned to a full commit SHA; write owner/repo@<40-character SHA> # vX.Y.Z"
      } else if (comment == "") {
        print NR "\tuses: " ref " has no version comment; add # vX.Y.Z after the SHA"
      }
    }
  ' "$1"
}

checked=0
bad=0
pinned=0
unpinned=0
for file in "${files[@]}"; do
  while IFS=$'\t' read -r job problem; do
    if [ -n "$problem" ]; then
      printf '%s: %s: %s\n' "$file" "$job" "$problem" >&2
      bad=$((bad + 1))
    else
      checked=$((checked + 1))
    fi
  done < <(scan "$file")
done

for file in "${files[@]}"; do
  while IFS=$'\t' read -r line problem; do
    printf '%s:%s: %s\n' "$file" "$line" "$problem" >&2
    unpinned=$((unpinned + 1))
  done < <(pins "$file")
  pinned=$((pinned + $(grep -cE '^[[:space:]]*(-[[:space:]]+)?uses:' "$file" || true)))
done
pinned=$((pinned - unpinned))

if [ "$unpinned" -ne 0 ]; then
  printf 'workflow-guard: %s uses: line(s) above are not pinned; pin each action to a full commit SHA with # vX.Y.Z after it\n' "$unpinned" >&2
fi

if [ "$bad" -ne 0 ]; then
  printf 'workflow-guard: %s job(s) above are not bounded; every job needs timeout-minutes, a whole number from 1 to %s\n' "$bad" "$max" >&2
fi

if [ "$bad" -ne 0 ] || [ "$unpinned" -ne 0 ]; then
  exit 1
fi

printf 'workflow-guard: %s jobs in %s workflows all have timeout-minutes of 1 to %s; %s uses: lines all pinned to a commit SHA\n' "$checked" "${#files[@]}" "$max" "$pinned"
