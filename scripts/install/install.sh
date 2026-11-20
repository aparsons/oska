#!/bin/sh
# Install oskac, the Oska compiler and runner, on Linux or macOS.
#
#   curl -fsSL https://oskalang.dev/install.sh | sh
#   curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.1.0 --json
#
# Options:
#   --version X        install release X (0.1.0 or v0.1.0) instead of the latest
#   --allow-downgrade  with --version, install X even if it is older than
#                      the installed oskac
#   --check            report the installed and the latest version, and
#                      install nothing
#   --uninstall        remove what the install receipt lists, and nothing else
#   --add-to-path      also add one marked line to a shell profile, so that
#                      oskac is on your PATH (nothing else is edited)
#   --hook <dir>       also install the Claude Code check-after-edit hook
#                      (not the Stop gate) into the project in <dir>
#   --json             print one JSON object on stdout, and nothing else
#   --help             print this help
#
# Environment:
#   OSKA_HOME      install into $OSKA_HOME/bin (default: ~/.oska)
#   OSKA_SITE_URL  where to download from (default: https://oskalang.dev);
#                  only https:// and file:// URLs are accepted
#   OSKA_LIBC      on Linux, glibc or musl: install that build rather than
#                  the one chosen for this system (see choose_linux_build)
#
# Before installing anything it checks that SHA256SUMS carries a valid SSH
# signature by the release key pinned below, that its signed first line
# names the release asked for (`# oska-release vX.Y.Z`, so an old release
# cannot be passed off as a newer one), that the binary's SHA-256 is the one
# SHA256SUMS lists, and that the binary reports that version. If any check
# fails, or anything else goes wrong, it installs nothing and exits
# non-zero. It never prompts.
#
# Without --version it installs the latest release. Once the site publishes
# a signed release index, the installer is pinned to require it and takes
# the latest release from release-index.json alone: signed by the release
# key in the `oska-index` namespace, not expired, and with a serial no
# lower than the last one this install accepted. Before that, it reads the
# unsigned latest.json.
#
# Re-running it updates oskac in place. It never installs an older release
# than the installed one unless asked with --version and --allow-downgrade.
# It records what it installed in $OSKA_HOME/receipt.json, which is
# advisory: no check above reads it. --uninstall removes the files the
# receipt lists, if they are unchanged, and downloads nothing.
#
# By default nothing outside the Oska home is touched. --add-to-path and
# --hook are opt-in, and the receipt records what they wrote, so that
# --uninstall can undo it: the profile line only if that exact line is
# still there, the hook settings file only if unchanged.

set -u

# The release signing key, pinned when the site is built
# (scripts/install/pin.sh). Signatures by any other key are refused.
RELEASE_KEY='@OSKA_RELEASE_KEY@'
NAMESPACE=oska-release
# Whether the latest release must come from the signed release index, also
# pinned when the site is built: `yes` once the site publishes a verified
# index, `no` before, when the installer reads the unsigned latest.json.
# Anything else, such as the unreplaced placeholder, is refused.
INDEX_REQUIRED='@OSKA_INDEX_REQUIRED@'
INDEX_NAMESPACE=oska-index
# The comment that ends the line --add-to-path writes, so that --uninstall
# removes only a line it can tell is the installer's own.
PROFILE_MARK='# added by the Oska installer'
DEFAULT_SITE_URL=https://oskalang.dev
# The glibc of the Linux build runners (.github/workflows/release.yml): the
# glibc build needs it or newer.
MIN_GLIBC=2.35

json=0
add_to_path=0
hook_dir=
profile_new=
profile_line=
profile_seen=0
settings_write=0
settings_existed=0
settings_merge_needed=false
tmp=
stage=
version_tmp=
receipt_tmp=
# While the install is committed (see commit_install), a signal waits.
committing=0
pending=

json_str() {
  tab=$(printf '\t')
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e "s/$tab/\\\\t/g" | tr -d '\r\n')"
}

cleanup() {
  if [ -n "$tmp" ]; then
    rm -rf "$tmp"
  fi
  if [ -n "$stage" ]; then
    rm -rf "$stage"
  fi
  if [ -n "$version_tmp" ]; then
    rm -f "$version_tmp"
  fi
  if [ -n "$receipt_tmp" ]; then
    rm -f "$receipt_tmp"
  fi
}

# on_signal <signal>: clean up, then die of that same signal, so whatever
# ran the installer sees how it ended. While the install is committed, the
# signal waits until it is, so an interruption never leaves the receipt
# describing a binary that is not the one installed.
on_signal() {
  if [ "$committing" = 1 ]; then
    pending=$1
    return
  fi
  cleanup
  trap - "$1" EXIT
  kill -s "$1" $$
}

# fail <code> <message>: report the failure and exit non-zero. Usage errors
# exit 2, everything else 1.
fail() {
  code=$1
  shift
  if [ "$json" = 1 ]; then
    printf '{"ok": false, "error": %s, "message": %s}\n' "$(json_str "$code")" "$(json_str "$*")"
  else
    echo "install.sh: $*" >&2
  fi
  cleanup
  if [ "$code" = usage ]; then
    exit 2
  fi
  exit 1
}

say() {
  if [ "$json" = 0 ]; then
    echo "$@"
  fi
}

usage() {
  cat <<'EOF'
Install oskac to ~/.oska/bin, verifying its checksum and release signature.

usage: curl -fsSL https://oskalang.dev/install.sh | sh -s -- [options]

  --version X        install release X (0.1.0 or v0.1.0) instead of the latest
  --allow-downgrade  with --version, install X even if it is older than
                     the installed oskac
  --check            report the installed and the latest version, and
                     install nothing
  --uninstall        remove what the install receipt lists, and nothing else
  --add-to-path      also add one marked line to a shell profile, so that
                     oskac is on your PATH (nothing else is edited)
  --hook <dir>       also install the Claude Code check-after-edit hook
                     (not the Stop gate) into the project in <dir>
  --json             print one JSON object on stdout, and nothing else
  --help             print this help

Environment: OSKA_HOME (default ~/.oska), OSKA_SITE_URL (https:// only),
OSKA_LIBC (Linux: glibc or musl, to choose the build rather than detect it).
EOF
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail missing_tool "$1 is required${2:+ ($2)}"
}

# fetch <url> <file>: download over HTTPS only, or read a local file:// URL.
fetch() {
  case $1 in
    https://*)
      curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent \
        --location --retry 3 --output "$2" "$1" ;;
    file://*)
      curl --proto '=file' --fail --silent --output "$2" "$1" ;;
    *)
      return 1 ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    openssl dgst -sha256 -r "$1" | awk '{print $1}'
  fi
}

is_tag() {
  printf '%s\n' "$1" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?'
}

detect_platform() {
  case $(uname -s) in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT)
      fail unsupported_platform "on Windows, use install.ps1: irm $DEFAULT_SITE_URL/install.ps1 | iex" ;;
    *) fail unsupported_platform "there is no oskac build for $(uname -s)" ;;
  esac
  case $(uname -m) in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail unsupported_platform "there is no oskac build for $(uname -m) on $os" ;;
  esac
  # An x86_64 shell under Rosetta on Apple silicon gets the native build.
  if [ "$os" = macos ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ]; then
    arch=aarch64
  fi
  platform=$os-$arch
  if [ "$os" = linux ]; then
    choose_linux_build
  fi
}

# check_libc_choice: OSKA_LIBC, if set and not empty, is glibc or musl.
# Anything else is refused before anything is downloaded or removed, in
# every mode and on every platform, so a typo never installs a build nobody
# asked for.
check_libc_choice() {
  case ${OSKA_LIBC:-} in
    '' | glibc | musl) ;;
    *) fail usage "OSKA_LIBC must be glibc or musl (or unset, to choose for this system)" ;;
  esac
}

