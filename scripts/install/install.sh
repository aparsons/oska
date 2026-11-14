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
#   --json             print one JSON object on stdout, and nothing else
#   --help             print this help
#
# Environment:
#   OSKA_HOME      install into $OSKA_HOME/bin (default: ~/.oska)
#   OSKA_SITE_URL  where to download from (default: https://oskalang.dev);
#                  only https:// and file:// URLs are accepted
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

set -u

# The release signing key, pinned when the site is built
# (scripts/install/pin.sh). Signatures by any other key are refused.
RELEASE_KEY='@OSKA_RELEASE_KEY@'
NAMESPACE=oska-release
# Whether the latest release must come from the signed release index, also
# pinned when the site is built: `yes` once the site publishes a verified
# index. Otherwise the installer reads the unsigned latest.json.
INDEX_REQUIRED='@OSKA_INDEX_REQUIRED@'
INDEX_NAMESPACE=oska-index
DEFAULT_SITE_URL=https://oskalang.dev
# The glibc of the Linux build runners (.github/workflows/release.yml).
MIN_GLIBC=2.35

json=0
tmp=
stage=
receipt_tmp=

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
  if [ -n "$receipt_tmp" ]; then
    rm -f "$receipt_tmp"
  fi
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
  --json             print one JSON object on stdout, and nothing else
  --help             print this help

Environment: OSKA_HOME (default ~/.oska), OSKA_SITE_URL (https:// only).
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
    check_glibc
  fi
}

# The Linux builds link against glibc $MIN_GLIBC (they are built on Ubuntu
# 22.04) and do not run on an older glibc or on musl. Refuse those up front
# rather than install a binary that cannot start.
check_glibc() {
  libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || libc=
  case $libc in
    glibc\ *) libc=${libc#glibc } ;;
    *)
      # No getconf: glibc's ldd names itself on its first line.
      libc=$(ldd --version 2>&1 | head -n 1) || libc=
      case $libc in
        *GLIBC* | *"GNU libc"*) libc=${libc##* } ;;
        *musl*) fail unsupported_libc "oskac for Linux needs glibc $MIN_GLIBC or newer; this system uses musl (Alpine, for example), so build oskac from source" ;;
        *) fail unsupported_libc "oskac for Linux needs glibc $MIN_GLIBC or newer, and this system's C library is not glibc; build oskac from source" ;;
      esac ;;
  esac
  major=${libc%%.*}
  minor=${libc#*.}
  minor=${minor%%[!0-9]*}
  case $major$minor in
    '' | *[!0-9]*) fail unsupported_libc "cannot tell which glibc this is ('$libc'); oskac for Linux needs glibc $MIN_GLIBC or newer" ;;
  esac
  if [ "$major" -lt "${MIN_GLIBC%%.*}" ] ||
    { [ "$major" -eq "${MIN_GLIBC%%.*}" ] && [ "$minor" -lt "${MIN_GLIBC#*.}" ]; }; then
    fail unsupported_libc "oskac for Linux needs glibc $MIN_GLIBC or newer; this system has glibc $libc, so build oskac from source"
  fi
}

