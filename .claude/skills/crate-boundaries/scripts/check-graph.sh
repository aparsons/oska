#!/usr/bin/env bash
# Checks the dependency graph of a Cargo workspace's own crates against a
# list of allowed edges. Install it in Oska as scripts/crate-graph/check.sh.
#
#   check-graph.sh [--repo DIR] [--allowed FILE]   check the graph
#   check-graph.sh [--repo DIR] --show             print the current graph
#
# DIR is the repo root: the default is the git top level of the current
# directory. FILE defaults to DIR/scripts/crate-graph/allowed.txt.
#
# It reads every crates/*/Cargo.toml: the package name, and each path
# dependency in [dependencies], [dev-dependencies] and [build-dependencies],
# target-specific tables too. A dev-dependency is an edge like any other.
#
# The allowed file has one line for each crate, `crate: dep dep ...`, with
# nothing after the colon for a crate that may depend on no other crate.
# `#` starts a comment. --show prints the graph in the same form.
#
# Exit 0: every crate is listed, every edge is allowed, and there is no cycle.
# Exit 1: a new edge, a crate that is missing on one side, or a cycle.
# Exit 2: input it cannot read. It fails closed: a dependency it cannot parse
# (a multi-line inline table, `workspace = true`, a registry version, a path
# outside the workspace's crates) refuses; it never skips.
set -euo pipefail

me=check-graph.sh
die() {
  printf '%s: %s\n' "$me" "$1" >&2
  exit 2
}

if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
  die "needs bash 4 or later (this is bash ${BASH_VERSION})"
fi
export LC_ALL=C

repo='' allowed='' show=0
while [ $# -gt 0 ]; do
  case $1 in
    --repo) [ $# -ge 2 ] || die "--repo needs a directory"; repo=$2; shift 2 ;;
    --allowed) [ $# -ge 2 ] || die "--allowed needs a file"; allowed=$2; shift 2 ;;
    --show) show=1; shift ;;
    -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
if [ -z "$repo" ]; then
  repo=$(git rev-parse --show-toplevel 2>/dev/null) || die "not in a git repo; give --repo DIR"
fi
[ -d "$repo/crates" ] || die "no crates/ directory in $repo"
repo=$(cd "$repo" && pwd -P)
allowed=${allowed:-$repo/scripts/crate-graph/allowed.txt}