# glibc_found: succeeds only when this system's C library is shown to be
# glibc $MIN_GLIBC or newer. Anything it cannot read for certain (no
# getconf, an ldd that is not glibc's, a version that is not
# <major>.<minor> or <major>.<minor>.<patch>) fails, which chooses the musl build: that one runs on
# any Linux, so not knowing is safe.
glibc_found() {
  glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || glibc=
  case $glibc in
    glibc\ *) glibc=${glibc#glibc } ;;
    *)
      # No getconf: glibc's ldd names itself on its first line, and ends
      # it with the version.
      glibc=$(ldd --version 2>&1 | head -n 1) || glibc=
      case $glibc in
        *GLIBC* | *"GNU libc"*) glibc=${glibc##* } ;;
        *) return 1 ;;
      esac ;;
  esac
  glibc_major=${glibc%%.*}
  glibc_minor=${glibc#*.}
  # A patch part, if any, is checked and then set aside: the build needs
  # only the major and minor version.
  case $glibc_minor in
    *.*)
      glibc_patch=${glibc_minor#*.}
      glibc_minor=${glibc_minor%%.*}
      case $glibc_patch in
        '' | *[!0-9]* | ?????*) return 1 ;;
      esac ;;
  esac
  # Each part is 1 to 4 digits, so the comparisons below cannot overflow.
  case $glibc_major in
    '' | *[!0-9]* | ?????*) return 1 ;;
  esac
  case $glibc_minor in
    '' | *[!0-9]* | ?????*) return 1 ;;
  esac
  [ "$glibc_major" -gt "${MIN_GLIBC%%.*}" ] ||
    { [ "$glibc_major" -eq "${MIN_GLIBC%%.*}" ] && [ "$glibc_minor" -ge "${MIN_GLIBC#*.}" ]; }
}

# choose_linux_build: sets $linux_build to gnu or musl, the release asset's
# suffix (ADR 0022), and $libc_forced to whether OSKA_LIBC chose it. The
# glibc build is linked against glibc $MIN_GLIBC (built on Ubuntu 22.04),
# so it is chosen only where glibc_found; the static musl build runs on any
# Linux, so it is chosen everywhere else. A forced glibc build where it
# cannot run is refused rather than installed.
choose_linux_build() {
  libc_forced=false
  case ${OSKA_LIBC:-} in
    musl)
      linux_build=musl
      libc_forced=true ;;
    glibc)
      glibc_found ||
        fail unsupported_libc "OSKA_LIBC=glibc asks for the glibc build, which needs glibc $MIN_GLIBC or newer, and this system does not have it; unset OSKA_LIBC, or set OSKA_LIBC=musl, for the musl build, which runs on any Linux"
      linux_build=gnu
      libc_forced=true ;;
    *)
      if glibc_found; then
        linux_build=gnu
      else
        linux_build=musl
      fi ;;
  esac
}

# sums_list <file>: succeeds if the downloaded SHA256SUMS, whose signature
# has been verified, has a line for <file>.
sums_list() {
  awk -v f="$1" 'NF == 2 && $2 == f {found = 1} END {exit !found}' "$tmp/SHA256SUMS"
}

# The install receipt: what this run installed, as JSON in the Oska home.
# It is advisory. Verification never reads it; it only records what counts
# as installed, what an uninstall may remove and the highest index serial
# seen. Written to a new file beside it, $receipt_tmp, which commit_install
# renames into place. The version file is prepared the same way, first.
prepare_records() {
  collect_extras
  version_tmp=$(mktemp "$home/.version.XXXXXXXX") ||
    fail install_failed "cannot write a version file in $home"
  { printf '%s\n' "$version" >"$version_tmp" && chmod 644 "$version_tmp"; } ||
    fail install_failed "cannot write $version_tmp"
  receipt_tmp=$(mktemp "$home/.receipt.XXXXXXXX") ||
    fail install_failed "cannot write a receipt in $home"
  {
    printf '{\n'
    printf '  "schema": 1,\n'
    printf '  "version": %s,\n' "$(json_str "$version")"
    printf '  "tag": %s,\n' "$(json_str "$tag")"
    printf '  "platform": %s,\n' "$(json_str "$platform")"
    if [ "$os" = linux ]; then
      printf '  "linux_build": %s,\n' "$(json_str "$linux_build")"
      printf '  "libc_forced": %s,\n' "$libc_forced"
    fi
    printf '  "installed_at": %s,\n' "$(json_str "$(date -u +%Y-%m-%dT%H:%M:%SZ)")"
    printf '  "site": %s,\n' "$(json_str "$site")"
    printf '  "key_fingerprint": %s,\n' "$(json_str "$fingerprint")"
    printf '  "sha256": %s,\n' "$(json_str "$actual")"
    printf '  "installer": "sh",\n'
    printf '  "index_serial": %s,\n' "$receipt_serial"
    printf '  "files": [\n'
    printf '    {"path": %s, "kind": "binary", "sha256": %s},\n' "$(json_str "$target")" "$(json_str "$actual")"
    printf '    {"path": %s, "kind": "record", "sha256": %s},\n' "$(json_str "$version_file")" "$(json_str "$(sha256 "$version_tmp")")"
    printf '%s' "$extras"
    printf '    {"path": %s, "kind": "record"}\n' "$(json_str "$receipt")"
    printf '  ]\n'
    printf '}\n'
  } >"$receipt_tmp" || fail install_failed "cannot write $receipt_tmp"
}

# add_extra <kind> <path> <sha256> <line> <project>: a receipt entry for
# what --add-to-path or --hook wrote, as one more line of $extras, unless
# it is there already: the entries of the receipt being replaced are added
# first, so a re-run without the flags keeps what an earlier run did.
add_extra() {
  case "$extra_keys" in
    *"
$1 $2 $4
"*) return ;;
  esac
  extra_keys="$extra_keys$1 $2 $4
"
  extras="$extras    {\"path\": $(json_str "$2"), \"kind\": $(json_str "$1")"
  if [ -n "$3" ]; then
    extras="$extras, \"sha256\": $(json_str "$3")"
  fi
  if [ -n "$4" ]; then
    extras="$extras, \"line\": $(json_str "$4")"
  fi
  if [ -n "$5" ]; then
    extras="$extras, \"project\": $(json_str "$5")"
  fi
  extras="$extras},
"
}

# $extras: the receipt entries, after the binary and the version file, for
# what an earlier run and this one wrote outside the Oska home.
collect_extras() {
  extras=
  extra_keys="
"
  tab=$(printf '\t')
  if [ -f "$receipt" ] && [ ! -L "$receipt" ]; then
    previous_entries=$(json_read files "$receipt" 2>/dev/null) || previous_entries=
    while IFS="$tab" read -r e_kind e_sum e_path e_line e_project; do
      case ${e_kind#k} in
        profile_line | hook_script | hook_settings)
          add_extra "${e_kind#k}" "${e_path#p}" "${e_sum#s}" "${e_line#l}" "${e_project#j}" ;;
      esac
    done <<EOF
$previous_entries
EOF
  fi
  if [ -n "$profile_new" ]; then
    add_extra profile_line "$profile_new" "" "$profile_line" ""
  fi
  if [ "$settings_write" = 1 ]; then
    add_extra hook_settings "$hook_settings" "$settings_sum" "" "$hook_dir"
  elif [ -n "$hook_dir" ] && [ "$settings_existed" = 1 ]; then
    # Settings the installer did not create, so that --uninstall reports
    # them: it never removes a file without a SHA-256 here.
    add_extra hook_settings "$hook_settings" "" "" "$hook_dir"
  fi
}

