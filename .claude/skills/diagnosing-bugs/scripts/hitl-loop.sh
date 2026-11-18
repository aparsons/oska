#!/usr/bin/env bash
# Human-in-the-loop feedback loop template.
#
# Copy this file to a temporary path, then replace the steps in main().
#   hitl_step "<instruction>"     show an instruction and wait for Enter
#   hitl_ask NAME "<question>"    ask a question and store the answer as NAME
# At the end, the script prints each captured value as NAME=value.
#
# Never ask for a password or a token. A sign-in step is an instruction only.
set -euo pipefail

if [[ ! -t 0 ]]; then
  echo "hitl-loop: standard input is not a terminal; a person must run this script" >&2
  exit 2
fi

declare -a HITL_NAMES=()
declare -A HITL_VALUES=()
HITL_STEP_NO=0

hitl_step() {
  if [[ $# -ne 1 ]]; then
    echo "hitl_step: needs exactly one instruction" >&2
    exit 2
  fi
  HITL_STEP_NO=$((HITL_STEP_NO + 1))
  printf '\n[%d] %s\n' "$HITL_STEP_NO" "$1"
  read -r -p "Press Enter when done. " _
}

hitl_ask() {
  if [[ $# -ne 2 ]]; then
    echo "hitl_ask: needs a NAME and a question" >&2
    exit 2
  fi
  local name=$1 question=$2 answer
  if [[ ! $name =~ ^[A-Z_][A-Z0-9_]*$ ]]; then
    echo "hitl_ask: bad name '$name'; it must match ^[A-Z_][A-Z0-9_]*\$" >&2
    exit 2
  fi
  case $name in
    *PASSWORD* | *PASSWD* | *TOKEN* | *SECRET* | *KEY* | *CREDENTIAL*)
      echo "hitl_ask: refusing to capture '$name'; never capture secrets" >&2
      exit 2
      ;;
  esac
  HITL_STEP_NO=$((HITL_STEP_NO + 1))
  printf '\n[%d] %s\n' "$HITL_STEP_NO" "$question"
  read -r -p "> " answer
  if [[ -z ${HITL_VALUES[$name]+set} ]]; then
    HITL_NAMES+=("$name")
  fi
  HITL_VALUES[$name]=$answer
}

hitl_report() {
  printf '\n--- captured values ---\n'
  local name
  for name in "${HITL_NAMES[@]}"; do
    printf '%s=%s\n' "$name" "${HITL_VALUES[$name]}"
  done
}

main() {
  # Replace these example steps with the steps for your bug.
  hitl_step "Open http://localhost:8080 in a browser and sign in with the test account."
  hitl_step "Click 'Export' on the first report."
  hitl_ask EXPORT_SHOWN "Did the export dialog open? (yes/no)"
  hitl_ask ERROR_TEXT "Copy the error text, or leave it empty if there is none."
  hitl_report
}

main "$@"
