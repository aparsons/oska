#!/usr/bin/env bash
# Interactive setup wizard. Made from the wizard skill template.
#
# Run it from the repository root, in a terminal:
#   ./path/to/this-script.sh
#
# The script guides you through steps that only a human can do. It saves
# public values and secrets to the env file, and to GitHub Actions secrets and
# variables. It never prints a secret value. It prints only key names.
set -euo pipefail
# Tracing would print secret values. Turn it off, also under "bash -x".
set +x
umask 077

# ======================================================================
# LIBRARY. Do not edit this section. Edit the STAGES section at the end.
# ======================================================================

# Defaults. The STAGES section sets the real values.
WIZARD_TITLE="Setup wizard"
STAGE_COUNT=1
ENV_FILE=".env"
MAX_TRIES=3

_STAGE_INDEX=0
_ENV_CHECKED=0
_GH_STATE="unknown"
_TMP_FILE=""
_WRITTEN=()
_SKIPPED=()

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  C_BOLD=$'\033[1m'
  C_DIM=$'\033[2m'
  C_RED=$'\033[31m'
  C_GREEN=$'\033[32m'
  C_YELLOW=$'\033[33m'
  C_BLUE=$'\033[34m'
  C_RESET=$'\033[0m'
else
  C_BOLD=""
  C_DIM=""
  C_RED=""
  C_GREEN=""
  C_YELLOW=""
  C_BLUE=""
  C_RESET=""
fi

_cleanup() {
  if [[ -n "$_TMP_FILE" && -e "$_TMP_FILE" ]]; then
    rm -f -- "$_TMP_FILE"
  fi
}
trap _cleanup EXIT
trap 'printf "\n"; _cleanup; exit 130' INT TERM