# Moves the version file, the verified binary and then the receipt into
# place, each an atomic rename, with signals held until all three are done
# (see on_signal). Only SIGKILL, or a crash, can stop it in between, and
# then the receipt left behind describes the binary that was replaced, by
# its SHA-256, so previous_version does not believe it and reads the
# version file, already the new one. An uninstall keeps the files it no
# longer matches. Either way a re-run repairs it.
commit_install() {
  committing=1
  mv -f "$version_tmp" "$version_file" || fail install_failed "cannot write $version_file"
  version_tmp=
  mv -f "$staged" "$target" || fail install_failed "cannot write $target"
  mv -f "$receipt_tmp" "$receipt" || fail install_failed "cannot write $receipt"
  receipt_tmp=
  write_extras
  committing=0
  if [ -n "$pending" ]; then
    on_signal "$pending"
  fi
}

# plan_profile: for --add-to-path, the one profile to add a line to,
# chosen from the login shell, and the line. Nothing is written yet. With
# the bin directory on PATH already there is nothing to add.
plan_profile() {
  [ -n "${HOME:-}" ] ||
    fail install_failed "--add-to-path needs HOME, to find your shell profile; add $bin_dir to your PATH yourself"
  nl='
'
  case $bin_dir in
    *[\"\$\`\\]* | *"$nl"*)
      fail install_failed "$bin_dir has a character that cannot go in a profile line safely; add it to your PATH yourself" ;;
  esac
  case ":${PATH:-}:" in
    *":$bin_dir:"*) return ;;
  esac
  login_shell=${SHELL:-}
  case ${login_shell##*/} in
    zsh) name=.zshrc ;;
    bash)
      if [ "$(uname -s)" = Darwin ]; then
        name=.bash_profile
      else
        name=.bashrc
      fi ;;
    *) name=.profile ;;
  esac
  profile=${HOME%/}/$name
  profile_line="export PATH=\"$bin_dir:\$PATH\" $PROFILE_MARK"
  if [ -L "$profile" ] || { [ -e "$profile" ] && [ ! -f "$profile" ]; }; then
    fail install_failed "$profile is not a regular file, so the installer will not edit it; add $bin_dir to your PATH yourself"
  fi
  if symlink_above "$profile"; then
    fail install_failed "$profile is reached through the symlink $linked, which --uninstall would refuse; add $bin_dir to your PATH yourself"
  fi
  profile_seen=0
  if [ -f "$profile" ] && grep -Fx -- "$profile_line" "$profile" >/dev/null 2>&1; then
    # Already there (an earlier run's, recorded in the receipt, or yours):
    # nothing is added, and nothing new is claimed.
    profile_seen=1
    return
  fi
  profile_new=$profile
}

# plan_hook: for --hook <dir>, where the settings go. Nothing is written yet.
plan_hook() {
  case $hook_dir in
    /*) ;;
    *) hook_dir=$PWD/$hook_dir ;;
  esac
  while :; do
    case $hook_dir in
      ?*/) hook_dir=${hook_dir%/} ;;
      *) break ;;
    esac
  done
  [ -d "$hook_dir" ] || fail usage "--hook needs a project directory that exists; $hook_dir is not one"
  tab=$(printf '\t')
  nl='
'
  case $hook_dir/ in
    */../* | */./* | *"$tab"* | *"$nl"* | */.config/oska/*)
      fail usage "--hook cannot use the directory $hook_dir: it has .., a tab or a newline in it, or is in ~/.config/oska" ;;
  esac
  hook_settings=$hook_dir/.claude/settings.json
  if symlink_above "$hook_settings"; then
    fail install_failed "$hook_dir is reached through the symlink $linked, which --uninstall would refuse"
  fi
  for f in "$hook_dir/.claude" "$hook_settings"; do
    if [ -L "$f" ]; then
      fail install_failed "$f is a symlink, so the installer will not write there"
    fi
  done
}

# fetch_hook: downloads the published post-edit hook settings, and decides
# what to write. They are the check-after-edit entry alone: --hook never
# turns on the Stop gate, which the user chooses separately. The settings
# are written only if the project has none; existing settings need a merge
# unless they already run the post-edit hook. An entry for the old shell
# script, .claude/hooks/oska-check.sh, does not count: the merge replaces it.
fetch_hook() {
  agent_setup=$site/agent-setup
  fetch "$agent_setup/claude-settings-post-edit.json" "$tmp/claude-settings.json" ||
    fail download_failed "cannot download $agent_setup/claude-settings-post-edit.json"
  if ! grep -q '"PostToolUse"' "$tmp/claude-settings.json" || grep -q '"Stop"' "$tmp/claude-settings.json"; then
    fail download_failed "$agent_setup/claude-settings-post-edit.json is not the post-edit hook settings"
  fi
  settings_sum=$(sha256 "$tmp/claude-settings.json")
  settings_write=1
  settings_existed=0
  settings_merge_needed=false
  if [ -e "$hook_settings" ]; then
    settings_write=0
    settings_existed=1
    if [ ! -f "$hook_settings" ]; then
      fail install_failed "$hook_settings is not a regular file; nothing was installed"
    fi
    if ! grep -q 'oskac hook claude --event=post-edit' "$hook_settings"; then
      settings_merge_needed=true
    fi
  fi
}

# write_extras: what --add-to-path and --hook add outside the Oska home,
# after the receipt that records it is in place.
write_extras() {
  if [ -n "$profile_new" ]; then
    if [ -s "$profile_new" ] && [ -n "$(tail -c 1 "$profile_new")" ]; then
      printf '\n' >>"$profile_new" || fail install_failed "cannot write $profile_new"
    fi
    printf '%s\n' "$profile_line" >>"$profile_new" || fail install_failed "cannot write $profile_new"
  fi
  if [ -n "$hook_dir" ] && [ "$settings_write" = 1 ]; then
    mkdir -p "$hook_dir/.claude" || fail install_failed "cannot create $hook_dir/.claude"
    # noclobber: a file that appeared meanwhile is never written over.
    (set -C && cat "$tmp/claude-settings.json" >"$hook_settings") 2>/dev/null ||
      fail install_failed "cannot write $hook_settings"
  fi
}

# semver_cmp <a> <b>: prints -1, 0 or 1 as version <a> sorts below, equal
# to or above <b> by semver precedence, so 0.4.0-pre.1 sorts below 0.4.0
# and above 0.3.0. Both must be versions as is_tag accepts them, without
# the v.
semver_cmp() {
  LC_ALL=C awk -v a="$1" -v b="$2" '
    # Numbers of any length, compared as digit strings.
    function num(x, y) {
      sub(/^0+/, "", x); sub(/^0+/, "", y)
      if (length(x) != length(y)) return length(x) < length(y) ? -1 : 1
      if ((x "") == (y "")) return 0
      return (x "") < (y "") ? -1 : 1
    }
    # Pre-release identifiers: numeric ones by value and below the others,
    # the others in ASCII order.
    function ident(x, y,    xn, yn) {
      xn = x ~ /^[0-9]+$/; yn = y ~ /^[0-9]+$/
      if (xn && yn) return num(x, y)
      if (xn) return -1
      if (yn) return 1
      if ((x "") == (y "")) return 0
      return (x "") < (y "") ? -1 : 1
    }
    BEGIN {
      ap = ""; bp = ""
      if (i = index(a, "-")) { ap = substr(a, i + 1); a = substr(a, 1, i - 1) }
      if (i = index(b, "-")) { bp = substr(b, i + 1); b = substr(b, 1, i - 1) }
      split(a, x, "."); split(b, y, ".")
      for (k = 1; k <= 3; k++) if (c = num(x[k], y[k])) { print c; exit }
      if (ap == "" || bp == "") { print (ap == "") - (bp == ""); exit }
      n = split(ap, p, "."); m = split(bp, q, ".")
      for (k = 1; k <= n && k <= m; k++) if (c = ident(p[k], q[k])) { print c; exit }
      print (n > m) - (n < m)
    }'
}

# json_value <file> <key>: the first value of "<key>" in the JSON file
# <file>, if it is a string or a number. Enough for the unsigned
# latest.json, whose release fields are top-level and whatever it names is
# checked against the signature; the receipt and the index are read with
# json_read.
json_value() {
  tr ',{}[]' '[\n*]' <"$1" |
    sed -n 's/^[[:space:]]*"'"$2"'"[[:space:]]*:[[:space:]]*"\{0,1\}\([^"]*\)"\{0,1\}[[:space:]]*$/\1/p' |
    head -n 1
}

# oska_paths <code>: the Oska home and the paths the installer writes in
# it. Install and uninstall work them out the same way, so a receipt's
# paths match. With neither OSKA_HOME nor HOME set there is no home, and
# it fails with <code>.
oska_paths() {
  if [ -n "${OSKA_HOME:-}" ]; then
    home=$OSKA_HOME
  elif [ -n "${HOME:-}" ]; then
    home=$HOME/.oska
  else
    fail "$1" "neither OSKA_HOME nor HOME is set, so there is no Oska home; set OSKA_HOME"
  fi
  case $home in
    /*) ;;
    *) home=$PWD/$home ;;
  esac
  # Without trailing slashes: `[ -L dir/ ]` looks through a symlinked dir.
  while :; do
    case $home in
      ?*/) home=${home%/} ;;
      *) break ;;
    esac
  done
  bin_dir=$home/bin
  target=$bin_dir/oskac
  version_file=$home/version
  receipt=$home/receipt.json
}

