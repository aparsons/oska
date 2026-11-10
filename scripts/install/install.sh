#!/bin/sh
# Install oskac, the Oska compiler and runner, on Linux or macOS.
#
#   curl -fsSL https://oskalang.dev/install.sh | sh
#   curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.1.0 --json
#
# Options:
#   --version X  install release X (0.1.0 or v0.1.0) instead of the latest
#   --json       print one JSON object on stdout, and nothing else
#   --help       print this help
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
# Re-running it updates oskac in place.

set -u

# The release signing key, pinned when the site is built
# (scripts/install/pin.sh). Signatures by any other key are refused.
RELEASE_KEY='@OSKA_RELEASE_KEY@'
NAMESPACE=oska-release
DEFAULT_SITE_URL=https://oskalang.dev
# The glibc of the Linux build runners (.github/workflows/release.yml).
MIN_GLIBC=2.35

json=0
tmp=
stage=

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

  --version X  install release X (0.1.0 or v0.1.0) instead of the latest
  --json       print one JSON object on stdout, and nothing else
  --help       print this help

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

main() {
  requested=
  while [ $# -gt 0 ]; do
    case $1 in
      --json) json=1 ;;
      --version)
        [ $# -ge 2 ] || fail usage "--version needs a value, like --version 0.1.0"
        requested=$2
        shift ;;
      --version=*) requested=${1#--version=} ;;
      -h | --help)
        usage
        exit 0 ;;
      *) fail usage "unknown option $1 (try --help)" ;;
    esac
    shift
  done

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

  detect_platform
  need curl
  need ssh-keygen "OpenSSH 8.1 or newer, to verify the release signature"
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    need openssl "or sha256sum or shasum, to check the SHA-256 checksum"
  fi

  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t oska-install) ||
    fail install_failed "cannot create a temporary directory"

  if [ -z "$requested" ]; then
    fetch "$site/latest.json" "$tmp/latest.json" ||
      fail download_failed "cannot download $site/latest.json"
    # The release fields of latest.json are top-level, so the first "tag" is
    # the release's. Whatever it names is checked against the signature.
    tag=$(tr ',{}' '[\n*]' <"$tmp/latest.json" |
      sed -n 's/^[[:space:]]*"tag"[[:space:]]*:[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' |
      head -n 1)
    is_tag "$tag" || fail download_failed "$site/latest.json does not name a release"
  fi
  version=${tag#v}

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

  home=${OSKA_HOME:-$HOME/.oska}
  bin_dir=$home/bin
  target=$bin_dir/oskac
  receipt=$home/version
  previous=
  if [ -f "$target" ] && [ -f "$receipt" ]; then
    previous=$(head -n 1 "$receipt")
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
  mv -f "$staged" "$target" || fail install_failed "cannot write $target"
  printf '%s\n' "$version" >"$receipt" || fail install_failed "cannot write $receipt"
  cleanup

  if [ -z "$previous" ]; then
    action=installed
  elif [ "$previous" = "$version" ]; then
    action=reinstalled
  else
    action=updated
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
    printf '{"ok": true, "action": %s, "version": %s, "tag": %s, "previous_version": %s, "platform": %s, "path": %s, "bin_dir": %s, "on_path": %s, "checksum_verified": true, "signature_verified": true}\n' \
      "$(json_str "$action")" "$(json_str "$version")" "$(json_str "$tag")" "$previous_json" \
      "$(json_str "$platform")" "$(json_str "$target")" "$(json_str "$bin_dir")" "$on_path"
    return
  fi

  case $action in
    installed) echo "Installed oskac $version to $target." ;;
    reinstalled) echo "Reinstalled oskac $version to $target." ;;
    updated) echo "Updated oskac $previous to $version in $target." ;;
  esac
  echo "The checksum and the release signature were verified."
  if [ "$on_path" = false ]; then
    echo
    echo "$bin_dir is not on your PATH. Add it, for example in ~/.profile:"
    echo
    echo "  export PATH=\"$bin_dir:\$PATH\""
  fi
}

main "$@"