# The install receipt: what this run installed, as JSON in the Oska home.
# It is advisory. Verification never reads it; it only records what counts
# as installed, what an uninstall may remove and the highest index serial
# seen. Written to a new file beside it, then renamed into place.
write_receipt() {
  receipt_tmp=$(mktemp "$home/.receipt.XXXXXXXX") ||
    fail install_failed "cannot write a receipt in $home"
  {
    printf '{\n'
    printf '  "schema": 1,\n'
    printf '  "version": %s,\n' "$(json_str "$version")"
    printf '  "tag": %s,\n' "$(json_str "$tag")"
    printf '  "platform": %s,\n' "$(json_str "$platform")"
    printf '  "installed_at": %s,\n' "$(json_str "$(date -u +%Y-%m-%dT%H:%M:%SZ)")"
    printf '  "site": %s,\n' "$(json_str "$site")"
    printf '  "key_fingerprint": %s,\n' "$(json_str "$fingerprint")"
    printf '  "sha256": %s,\n' "$(json_str "$actual")"
    printf '  "installer": "sh",\n'
    printf '  "index_serial": %s,\n' "$receipt_serial"
    printf '  "files": [\n'
    printf '    {"path": %s, "kind": "binary", "sha256": %s},\n' "$(json_str "$target")" "$(json_str "$actual")"
    printf '    {"path": %s, "kind": "record", "sha256": %s},\n' "$(json_str "$version_file")" "$(json_str "$(sha256 "$version_file")")"
    printf '    {"path": %s, "kind": "record"}\n' "$(json_str "$receipt")"
    printf '  ]\n'
    printf '}\n'
  } >"$receipt_tmp" || fail install_failed "cannot write $receipt_tmp"
  mv -f "$receipt_tmp" "$receipt" || fail install_failed "cannot write $receipt"
  receipt_tmp=
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
# <file>, if it is a string or a number. Enough for latest.json and the
# receipt, whose fields of interest come before any nested object that
# could hold the same key; callers validate what it returns.
json_value() {
  tr ',{}[]' '[\n*]' <"$1" |
    sed -n 's/^[[:space:]]*"'"$2"'"[[:space:]]*:[[:space:]]*"\{0,1\}\([^"]*\)"\{0,1\}[[:space:]]*$/\1/p' |
    head -n 1
}

# The Oska home and the paths the installer writes in it. Install and
# uninstall work them out the same way, so a receipt's paths match.
oska_paths() {
  home=${OSKA_HOME:-$HOME/.oska}
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

# receipt_files <receipt>: one line per entry of the receipt's "files",
# "k<kind><TAB>s<sha256><TAB>p<path>", with an empty value for a missing
# or non-string one. A strict JSON reader rather than a pattern match, so a
# hand-edited receipt reads as it would anywhere else. It fails, printing
# nothing, on anything it cannot read for certain: malformed JSON, a schema
# other than 1, no "files" array, a repeated key, or a control character
# or non-ASCII escape in an entry's value.
receipt_files() {
  LC_ALL=C awk '
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
          if (peek() != ":") die()
          pos++
          k = at "/" key
          if (k in seen) die()
          seen[k] = 1
          value(k)
          if (at == "" && key == "schema") schema = (vtype == "n" ? sval : "")
          if (at ~ /^\/files\/[0-9]+$/ && vtype == "\"") {
            if (key == "path") e_path = sval
            else if (key == "kind") e_kind = sval
            else if (key == "sha256") e_sha = sval
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
            e_path = ""; e_kind = ""; e_sha = ""
          }
          value(at "/" i)
          if (at == "/files") {
            if (index(e_kind e_sha e_path, "\001")) die()
            lines = lines "k" e_kind "\ts" e_sha "\tp" e_path "\n"
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
      if (pos <= n || !files || schema != "1") die()
      printf "%s", lines
    }' "$1"
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

# --uninstall: remove what the receipt lists, and nothing else. Every entry
# is checked before anything is removed, and one that is not a file this
# installer writes, in a place it writes, refuses the whole uninstall.
# A file that changed since it was installed is kept. Directories are
# removed only if empty, never recursively, and ~/.config/oska is never
# touched. Lists are one path a line: the receipt reader refuses a path
# with a control character in it.
uninstall() {
  oska_paths
  config_dir=${HOME%/}/.config/oska
  if [ -L "$home" ] || [ -L "$receipt" ]; then
    unsafe "$receipt is reached through a symlink"
  fi
  [ -f "$receipt" ] ||
    fail not_installed "there is no install receipt at $receipt, so nothing is known to have been installed there; nothing was removed"
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    need openssl "or sha256sum or shasum, to check that a file is unchanged before removing it"
  fi
  entries=$(receipt_files "$receipt") ||
    unsafe "cannot read the install receipt $receipt, so none of its paths is known to be safe"

  tab=$(printf '\t')
  remove=
  remove_receipt=0
  kept=
  kept_why=
  while IFS="$tab" read -r kind sum path; do
    [ -n "$kind" ] || continue
    # Each field carries a one-letter prefix, so none is empty and the
    # tab separators never merge.
    kind=${kind#k}
    sum=${sum#s}
    path=${path#p}
    case $path in
      /*) ;;
      *) unsafe "the receipt lists '$path', which is not an absolute path" ;;
    esac
    case $path/ in
      */../*) unsafe "the receipt lists $path, which climbs out with .." ;;
    esac
    case $path/ in
      "$config_dir"/*) unsafe "the receipt lists $path, in $config_dir, which uninstall never touches" ;;
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
      *) unsafe "the receipt lists $path as a '$kind', which this installer does not remove" ;;
    esac
    if [ -L "$dir" ] || [ -L "$path" ]; then
      unsafe "the receipt lists $path, which is reached through a symlink"
    fi

    if [ ! -e "$path" ]; then
      continue
    elif [ ! -f "$path" ]; then
      keep "$path" "not a regular file"
    elif [ "$path" = "$receipt" ]; then
      remove_receipt=1
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
  if [ -e "$config_dir" ] || [ -L "$config_dir" ]; then
    keep "$config_dir" "settings and credentials, which uninstall never touches"
  fi

  path_hint=
  case ":${PATH:-}:" in
    *":$bin_dir:"*) path_hint="$bin_dir is on your PATH; remove it from the shell profile that adds it" ;;
  esac

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

# The version installed now, or nothing. The receipt wins over the legacy
# version file; an install from before receipts has only the latter.
# Either is advisory: what is not a version counts as nothing installed.
previous_version() {
  [ -f "$target" ] || return 0
  if [ -f "$receipt" ]; then
    v=$(json_value "$receipt" version)
  elif [ -f "$version_file" ]; then
    v=$(head -n 1 "$version_file" | tr -d '\r')
  else
    v=
  fi
  if is_tag "v$v"; then
    printf '%s\n' "$v"
  fi
}

# The highest index serial this install accepted, from the receipt, or 0.
# Advisory like the rest of the receipt: it can only make the rollback
# check below stricter or looser, never let an unsigned index through.
serial_seen() {
  s=
  if [ -f "$receipt" ]; then
    s=$(json_value "$receipt" index_serial)
  fi
  case $s in
    '' | *[!0-9]* | 0* | ????????????????*) echo 0 ;;
    *) echo "$s" ;;
  esac
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

  index_serial=$(json_value "$tmp/release-index.json" serial)
  index_expires=$(json_value "$tmp/release-index.json" expires)
  tag=$(json_value "$tmp/release-index.json" tag)
  # A serial is a positive integer, short enough for shell arithmetic.
  case $index_serial in
    '' | *[!0-9]* | 0* | ????????????????*) index_serial= ;;
  esac
  if [ -z "$index_serial" ] || [ "$(json_value "$tmp/release-index.json" schema)" != 1 ] ||
    ! printf '%s\n' "$index_expires" | grep -Eqx '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z' ||
    ! printf '%s\n' "$tag" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+' ||
    [ "$(json_value "$tmp/release-index.json" version)" != "${tag#v}" ]; then
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
  # Install is the default mode; --check and --uninstall are the others.
  # --version and --allow-downgrade belong to install alone.
  if [ -n "$mode" ]; then
    [ -z "$requested" ] || fail usage "--version cannot be used with $mode"
    [ "$allow_downgrade" = 0 ] || fail usage "--allow-downgrade cannot be used with $mode"
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

  oska_paths
  previous=$(previous_version)

  # An explicit template, so the directory is under $TMPDIR (or /tmp) on
  # every mktemp; BSD and GNU differ on where a bare `mktemp -d` goes.
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/oska-install.XXXXXXXX") ||
    fail install_failed "cannot create a temporary directory"

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

  asset=oskac-$platform
  release=$site/releases/$tag
  say "Downloading oskac $version for $platform from $release/" >&2
  for f in SHA256SUMS SHA256SUMS.sig "$asset"; do
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
  mv -f "$staged" "$target" || fail install_failed "cannot write $target"
  printf '%s\n' "$version" >"$version_file" || fail install_failed "cannot write $version_file"
  write_receipt
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

  if [ "$json" = 1 ]; then
    if [ -n "$previous" ]; then
      previous_json=$(json_str "$previous")
    else
      previous_json=null
    fi
    printf '{"ok": true, "action": %s, "version": %s, "tag": %s, "previous_version": %s, "platform": %s, "path": %s, "bin_dir": %s, "on_path": %s, "checksum_verified": true, "signature_verified": true, "receipt": %s, "index_verified": %s, "index_serial": %s}\n' \
      "$(json_str "$action")" "$(json_str "$version")" "$(json_str "$tag")" "$previous_json" \
      "$(json_str "$platform")" "$(json_str "$target")" "$(json_str "$bin_dir")" "$on_path" \
      "$(json_str "$receipt")" "$index_verified" "${index_serial:-null}"
    return
  fi

  case $action in
    installed) echo "Installed oskac $version to $target." ;;
    reinstalled) echo "Reinstalled oskac $version to $target." ;;
    updated) echo "Updated oskac $previous to $version in $target." ;;
    downgraded) echo "Downgraded oskac $previous to $version in $target." ;;
  esac
  echo "The checksum and the release signature were verified."
  if [ "$on_path" = false ]; then
    echo
    echo "$bin_dir is not on your PATH. Add it, for example in ~/.profile:"
    echo
    echo "  export PATH=\"$bin_dir:\$PATH\""
  fi
}

# Remove the temp and staging directories however the run ends. On a
# signal, clean up, then die of that same signal, so whatever ran the
# installer sees how it ended.
trap cleanup EXIT
for sig in HUP INT TERM; do
  # shellcheck disable=SC2064 # $sig is expanded now, $$ when it fires
  trap "cleanup; trap - $sig EXIT; kill -s $sig \$\$" "$sig"
done

main "$@"