# json_read <mode> <file>: a strict JSON reader for the receipt and the
# release index, rather than a pattern match, so a hand-edited file reads
# as it would anywhere else. It fails, printing nothing, on anything it
# cannot read for certain: malformed JSON, a repeated key, or a key with a
# / in it. A control character or non-ASCII escape in a string reads as
# \001, which no value it is checked for may hold. Modes:
#
#   fields  one line per string or number in an object, under its path from
#           the top: "/latest/tag<TAB>s<string>" or "/serial<TAB>n<number>"
#           (see json_get);
#   files   one line per entry of a receipt's "files",
#           "k<kind><TAB>s<sha256><TAB>p<path><TAB>l<line><TAB>j<project>",
#           with an empty value for a missing or non-string one. It also fails on a schema other than
#           1, no "files" array, a \001 in an entry's value, keys that differ
#           only in case, or a \u escape
#           outside printable ASCII anywhere in the file.
json_read() {
  LC_ALL=C awk -v mode="$1" '
    function die() { exit 1 }
    function ws() {
      while (pos <= n && index(" \t\n\r", substr(s, pos, 1))) pos++
    }
    function peek() { ws(); return substr(s, pos, 1) }
    # A string at pos. Escapes that make a control character, or one
    # outside ASCII, become \001, which no entry may hold.
    function str(   out, c, h, i, v) {
      pos++
      out = ""
      while (1) {
        if (pos > n) die()
        c = substr(s, pos, 1)
        if (c == "\"") { pos++; return out }
        if (index(ctl, c)) die()
        if (c != "\\") { out = out c; pos++; continue }
        c = substr(s, pos + 1, 1)
        pos += 2
        if (c == "\"" || c == "\\" || c == "/") out = out c
        else if (index("bfnrt", c)) out = out "\001"
        else if (c == "u") {
          h = tolower(substr(s, pos, 4))
          if (h !~ /^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]$/) die()
          v = 0
          for (i = 1; i <= 4; i++) v = v * 16 + index("0123456789abcdef", substr(h, i, 1)) - 1
          # A receipt is refused for one anywhere, as install.ps1 and
          # oskac refuse it (docs/guide/install-receipt.md).
          if ((v < 32 || v >= 127) && mode == "files") die()
          out = out (v >= 32 && v < 127 ? sprintf("%c", v) : "\001")
          pos += 4
        } else die()
      }
    }
    # A value at pos, under <at> (its path from the top, like /files/0).
    # Leaves a string in sval and its type in vtype.
    function value(at,   c, key, k, i) {
      c = peek()
      vtype = c
      if (c == "\"") { sval = str(); return }
      if (c == "{") {
        pos++
        if (peek() == "}") { pos++; vtype = "{"; return }
        while (1) {
          if (peek() != "\"") die()
          key = str()
          if (index(key, "/") || peek() != ":") die()
          pos++
          k = at "/" key
          # In a receipt, keys that differ only in case are repeats too,
          # as install.ps1 cannot tell them apart.
          if ((mode == "files" ? tolower(k) : k) in seen) die()
          seen[mode == "files" ? tolower(k) : k] = 1
          value(k)
          if (vtype == "\"") fields = fields k "\ts" sval "\n"
          else if (vtype == "n") fields = fields k "\tn" sval "\n"
          if (at == "" && key == "schema") schema = (vtype == "n" ? sval : "")
          if (at ~ /^\/files\/[0-9]+$/ && vtype == "\"") {
            if (key == "path") e_path = sval
            else if (key == "kind") e_kind = sval
            else if (key == "sha256") e_sha = sval
            else if (key == "line") e_line = sval
            else if (key == "project") e_project = sval
          }
          c = peek()
          pos++
          if (c == "}") break
          if (c != ",") die()
        }
        vtype = "{"
        return
      }
      if (c == "[") {
        pos++
        if (at == "/files") files = 1
        if (peek() == "]") { pos++; vtype = "["; return }
        for (i = 0; ; i++) {
          if (at == "/files") {
            if (peek() != "{") die()
            e_path = ""; e_kind = ""; e_sha = ""; e_line = ""; e_project = ""
          }
          value(at "/" i)
          if (at == "/files") {
            if (index(e_kind e_sha e_path e_line e_project, "\001")) die()
            lines = lines "k" e_kind "\ts" e_sha "\tp" e_path "\tl" e_line "\tj" e_project "\n"
          }
          c = peek()
          pos++
          if (c == "]") break
          if (c != ",") die()
        }
        vtype = "["
        return
      }
      if (match(substr(s, pos), /^-?[0-9]+(\.[0-9]+)?([eE][-+]?[0-9]+)?/)) {
        sval = substr(s, pos, RLENGTH); pos += RLENGTH; vtype = "n"; return
      }
      if (match(substr(s, pos), /^(true|false|null)/)) { pos += RLENGTH; vtype = "l"; return }
      die()
    }
    { s = s $0 "\n" }
    END {
      for (i = 1; i < 32; i++) if (i != 9 && i != 10 && i != 13) ctl = ctl sprintf("%c", i)
      ctl = ctl "\t\n\r"
      n = length(s)
      pos = 1
      if (peek() != "{") die()
      value("")
      ws()
      if (pos <= n) die()
      if (mode == "fields") { printf "%s", fields; exit }
      if (!files || schema != "1") die()
      printf "%s", lines
    }' "$2"
}

# json_get <fields> <path> <type>: the value at <path> in what
# `json_read fields` printed, if it is a string (type s) or a number (n).
json_get() {
  printf '%s\n' "$1" | awk -F '\t' -v p="$2" -v t="$3" \
    '$1 == p && substr($2, 1, 1) == t { print substr($2, 2); exit }'
}



