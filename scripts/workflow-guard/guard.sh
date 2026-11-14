#!/usr/bin/env bash
# Keeps every CI job bounded. GitHub gives a job 360 minutes unless the
# workflow says otherwise, and a hung test once held a runner for six hours.
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
      jobs++
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
      if (jobs == 0) print "-\thas no jobs under jobs:"
    }
  ' "$1"
}

jobs=0
bad=0
for file in "${files[@]}"; do
  while IFS=$'\t' read -r job problem; do
    if [ -n "$problem" ]; then
      printf '%s: %s: %s\n' "$file" "$job" "$problem" >&2
      bad=$((bad + 1))
    else
      jobs=$((jobs + 1))
    fi
  done < <(scan "$file")
done

if [ "$bad" -ne 0 ]; then
  printf 'workflow-guard: %s job(s) above are not bounded; every job needs timeout-minutes, a whole number from 1 to %s\n' "$bad" "$max" >&2
  exit 1
fi

printf 'workflow-guard: %s jobs in %s workflows all have timeout-minutes of 1 to %s\n' "$jobs" "${#files[@]}" "$max"