# strip_comment LINE: print LINE without a # comment outside double quotes.
strip_comment() {
  local line=$1 out='' c inq=0 i
  for ((i = 0; i < ${#line}; i++)); do
    c=${line:i:1}
    if [ "$c" = '"' ]; then
      inq=$((1 - inq))
    elif [ "$c" = '#' ] && [ "$inq" -eq 0 ]; then
      break
    fi
    out+=$c
  done
  out=${out#"${out%%[![:space:]]*}"}
  out=${out%"${out##*[![:space:]]}"}
  printf '%s' "$out"
}

declare -A name_of_dir=() dir_of_name=() deps=()
manifests=()
for m in "$repo"/crates/*/Cargo.toml; do
  [ -e "$m" ] || [ -L "$m" ] || continue
  manifests+=("$m")
done
[ "${#manifests[@]}" -gt 0 ] || die "no crates/*/Cargo.toml in $repo"

# First pass: the package name of each crate.
for m in "${manifests[@]}"; do
  [ -f "$m" ] && [ -r "$m" ] || die "cannot read $m"
  dir=$(cd "$(dirname "$m")" && pwd -P)
  section='' name=''
  while IFS= read -r raw || [ -n "$raw" ]; do
    line=$(strip_comment "$raw")
    if [[ $line =~ ^\[([^]]*)\]$ ]]; then
      section=${BASH_REMATCH[1]}
    elif [ "$section" = package ] && [[ $line =~ ^name[[:space:]]*=[[:space:]]*\"([A-Za-z0-9_-]+)\"$ ]]; then
      name=${BASH_REMATCH[1]}
    fi
  done <"$m"
  [ -n "$name" ] || die "no [package] name in $m"
  [ -z "${dir_of_name[$name]:-}" ] || die "two crates are named $name"
  name_of_dir[$dir]=$name
  dir_of_name[$name]=$dir
  deps[$name]=
done

# add_path_edge FROM MANIFEST_DIR PATH: add the edge FROM -> the crate at
# PATH, or fail closed. MANIFEST_DIR is the directory of FROM's Cargo.toml.
add_path_edge() {
  local target
  [ -d "$2/$3" ] || die "$2/Cargo.toml: the path dependency \"$3\" is not a directory"
  target=$(cd "$2/$3" && pwd -P)
  [ -n "${name_of_dir[$target]:-}" ] || die "$2/Cargo.toml: the path dependency \"$3\" is not a crate under crates/"
  add_edge "$1" "${name_of_dir[$target]}"
}

add_edge() {
  local from=$1 to=$2
  [ "$from" != "$to" ] || die "$from depends on itself"
  case " ${deps[$from]} " in
    *" $to "*) ;;
    *) deps[$from]="${deps[$from]:+${deps[$from]} }$to" ;;
  esac
}

is_dep_section() {
  [[ $1 =~ ^(target\..+\.)?(dependencies|dev-dependencies|build-dependencies)$ ]]
}
is_dep_table() {
  [[ $1 =~ ^(target\..+\.)?(dependencies|dev-dependencies|build-dependencies)\.([A-Za-z0-9_-]+)$ ]]
}

# Second pass: the dependencies.
path_re='(^|[,[:space:]])path[[:space:]]*=[[:space:]]*"([^"]*)"'
for m in "${manifests[@]}"; do
  dir=$(cd "$(dirname "$m")" && pwd -P)
  self=${name_of_dir[$dir]}
  section='' table='' table_path=''
  finish_table() {
    if [ -n "$table" ]; then
      [ -n "$table_path" ] || die "$m: [$section] has no path; only path dependencies are allowed"
      add_path_edge "$self" "$dir" "$table_path"
    fi
    table='' table_path=''
  }
  while IFS= read -r raw || [ -n "$raw" ]; do
    line=$(strip_comment "$raw")
    [ -n "$line" ] || continue
    if [[ $line =~ ^\[\[(.*)\]\]$ ]]; then
      finish_table
      section="[${BASH_REMATCH[1]}]"
      continue
    fi
    if [[ $line =~ ^\[([^]]*)\]$ ]]; then
      finish_table
      section=${BASH_REMATCH[1]}
      if is_dep_table "$section"; then
        table=${BASH_REMATCH[3]}
      fi
      continue
    fi
    if [ -n "$table" ]; then
      if [[ $line =~ ^path[[:space:]]*=[[:space:]]*\"([^\"]*)\"$ ]]; then
        table_path=${BASH_REMATCH[1]}
      elif [[ $line =~ ^workspace[[:space:]]*= ]]; then
        die "$m: [$section] uses workspace = true, which this script cannot resolve"
      fi
      continue
    fi
    is_dep_section "$section" || continue
    if [[ $line =~ ^([A-Za-z0-9_-]+)[[:space:]]*=[[:space:]]*\{(.*)\}$ ]]; then
      body=${BASH_REMATCH[2]}
      if [[ $body =~ (^|[,[:space:]])workspace[[:space:]]*= ]]; then
        die "$m: the dependency ${BASH_REMATCH[1]} uses workspace = true, which this script cannot resolve"
      fi
      if [[ $body =~ $path_re ]]; then
        add_path_edge "$self" "$dir" "${BASH_REMATCH[2]}"
      else
        die "$m: the dependency in \"$line\" has no path; only path dependencies are allowed"
      fi
    elif [[ $line =~ ^([A-Za-z0-9_-]+)\.path[[:space:]]*=[[:space:]]*\"([^\"]*)\"$ ]]; then
      add_path_edge "$self" "$dir" "${BASH_REMATCH[2]}"
    else
      die "$m: cannot read the dependency line \"$line\" in [$section]"
    fi
  done <"$m"
  finish_table
done

mapfile -t crates < <(printf '%s\n' "${!deps[@]}" | sort)

sorted() {
  if [ -n "$1" ]; then
    tr ' ' '\n' <<<"$1" | sort | tr '\n' ' ' | sed 's/ $//'
  fi
}

if [ "$show" -eq 1 ]; then
  for c in "${crates[@]}"; do
    s=$(sorted "${deps[$c]}")
    printf '%s:%s\n' "$c" "${s:+ $s}"
  done
  exit 0
fi

# The allowed edges.
[ -f "$allowed" ] && [ -r "$allowed" ] || die "cannot read the allowed-edges file $allowed"
declare -A allow=()
while IFS= read -r raw || [ -n "$raw" ]; do
  line=${raw%%#*}
  line=${line#"${line%%[![:space:]]*}"}
  line=${line%"${line##*[![:space:]]}"}
  [ -n "$line" ] || continue
  if ! [[ $line =~ ^([A-Za-z0-9_-]+):([A-Za-z0-9_[:space:]-]*)$ ]]; then
    die "$allowed: cannot read the line \"$raw\""
  fi
  c=${BASH_REMATCH[1]}
  [ -z "${allow[$c]+x}" ] || die "$allowed: $c is listed twice"
  read -r -a list <<<"${BASH_REMATCH[2]}"
  allow[$c]=" ${list[*]} "
done <"$allowed"

problems=0
problem() {
  printf '%s: %s\n' "$me" "$1" >&2
  problems=$((problems + 1))
}

for c in "${crates[@]}"; do
  if [ -z "${allow[$c]+x}" ]; then
    problem "the crate $c is not in $allowed; add a line for it"
    continue
  fi
  for d in ${deps[$c]}; do
    case ${allow[$c]} in
      *" $d "*) ;;
      *)
        if [ "$d" = oska-sys ]; then
          problem "new edge $c -> $d: it widens access to system calls, so it needs an explicit reason before you allow it"
        else
          problem "new edge $c -> $d is not in $allowed"
        fi
        ;;
    esac
  done
done
for c in "${!allow[@]}"; do
  if [ -z "${deps[$c]+x}" ]; then
    problem "the crate $c in $allowed is missing from crates/"
  fi
  for d in ${allow[$c]}; do
    if [ -z "${deps[$d]+x}" ]; then
      problem "the edge $c -> $d in $allowed names a missing crate $d"
    fi
  done
done

# Cycles: a depth-first search over the current graph.
# `stack` holds the crates on the current path; a cycle is printed from the
# crate it returns to.
declare -A state=()
stack=()
visit() {
  local c=$1 d i text
  state[$c]=open
  stack+=("$c")
  for d in ${deps[$c]}; do
    if [ "${state[$d]:-}" = open ]; then
      text=
      for ((i = ${#stack[@]} - 1; i >= 0; i--)); do
        text="${stack[i]} -> ${text:-$d}"
        [ "${stack[i]}" != "$d" ] || break
      done
      problem "cycle: $text"
    elif [ -z "${state[$d]:-}" ]; then
      visit "$d"
    fi
  done
  unset 'stack[${#stack[@]}-1]'
  state[$c]=closed
}
for c in "${crates[@]}"; do
  [ -n "${state[$c]:-}" ] || visit "$c"
done

if [ "$problems" -gt 0 ]; then
  printf '%s: %s problem(s) in the crate graph\n' "$me" "$problems" >&2
  exit 1
fi
printf 'crate graph: %s crates, every edge allowed, no cycle\n' "${#crates[@]}"