# json_list <lines>: the non-empty lines of <lines> as a JSON array.
json_list() {
  printf '['
  printf '%s\n' "$1" | {
    sep=
    while IFS= read -r line; do
      [ -n "$line" ] || continue
      printf '%s%s' "$sep" "$(json_str "$line")"
      sep=', '
    done
  }
  printf ']'
}

# keep <path> <why>: uninstall leaves <path> in place, and says why.
keep() {
  kept="$kept$1
"
  kept_why="$kept_why  $1 ($2)
"
}

unsafe() {
  fail unsafe_path "$*; nothing was removed"
}

# symlink_above <path>: succeeds, setting $linked, if any directory above
# <path> is a symlink, unless root owns it. Only root can make a symlink
# root owns, so such a one (macOS's /var and /tmp, say) is the system's;
# one anyone else made could steer a deletion out of where it should be.
# `find -user` looks at the link itself, as find does not follow links.
symlink_above() {
  above=$1
  while :; do
    above=${above%/*}
    # Without trailing slashes, through which `[ -L ]` would look.
    while :; do
      case $above in
        ?*/) above=${above%/} ;;
        *) break ;;
      esac
    done
    [ -n "$above" ] || return 1
    if [ -L "$above" ] && [ -z "$(find "$above" -prune -user 0 2>/dev/null)" ]; then
      linked=$above
      return 0
    fi
  done
}

# no_symlink_above <path>: refuses the uninstall if symlink_above does.
no_symlink_above() {
  if symlink_above "$1"; then
    unsafe "$1 is reached through the symlink $linked"
  fi
}