say() { printf '%s\n' "$*"; }
step() { printf '%s->%s %s\n' "$C_BLUE" "$C_RESET" "$*"; }
note() { printf '%s   note: %s%s\n' "$C_DIM" "$*" "$C_RESET"; }
warn() { printf '%sWARNING:%s %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
die() {
  printf '%sERROR:%s %s\n' "$C_RED" "$C_RESET" "$*" >&2
  exit 1
}

_no_input() { die "No more input. Stopping. Run the wizard again in a terminal."; }
_written() { _WRITTEN+=("$1"); }
skip_item() {
  _SKIPPED+=("$1")
  warn "Skipped: $1"
}

# Stop if a key or name is not a safe identifier.
valid_key() {
  if [[ ! "${1-}" =~ ^[A-Z_][A-Z0-9_]*$ ]]; then
    die "Invalid key name: $(printf '%q' "${1-}"). Use A-Z, 0-9 and _ only."
  fi
}

banner() {
  printf '%s%s%s\n' "$C_BOLD" "$WIZARD_TITLE" "$C_RESET"
  printf 'This wizard has %d stage(s). Press Ctrl-C to stop at any time.\n' "$STAGE_COUNT"
  printf 'Values go to %s and to the places each stage names.\n\n' "$ENV_FILE"
}

stage() {
  _STAGE_INDEX=$((_STAGE_INDEX + 1))
  if [[ -t 1 ]]; then
    clear 2>/dev/null || printf '\033[2J\033[H'
  fi
  printf '%sStage %d/%d · %s%s\n\n' "$C_BOLD" "$_STAGE_INDEX" "$STAGE_COUNT" "$*" "$C_RESET"
}

open_url() {
  local url="$1"
  step "Open: $url"
  if command -v wslview >/dev/null 2>&1; then
    wslview "$url" >/dev/null 2>&1 && return 0
  elif command -v explorer.exe >/dev/null 2>&1; then
    # explorer.exe returns 1 also when it opens the page.
    explorer.exe "$url" >/dev/null 2>&1 || true
    return 0
  elif command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$url" >/dev/null 2>&1 && return 0
  elif [[ "$(uname -s)" == "Darwin" ]] && command -v open >/dev/null 2>&1; then
    open "$url" >/dev/null 2>&1 && return 0
  fi
  say "Open this URL in your browser: $url"
}

pause() {
  local _reply
  IFS= read -r -p "Press Enter to continue... " _reply || _no_input
}

# confirm "Question" -> 0 for yes, 1 for no. The default is no.
confirm() {
  local reply
  IFS= read -r -p "$1 [y/N] " reply || _no_input
  [[ "$reply" =~ ^[Yy]([Ee][Ss])?$ ]]
}

# wiz_value KEY -> the value that ask or ask_secret captured for KEY.
wiz_value() {
  valid_key "$1"
  local var="WIZ_VAL_$1"
  printf '%s' "${!var-}"
}

_store() {
  printf -v "WIZ_VAL_$1" '%s' "$2"
}

# env_get KEY -> the current value of KEY in the env file, or nothing.
env_get() {
  local key="$1" line=""
  valid_key "$key"
  [[ -f "$ENV_FILE" ]] || return 0
  line=$(grep -E "^(export[[:space:]]+)?${key}=" -- "$ENV_FILE" | tail -n 1) || true
  [[ -n "$line" ]] || return 0
  line="${line#*=}"
  if [[ ${#line} -ge 2 ]]; then
    case "$line" in
      \'*\' | \"*\") line="${line:1:${#line}-2}" ;;
    esac
  fi
  printf '%s' "$line"
}

# ask KEY "Prompt" -> visible input. Offers the current env value as default.
ask() {
  local key="$1" prompt="$2" current reply tries=0
  valid_key "$key"
  current=$(env_get "$key")
  while ((tries < MAX_TRIES)); do
    tries=$((tries + 1))
    if [[ -n "$current" ]]; then
      IFS= read -r -p "$prompt [$current]: " reply || _no_input
      reply="${reply:-$current}"
    else
      IFS= read -r -p "$prompt: " reply || _no_input
    fi
    if [[ -n "$reply" ]]; then
      _store "$key" "$reply"
      return 0
    fi
    warn "The value is empty. Type a value."
  done
  _store "$key" ""
  skip_item "$key (no value after $MAX_TRIES tries)"
}

# ask_secret KEY "Prompt" -> hidden input. Can keep the current value unseen.
ask_secret() {
  local key="$1" prompt="$2" current reply tries=0
  valid_key "$key"
  current=$(env_get "$key")
  if [[ -n "$current" ]] && confirm "$key already has a value in $ENV_FILE. Keep it?"; then
    _store "$key" "$current"
    return 0
  fi
  while ((tries < MAX_TRIES)); do
    tries=$((tries + 1))
    IFS= read -r -s -p "$prompt (input is hidden): " reply || _no_input
    printf '\n'
    if [[ -n "$reply" ]]; then
      _store "$key" "$reply"
      say "Captured $key."
      return 0
    fi
    warn "The value is empty. Paste the value."
  done
  _store "$key" ""
  skip_item "$key (no value after $MAX_TRIES tries)"
}

# Make sure git ignores the env file before the first write.
_env_guard() {
  if [[ "$_ENV_CHECKED" -eq 1 ]]; then
    return 0
  fi
  if ! command -v git >/dev/null 2>&1 || ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    warn "Cannot use git to check that $ENV_FILE is ignored."
    confirm "Write values to $ENV_FILE anyway?" || die "Stopped. Nothing was written to $ENV_FILE."
  elif ! git check-ignore -q -- "$ENV_FILE"; then
    warn "Git does not ignore $ENV_FILE. A commit can publish its secrets."
    say "Add $ENV_FILE to .gitignore, then run the wizard again."
    confirm "Write values to $ENV_FILE anyway?" || die "Stopped. Nothing was written to $ENV_FILE."
  fi
  _ENV_CHECKED=1
}

# write_env KEY VALUE -> add or replace one KEY='VALUE' line in the env file.
write_env() {
  local key="$1" value="$2" dir rc=0
  valid_key "$key"
  if [[ -z "$value" ]]; then
    skip_item "$key -> $ENV_FILE (no value)"
    return 0
  fi
  case "$value" in
    *"'"* | *$'\n'* | *$'\r'*)
      skip_item "$key -> $ENV_FILE (the value has a quote or a line break; write it by hand)"
      return 0
      ;;
  esac
  _env_guard
  dir=$(dirname -- "$ENV_FILE")
  _TMP_FILE=$(mktemp "$dir/.wizard-env.XXXXXX")
  chmod 600 -- "$_TMP_FILE"
  if [[ -f "$ENV_FILE" ]]; then
    grep -v -E "^(export[[:space:]]+)?${key}=" -- "$ENV_FILE" >"$_TMP_FILE" || rc=$?
    if [[ "$rc" -gt 1 ]]; then
      die "Cannot read $ENV_FILE. Nothing was written."
    fi
  fi
  printf "%s='%s'\n" "$key" "$value" >>"$_TMP_FILE"
  mv -f -- "$_TMP_FILE" "$ENV_FILE"
  _TMP_FILE=""
  chmod 600 -- "$ENV_FILE"
  _written "$key -> $ENV_FILE"
}

# Check once that gh is installed, signed in, and points at the right repo.
_gh_ready() {
  local repo
  case "$_GH_STATE" in
    ok) return 0 ;;
    no) return 1 ;;
  esac
  _GH_STATE="no"
  if ! command -v gh >/dev/null 2>&1; then
    warn "The gh CLI is not installed."
    return 1
  fi
  if ! gh auth status >/dev/null 2>&1; then
    warn "The gh CLI is not signed in. Run: gh auth login"
    return 1
  fi
  if ! repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null) || [[ -z "$repo" ]]; then
    warn "gh cannot find the GitHub repository for this folder."
    return 1
  fi
  if ! confirm "Write GitHub secrets and variables to $repo?"; then
    warn "You did not confirm $repo."
    return 1
  fi
  _GH_STATE="ok"
}

# Stop if a GitHub environment name is not safe. An empty name is allowed.
_valid_env_name() {
  if [[ -n "${1-}" && ! "$1" =~ ^[A-Za-z0-9_.-]+$ ]]; then
    die "Invalid environment name: $(printf '%q' "$1")."
  fi
}