# --uninstall: remove what the receipt lists, and nothing else. Every entry
# is checked before anything is removed, and one that is not a file this
# installer writes, in a place it writes, refuses the whole uninstall.
# A file that changed since it was installed is kept. Directories are
# removed only if empty, never recursively, and ~/.config/oska is never
# touched. Lists are one path a line: the receipt reader refuses a path
# with a control character in it.
uninstall() {
  oska_paths not_installed
  # ~/.config/oska, if there is a HOME to find it by. Any .config/oska
  # directory is refused below, so it is refused without one too.
  config_dir=
  if [ -n "${HOME:-}" ]; then
    config_dir=${HOME%/}/.config/oska
  fi
  if [ -L "$home" ] || [ -L "$receipt" ]; then
    unsafe "$receipt is reached through a symlink"
  fi
  no_symlink_above "$home"
  [ -f "$receipt" ] ||
    fail not_installed "there is no install receipt at $receipt, so nothing is known to have been installed there; nothing was removed"
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    need openssl "or sha256sum or shasum, to check that a file is unchanged before removing it"
  fi
  entries=$(json_read files "$receipt") ||
    unsafe "cannot read the install receipt $receipt, so none of its paths is known to be safe"

  tab=$(printf '\t')
  # The paths acted on so far, one a line. A path listed again is checked
  # like any entry, and acted on, and reported, once. bin/ is handled with
  # the home, after the files.
  acted="
$bin_dir
"
  remove=
  remove_receipt=0
  kept=
  kept_why=
  unline=
  profile_hint=
  while IFS="$tab" read -r kind sum path line project; do
    [ -n "$kind" ] || continue
    # Each field carries a one-letter prefix, so none is empty and the
    # tab separators never merge.
    kind=${kind#k}
    sum=${sum#s}
    path=${path#p}
    line=${line#l}
    project=${project#j}
    case $path in
      /*) ;;
      *) unsafe "the receipt lists '$path', which is not an absolute path" ;;
    esac
    case $path/ in
      */../*) unsafe "the receipt lists $path, which climbs out with .." ;;
    esac
    case $path/ in
      */.config/oska/*) unsafe "the receipt lists $path, in a .config/oska directory of settings and credentials, which uninstall never touches" ;;
    esac
    case $kind in
      binary | record)
        case $path in
          "$home"/*) name=${path#"$home"/} ;;
          *) unsafe "the receipt lists $path, which is outside the Oska home $home" ;;
        esac
        dir=$home
        case $name in
          bin/*)
            name=${name#bin/}
            dir=$bin_dir ;;
        esac
        case $name in
          '' | . | */*) unsafe "the receipt lists $path, which is not directly in $home or $bin_dir" ;;
        esac ;;
      profile_line)
        # Only the profile files --add-to-path picks from, directly in the
        # user's home, and only the line it writes: marked, so that a
        # receipt cannot name some other line of the profile.
        [ -n "${HOME:-}" ] ||
          unsafe "the receipt lists the profile $path, and there is no HOME to check it against"
        case $path in
          "${HOME%/}/.zshrc" | "${HOME%/}/.bashrc" | "${HOME%/}/.bash_profile" | "${HOME%/}/.profile") ;;
          *) unsafe "the receipt lists the profile $path, which is not a shell profile directly in $HOME" ;;
        esac
        case $line in
          "export PATH="*"$PROFILE_MARK") ;;
          *) unsafe "the receipt lists a line of $path that is not the marked line the installer adds" ;;
        esac
        dir=${path%/*}
        no_symlink_above "$path" ;;
      hook_script | hook_settings)
        # Under the project recorded with it, at the place --hook writes.
        case $project in
          /*) ;;
          *) unsafe "the receipt lists $path with the project '$project', which is not an absolute path" ;;
        esac
        case $project/ in
          */../* | */./*) unsafe "the receipt lists the project $project, which climbs out with .." ;;
        esac
        case $kind in
          hook_script) [ "$path" = "${project%/}/.claude/hooks/oska-check.sh" ] ;;
          *) [ "$path" = "${project%/}/.claude/settings.json" ] ;;
        esac ||
          unsafe "the receipt lists $path, which is not where --hook writes it in the project $project"
        dir=${path%/*}
        no_symlink_above "$path" ;;
      *) unsafe "the receipt lists $path as a '$kind', which this installer does not remove" ;;
    esac
    if [ -L "$dir" ] || [ -L "$path" ]; then
      unsafe "the receipt lists $path, which is reached through a symlink"
    fi
    case $acted in
      *"
$path
"*) continue ;;
    esac
    acted="$acted$path
"

    if [ "$kind" = profile_line ]; then
      if [ ! -e "$path" ]; then
        continue
      elif [ ! -f "$path" ]; then
        keep "$path" "not a regular file"
      elif [ "$line" != "export PATH=\"$bin_dir:\$PATH\" $PROFILE_MARK" ]; then
        # Marked, but not the line --add-to-path writes for this home.
        keep "$path" "the receipt's line is not the one the installer adds for $bin_dir"
        profile_hint="remove the line the installer added, if it is still there, from $path: $line"
      elif grep -Fx -- "$line" "$path" >/dev/null 2>&1; then
        unline="$unline$path$tab$line
"
      else
        keep "$path" "the line the installer added is no longer there as it was written"
        profile_hint="remove the line the installer added, if it is still there, from $path: $line"
      fi
      continue
    fi
    if [ ! -e "$path" ]; then
      continue
    elif [ ! -f "$path" ]; then
      keep "$path" "not a regular file"
    elif [ "$path" = "$receipt" ]; then
      remove_receipt=1
    elif [ -z "$sum" ] && [ "$kind" = hook_settings ]; then
      keep "$path" "settings the installer did not create; remove its oskac hook entries by hand"
    elif [ -z "$sum" ]; then
      keep "$path" "the receipt records no SHA-256 for it"
    elif [ "$(sha256 "$path")" != "$sum" ]; then
      keep "$path" "changed since it was installed"
    else
      remove="$remove$path
"
    fi
  done <<EOF
$entries
EOF

  # Everything is checked; now remove. The receipt goes last, and stays
  # if a file could not be removed, so a second run can finish the job.
  removed=
  failed=0
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    if rm -f "$path" 2>/dev/null; then
      removed="$removed$path
"
    else
      failed=1
      keep "$path" "could not be removed"
    fi
  done <<EOF
$remove
EOF
  # A profile line goes by rewriting the file without the lines that equal
  # it, which keeps the file's own mode and owner.
  while IFS="$tab" read -r path line; do
    [ -n "$path" ] || continue
    if edited=$(L=$line awk 'BEGIN { l = ENVIRON["L"] } $0 != l { print }' "$path" && echo x) &&
      printf '%s' "${edited%x}" >"$path" 2>/dev/null; then
      removed="$removed$path (the PATH line the installer added)
"
    else
      failed=1
      keep "$path" "could not be edited"
    fi
  done <<EOF
$unline
EOF
  if [ "$remove_receipt" = 1 ]; then
    if [ "$failed" = 0 ] && rm -f "$receipt" 2>/dev/null; then
      removed="$removed$receipt
"
    else
      keep "$receipt" "kept while other files remain"
    fi
  fi
  for dir in "$bin_dir" "$home"; do
    [ -d "$dir" ] && [ ! -L "$dir" ] || continue
    if rmdir "$dir" 2>/dev/null; then
      removed="$removed$dir
"
    else
      keep "$dir" "not empty"
    fi
  done
  if [ -n "$config_dir" ] && { [ -e "$config_dir" ] || [ -L "$config_dir" ]; }; then
    keep "$config_dir" "settings and credentials, which uninstall never touches"
  fi

  path_hint=$profile_hint
  if [ -z "$path_hint" ] && [ -z "$unline" ]; then
    case ":${PATH:-}:" in
      *":$bin_dir:"*) path_hint="$bin_dir is on your PATH; remove it from the shell profile that adds it" ;;
    esac
  fi

  if [ "$json" = 1 ]; then
    if [ -n "$path_hint" ]; then
      hint_json=$(json_str "$path_hint")
    else
      hint_json=null
    fi
    printf '{"ok": true, "action": "uninstalled", "removed": %s, "kept": %s, "path_hint": %s}\n' \
      "$(json_list "$removed")" "$(json_list "$kept")" "$hint_json"
    return
  fi
  echo "Uninstalled oskac from $home."
  if [ -n "$removed" ]; then
    echo "Removed:"
    printf '%s' "$removed" | sed 's/^/  /'
  fi
  if [ -n "$kept" ]; then
    echo "Kept:"
    printf '%s' "$kept_why"
  fi
  if [ -n "$path_hint" ]; then
    echo "$path_hint."
  fi
}

# Whether <serial> is a serial: a positive integer of at most 15 digits.
# 999999999999999 is the largest, as verify-index.sh and sign-index.sh
# agree, and small enough for shell arithmetic.
is_serial() {
  case $1 in
    '' | *[!0-9]* | 0* | ????????????????*) return 1 ;;
  esac
}

# The receipt's fields (json_read fields), or nothing if there is no
# receipt or it cannot be read.
receipt_fields() {
  if [ -f "$receipt" ]; then
    json_read fields "$receipt" || true
  fi
}

# The version installed now, or nothing. The receipt wins over the legacy
# version file, but only while it describes the installed binary: its
# SHA-256 must be the binary's. One that does not (oskac was replaced by
# hand, or an install was killed as it committed, see commit_install) is
# not believed, and neither is one that cannot be read. An install from
# before receipts has only the version file. Either is advisory: what is
# not a version counts as nothing installed.
previous_version() {
  [ -f "$target" ] || return 0
  fields=$(receipt_fields)
  v=$(json_get "$fields" /version s)
  recorded=$(json_get "$fields" /sha256 s)
  if [ -z "$recorded" ] || [ "$(sha256 "$target" 2>/dev/null)" != "$recorded" ]; then
    v=
    if [ -f "$version_file" ]; then
      v=$(head -n 1 "$version_file" | tr -d '\r')
    fi
  fi
  if is_tag "v$v"; then
    printf '%s\n' "$v"
  fi
}

# The highest index serial this install accepted, from the receipt, or 0.
# Advisory like the rest of the receipt: it can only make the rollback
# check below stricter or looser, never let an unsigned index through.
serial_seen() {
  s=$(json_get "$(receipt_fields)" /index_serial n)
  if is_serial "$s"; then
    echo "$s"
  else
    echo 0
  fi
}

# epoch_of <YYYY-MM-DDTHH:MM:SSZ>: seconds since the epoch. Computed here,
# as GNU and BSD date parse dates differently.
epoch_of() {
  printf '%s\n' "$1" | LC_ALL=C awk -F '[-T:Z]' '{
    y = $1 + 0; m = $2 + 0
    if (m <= 2) { y--; m += 12 }
    days = 365 * y + int(y / 4) - int(y / 100) + int(y / 400) + int((153 * (m - 3) + 2) / 5) + $3 - 719469
    printf "%d\n", days * 86400 + $4 * 3600 + $5 * 60 + $6
  }'
}

# Fetches and checks the signed release index, and sets tag, index_serial
# and index_expires from it. Refuses an index that is not signed by the
# release key in the `oska-index` namespace or is not in the index format
# (bad_index), one that has expired (stale_index), and one whose serial is
# lower than the receipt's (rollback_refused). Warns on stderr when fewer
# than 30 days remain.
read_index() {
  index_url=$site/release-index.json
  fetch "$index_url" "$tmp/release-index.json" ||
    fail download_failed "cannot download $index_url"
  fetch "$index_url.sig" "$tmp/release-index.json.sig" ||
    fail bad_index "the release index at $index_url has no signature ($index_url.sig); nothing was installed"
  printf '%s namespaces="%s" %s\n' "$INDEX_NAMESPACE" "$INDEX_NAMESPACE" "$RELEASE_KEY" >"$tmp/allowed_index"
  ssh-keygen -Y verify -f "$tmp/allowed_index" -I "$INDEX_NAMESPACE" -n "$INDEX_NAMESPACE" \
    -s "$tmp/release-index.json.sig" <"$tmp/release-index.json" >/dev/null 2>&1 ||
    fail bad_index "the release index at $index_url is not signed by the Oska release key as an index (verifying needs OpenSSH 8.1 or newer); nothing was installed"

  # Read as strictly as the receipt; a file that cannot be read has no
  # fields, so it fails the format check.
  fields=$(json_read fields "$tmp/release-index.json") || fields=
  index_serial=$(json_get "$fields" /serial n)
  index_expires=$(json_get "$fields" /expires s)
  tag=$(json_get "$fields" /latest/tag s)
  if ! is_serial "$index_serial" || [ "$(json_get "$fields" /schema n)" != 1 ] ||
    ! printf '%s\n' "$index_expires" | grep -Eqx '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z' ||
    ! printf '%s\n' "$tag" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+' ||
    [ "$(json_get "$fields" /latest/version s)" != "${tag#v}" ]; then
    fail bad_index "the release index at $index_url is not in the release index format; nothing was installed"
  fi

  left=$(($(epoch_of "$index_expires") - $(date -u +%s)))
  [ "$left" -gt 0 ] ||
    fail stale_index "the release index at $index_url expired at $index_expires, so the latest release cannot be trusted; nothing was installed (a named release still installs with --version)"
  seen=$(serial_seen)
  [ "$index_serial" -ge "$seen" ] ||
    fail rollback_refused "the release index at $index_url has serial $index_serial, lower than serial $seen, which this install already accepted; nothing was installed"
  if [ "$left" -lt $((30 * 86400)) ]; then
    echo "install.sh: warning: the release index expires at $index_expires, in fewer than 30 days; after that, installing the latest release fails until it is re-signed" >&2
  fi
}

# --check: report the installed and the latest version, and write nothing.
report_check() {
  if [ -z "$previous" ] || [ "$(semver_cmp "$version" "$previous")" -gt 0 ]; then
    update_available=true
  else
    update_available=false
  fi
  if [ "$json" = 1 ]; then
    if [ -n "$previous" ]; then installed_json=$(json_str "$previous"); else installed_json=null; fi
    if [ "$index_verified" = true ]; then
      serial_json=$index_serial
      expires_json=$(json_str "$index_expires")
    else
      serial_json=null
      expires_json=null
    fi
    printf '{"ok": true, "action": "checked", "installed": %s, "latest": %s, "tag": %s, "update_available": %s, "index_verified": %s, "index_serial": %s, "index_expires": %s}\n' \
      "$installed_json" "$(json_str "$version")" "$(json_str "$tag")" "$update_available" \
      "$index_verified" "$serial_json" "$expires_json"
    return
  fi
  if [ -z "$previous" ]; then
    echo "oskac is not installed in $home. The latest release is $version."
  elif [ "$update_available" = true ]; then
    echo "oskac $previous is installed; the latest release is $version. To update, run the installer again."
  elif [ "$previous" = "$version" ]; then
    echo "oskac $previous is installed, and it is the latest release."
  else
    echo "oskac $previous is installed, which is newer than the latest release, $version."
  fi
  if [ "$index_verified" = true ]; then
    echo "The latest release comes from the signed release index (serial $index_serial, expires $index_expires)."
  else
    echo "The latest release comes from $site/latest.json, which is not signed."
  fi
}

main() {
  requested=
  allow_downgrade=0
  mode=
  while [ $# -gt 0 ]; do
    case $1 in
      --json) json=1 ;;
      --version)
        [ $# -ge 2 ] || fail usage "--version needs a value, like --version 0.1.0"
        requested=$2
        shift ;;
      --version=*) requested=${1#--version=} ;;
      --allow-downgrade) allow_downgrade=1 ;;
      --add-to-path) add_to_path=1 ;;
      --hook)
        [ $# -ge 2 ] && [ -n "$2" ] || fail usage "--hook needs a project directory, like --hook ."
        hook_dir=$2
        shift ;;
      --hook=*)
        hook_dir=${1#--hook=}
        [ -n "$hook_dir" ] || fail usage "--hook needs a project directory, like --hook ." ;;
      --check | --uninstall)
        [ -z "$mode" ] || [ "$mode" = "$1" ] ||
          fail usage "$mode and $1 cannot be used together; pick one"
        mode=$1 ;;
      -h | --help)
        usage
        exit 0 ;;
      *) fail usage "unknown option $1 (try --help)" ;;
    esac
    shift
  done
  # In every mode, so a bad value is refused wherever it is set.
  check_libc_choice
  # Install is the default mode; --check and --uninstall are the others.
  # --version and --allow-downgrade belong to install alone.
  if [ -n "$mode" ]; then
    [ -z "$requested" ] || fail usage "--version cannot be used with $mode"
    [ "$allow_downgrade" = 0 ] || fail usage "--allow-downgrade cannot be used with $mode"
    [ "$add_to_path" = 0 ] || fail usage "--add-to-path cannot be used with $mode"
    [ -z "$hook_dir" ] || fail usage "--hook cannot be used with $mode"
    if [ "$mode" = --uninstall ]; then
      uninstall
      return
    fi
  fi
  [ "$allow_downgrade" = 0 ] || [ -n "$requested" ] ||
    fail usage "--allow-downgrade needs --version naming the release to go back to"

  case $RELEASE_KEY in
    ssh-ed25519\ *) ;;
    *) fail no_signing_key "this installer has no pinned release key; download it from $DEFAULT_SITE_URL/install.sh" ;;
  esac
  case $INDEX_REQUIRED in
    yes | no) ;;
    *) fail no_signing_key "this installer was not pinned to say whether the release index is required; download it from $DEFAULT_SITE_URL/install.sh" ;;
  esac

  site=${OSKA_SITE_URL:-$DEFAULT_SITE_URL}
  site=${site%/}
  case $site in
    https://* | file://*) ;;
    *) fail insecure_url "refusing to download from $site: only https:// URLs are allowed" ;;
  esac

  if [ -n "$requested" ]; then
    case $requested in
      v*) tag=$requested ;;
      *) tag=v$requested ;;
    esac
    is_tag "$tag" || fail usage "'$requested' is not a version like 0.1.0"
  fi

  # --check only reads, so it needs no platform build or checksum tool.
  if [ "$mode" != --check ]; then
    detect_platform
  fi
  need curl
  need ssh-keygen "OpenSSH 8.1 or newer, to verify the release signature"
  if [ "$mode" != --check ] && ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    need openssl "or sha256sum or shasum, to check the SHA-256 checksum"
  fi

  oska_paths install_failed
  previous=$(previous_version)

  # An explicit template, so the directory is under $TMPDIR (or /tmp) on
  # every mktemp; BSD and GNU differ on where a bare `mktemp -d` goes.
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/oska-install.XXXXXXXX") ||
    fail install_failed "cannot create a temporary directory"
  if [ "$mode" != --check ]; then
    if [ "$add_to_path" = 1 ]; then
      plan_profile
    fi
    if [ -n "$hook_dir" ]; then
      plan_hook
    fi
  fi

  # The latest release, unless --version names one: from the signed index
  # when this installer is pinned to require it, else from latest.json.
  # With --version the index is not consulted, and the receipt keeps the
  # serial it had.
  index_verified=false
  index_serial=
  index_expires=
  receipt_serial=$(serial_seen)
  if [ -z "$requested" ]; then
    if [ "$INDEX_REQUIRED" = yes ]; then
      read_index
      index_verified=true
      receipt_serial=$index_serial
    else
      fetch "$site/latest.json" "$tmp/latest.json" ||
        fail download_failed "cannot download $site/latest.json"
      # The release fields of latest.json are top-level, so the first "tag"
      # is the release's. Whatever it names is checked against the
      # signature.
      tag=$(json_value "$tmp/latest.json" tag)
      is_tag "$tag" || fail download_failed "$site/latest.json does not name a release"
    fi
  fi
  version=${tag#v}

  if [ "$mode" = --check ]; then
    report_check
    return
  fi

  # The downgrade guard. An older release is installed only when it is
  # named with --version and --allow-downgrade, so a stale latest.json, an
  # old script or a re-run never moves an install back.
  if [ -n "$previous" ] && [ "$(semver_cmp "$version" "$previous")" -lt 0 ]; then
    if [ -z "$requested" ]; then
      fail downgrade_refused "oskac $previous is installed, which is newer than the latest release, $version; nothing was installed (to go back, pass --version $version --allow-downgrade)"
    elif [ "$allow_downgrade" = 0 ]; then
      fail downgrade_refused "oskac $previous is installed, which is newer than $version; nothing was installed (to downgrade, add --allow-downgrade)"
    fi
  fi

  release=$site/releases/$tag
  for f in SHA256SUMS SHA256SUMS.sig; do
    fetch "$release/$f" "$tmp/$f" ||
      fail download_failed "cannot download $release/$f (is $version a release?)"
  done

  printf '%s namespaces="%s" %s\n' "$NAMESPACE" "$NAMESPACE" "$RELEASE_KEY" >"$tmp/allowed_signers"
  ssh-keygen -Y verify -f "$tmp/allowed_signers" -I "$NAMESPACE" -n "$NAMESPACE" \
    -s "$tmp/SHA256SUMS.sig" <"$tmp/SHA256SUMS" >/dev/null 2>&1 ||
    fail bad_signature "SHA256SUMS for $tag is not signed by the Oska release key (verifying needs OpenSSH 8.1 or newer); nothing was installed"

  # The signed first line names the release. A genuine old release served
  # under a newer tag has a valid signature, but names its own tag here.
  signed=$(head -n 1 "$tmp/SHA256SUMS" | tr -d '\r')
  case $signed in
    "# oska-release $tag") ;;
    "# oska-release "*)
      fail version_mismatch "the files at $release/ are signed as release ${signed#"# oska-release "}, not $tag; nothing was installed" ;;
    *)
      fail version_mismatch "SHA256SUMS at $release/ does not name the release it is for; nothing was installed" ;;
  esac

  # Which file to download, from the signed SHA256SUMS. On Linux a release
  # has a glibc build (-gnu) and a musl build (-musl) (ADR 0022), and
  # choose_linux_build has picked one. A release from before them (v0.5.0
  # and earlier) has one Linux file, oskac-<platform>, the glibc build: it
  # is installed only where the glibc build was chosen, and refused, as
  # those releases always were, where only the musl build would run.
  asset=oskac-$platform
  build_note=
  if [ "$os" = linux ]; then
    if sums_list "$asset-gnu" || sums_list "$asset-musl"; then
      asset=$asset-$linux_build
    elif [ "$linux_build" != gnu ]; then
      if [ "$libc_forced" = true ]; then
        fail unsupported_libc "OSKA_LIBC=musl asks for the musl build, and oskac $version has no musl build (releases before the musl build have only the glibc build, which needs glibc $MIN_GLIBC or newer); install a later release, or build oskac from source"
      fi
      fail unsupported_libc "oskac $version has only the glibc build, which needs glibc $MIN_GLIBC or newer, and this system does not have it; install a later release, which has a musl build, or build oskac from source"
    fi
    build_note=" ($linux_build build)"
  fi
  say "Downloading oskac $version for $platform$build_note from $release/" >&2
  fetch "$release/$asset" "$tmp/$asset" ||
    fail download_failed "cannot download $release/$asset (is $version a release?)"

  expected=$(awk -v f="$asset" 'NF == 2 && $2 == f {print $1}' "$tmp/SHA256SUMS")
  printf '%s\n' "$expected" | grep -Eqx '[0-9a-f]{64}' ||
    fail checksum_mismatch "SHA256SUMS for $tag does not list $asset exactly once; nothing was installed"
  actual=$(sha256 "$tmp/$asset")
  [ "$actual" = "$expected" ] ||
    fail checksum_mismatch "$asset does not match its SHA-256 in SHA256SUMS; nothing was installed"

  printf '%s\n' "$RELEASE_KEY" >"$tmp/release.pub"
  fingerprint=$(ssh-keygen -l -f "$tmp/release.pub" 2>/dev/null | awk '{print $2}')
  case $fingerprint in
    SHA256:*) ;;
    *) fail install_failed "ssh-keygen cannot fingerprint the release key" ;;
  esac

  if [ -n "$hook_dir" ]; then
    fetch_hook
  fi
  mkdir -p "$bin_dir" || fail install_failed "cannot create $bin_dir"
  # Stage in a new directory only this user can write to, made next to the
  # target so the final move is an atomic rename. mktemp picks a name no one
  # can predict and refuses one that exists, so nothing planted in $bin_dir
  # (a symlink, say) is ever written through.
  stage=$(mktemp -d "$bin_dir/.oskac.XXXXXXXX") ||
    fail install_failed "cannot create a staging directory in $bin_dir"
  staged=$stage/oskac
  { cp "$tmp/$asset" "$staged" && chmod 755 "$staged"; } ||
    fail install_failed "cannot write $staged"
  reported=$("$staged" --version 2>/dev/null | head -n 1 | tr -d '\r') || reported=
  [ "$reported" = "oskac $version" ] ||
    fail version_mismatch "the $tag binary reports '$reported', not 'oskac $version'; nothing was installed"
  prepare_records
  commit_install
  cleanup

  if [ -z "$previous" ]; then
    action=installed
  else
    case $(semver_cmp "$version" "$previous") in
      1) action=updated ;;
      0) action=reinstalled ;;
      *) action=downgraded ;;
    esac
  fi
  case ":${PATH:-}:" in
    *":$bin_dir:"*) on_path=true ;;
    *) on_path=false ;;
  esac

  # What --add-to-path and --hook did, as extra JSON fields.
  extra_json=
  if [ -n "$profile_new" ]; then
    extra_json=", \"profile\": $(json_str "$profile_new")"
  fi
  if [ -n "$hook_dir" ]; then
    if [ "$settings_write" = 1 ]; then
      extra_json="$extra_json, \"hook_settings\": $(json_str "$hook_settings")"
    else
      extra_json="$extra_json, \"hook_settings\": null"
    fi
    extra_json="$extra_json, \"settings_merge_needed\": $settings_merge_needed"
  fi

  if [ "$json" = 1 ]; then
    if [ -n "$previous" ]; then
      previous_json=$(json_str "$previous")
    else
      previous_json=null
    fi
    printf '{"ok": true, "action": %s, "version": %s, "tag": %s, "previous_version": %s, "platform": %s, "path": %s, "bin_dir": %s, "on_path": %s, "checksum_verified": true, "signature_verified": true, "receipt": %s, "index_verified": %s, "index_serial": %s%s}\n' \
      "$(json_str "$action")" "$(json_str "$version")" "$(json_str "$tag")" "$previous_json" \
      "$(json_str "$platform")" "$(json_str "$target")" "$(json_str "$bin_dir")" "$on_path" \
      "$(json_str "$receipt")" "$index_verified" "${index_serial:-null}" "$extra_json"
    return
  fi

  case $action in
    installed) echo "Installed oskac $version to $target." ;;
    reinstalled) echo "Reinstalled oskac $version to $target." ;;
    updated) echo "Updated oskac $previous to $version in $target." ;;
    downgraded) echo "Downgraded oskac $previous to $version in $target." ;;
  esac
  echo "The checksum and the release signature were verified."
  if [ -n "$profile_new" ]; then
    echo
    echo "Added $bin_dir to your PATH in $profile_new; open a new shell to use it."
    echo "  $profile_line"
  elif [ "$add_to_path" = 1 ] && [ "$profile_seen" = 1 ]; then
    echo
    echo "$profile already puts $bin_dir on your PATH; open a new shell to use it."
  fi
  if [ -n "$hook_dir" ]; then
    echo
    if [ "$settings_write" = 1 ]; then
      echo "Created the project's Claude Code settings, $hook_settings,"
      echo "with the check-after-edit hook."
    elif [ "$settings_merge_needed" = true ]; then
      echo "$hook_settings already exists, so it was left as it is: merge the PostToolUse"
      echo "entry of $site/agent-setup/claude-settings-post-edit.json into it to turn the"
      echo "hook on, in place of any entry that runs .claude/hooks/oska-check.sh."
    else
      echo "$hook_settings already runs the check-after-edit hook."
    fi
  fi
  if [ "$on_path" = false ] && [ "$add_to_path" = 0 ]; then
    echo
    echo "$bin_dir is not on your PATH. Add it, for example in ~/.profile:"
    echo
    echo "  export PATH=\"$bin_dir:\$PATH\""
  fi
}

# Remove the temp and staging directories however the run ends. On a
# signal, clean up, then die of that same signal (on_signal).
trap cleanup EXIT
for sig in HUP INT TERM; do
  # shellcheck disable=SC2064 # $sig is expanded now
  trap "on_signal $sig" "$sig"
done

main "$@"