# _gh_secret_put NAME ENV LABEL -> reads the value from stdin and sets it.
# With ENV, the secret goes to that GitHub environment, not to the repository.
_gh_secret_put() {
  local name="$1" env="$2" label="$3"
  local -a scope=()
  [[ -n "$env" ]] && scope=(--env "$env")
  if gh secret set "$name" ${scope[@]+"${scope[@]}"} >/dev/null; then
    _written "$label"
  else
    skip_item "$label (gh secret set failed)"
  fi
}

# set_secret NAME [KEY] [ENV] -> GitHub Actions secret NAME from the value of
# KEY. KEY defaults to NAME. ENV puts the secret in that GitHub environment.
# The value goes to gh through stdin, not as an argument.
set_secret() {
  local name="$1" key="${2:-$1}" env="${3:-}" value label
  valid_key "$name"
  valid_key "$key"
  _valid_env_name "$env"
  label="GitHub secret $name${env:+ (environment $env)}"
  value=$(wiz_value "$key")
  if [[ -z "$value" ]]; then
    skip_item "$label (no value)"
    return 0
  fi
  if ! _gh_ready; then
    skip_item "$label (set it by hand: gh secret set $name${env:+ --env $env})"
    return 0
  fi
  # Process substitution keeps _gh_secret_put in this shell, so it can record.
  _gh_secret_put "$name" "$env" "$label" < <(printf '%s' "$value")
}

# set_secret_file NAME FILE [ENV] -> GitHub Actions secret NAME from the bytes
# of FILE. Use it for a multi-line value, such as a private key. The file goes
# to gh through stdin. The caller makes FILE with mktemp and removes it.
set_secret_file() {
  local name="$1" file="$2" env="${3:-}" label
  valid_key "$name"
  _valid_env_name "$env"
  label="GitHub secret $name${env:+ (environment $env)}"
  if [[ ! -s "$file" ]]; then
    skip_item "$label (no value in the file)"
    return 0
  fi
  if ! _gh_ready; then
    skip_item "$label (set it by hand: gh secret set $name${env:+ --env $env} < FILE)"
    return 0
  fi
  _gh_secret_put "$name" "$env" "$label" <"$file"
}

# set_var NAME VALUE -> GitHub Actions variable. Use it for public values only.
set_var() {
  local name="$1" value="$2"
  valid_key "$name"
  if [[ -z "$value" ]]; then
    skip_item "GitHub variable $name (no value)"
    return 0
  fi
  if ! _gh_ready; then
    skip_item "GitHub variable $name (set it by hand: gh variable set $name)"
    return 0
  fi
  if gh variable set "$name" --body "$value" >/dev/null; then
    _written "GitHub variable $name"
  else
    skip_item "GitHub variable $name (gh variable set failed)"
  fi
}

finish() {
  local item
  if [[ -t 1 ]]; then
    clear 2>/dev/null || printf '\033[2J\033[H'
  fi
  printf '%s%s: summary%s\n\n' "$C_BOLD" "$WIZARD_TITLE" "$C_RESET"
  if [[ ${#_WRITTEN[@]} -gt 0 ]]; then
    printf '%sWritten:%s\n' "$C_GREEN" "$C_RESET"
    for item in "${_WRITTEN[@]}"; do
      printf '  - %s\n' "$item"
    done
  else
    say "Nothing was written."
  fi
  if [[ ${#_SKIPPED[@]} -gt 0 ]]; then
    printf '\n%sDo these by hand:%s\n' "$C_YELLOW" "$C_RESET"
    for item in "${_SKIPPED[@]}"; do
      printf '  - %s\n' "$item"
    done
  fi
}

# ======================================================================
# STAGES. Edit below this line. Keep the call to stages on the last line.
# ======================================================================

WIZARD_TITLE="Example: connect Acme Mail"
STAGE_COUNT=1
ENV_FILE=".env"

stages() {
  banner
  pause

  # Stage 1. Acme Mail is a fictional service. Replace it with a real stage.
  stage "Acme Mail API key"
  say "The app sends email through Acme Mail. It needs an API key."
  open_url "https://dashboard.acme-mail.example/settings/api-keys"
  step "Sign in to the Acme Mail dashboard."
  step "Click Settings, then API keys, then New key."
  step "Give the key the 'Send' scope only."
  note "The dashboard shows the key one time. Copy it before you close the page."
  ask_secret ACME_MAIL_API_KEY "Paste the Acme Mail API key"
  ask ACME_MAIL_FROM "Sender address for email from the app"
  write_env ACME_MAIL_API_KEY "$(wiz_value ACME_MAIL_API_KEY)"
  write_env ACME_MAIL_FROM "$(wiz_value ACME_MAIL_FROM)"
  set_secret ACME_MAIL_API_KEY
  set_var ACME_MAIL_FROM "$(wiz_value ACME_MAIL_FROM)"

  # An irreversible stage must ask first, for example:
  #   if confirm "Revoke the old Acme Mail key now? This cannot be undone."; then
  #     step "Click Revoke next to the old key."
  #     pause
  #   else
  #     skip_item "Revoke the old Acme Mail key"
  #   fi

  finish
}

stages "$@"
