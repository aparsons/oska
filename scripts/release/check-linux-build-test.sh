#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for scripts/release/check-linux-build.sh, the check the release
# build job runs on each Linux build. Run from anywhere:
#
#   scripts/release/check-linux-build-test.sh [NAME-REGEX]
#
# The musl checks run the real GNU readelf on small ELF files written here
# by hand; the glibc check runs against a fake objdump, since a binary that
# needs a given glibc cannot be written by hand. Needs GNU binutils
# (readelf), as on the Ubuntu runners; without it this stops at once and
# says so, and scripts/check.sh skips it outside CI.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
check=$here/check-linux-build.sh

if ! grep -q 'GNU readelf' <<<"$(readelf --version 2>/dev/null || true)"; then
  echo "check-linux-build-test.sh: needs GNU readelf (binutils), which is not on PATH" >&2
  exit 1
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

failures=0
passes=0

fail() {
  echo "    $*" >&2
  return 1
}

run_test() {
  local name=$1
  local dir="$scratch/$name"
  mkdir -p "$dir"
  # Outside any `if`, `&&` or `||`, so that `set -e` holds in the test.
  local status
  set +e
  (
    set -e
    cd "$dir"
    "$name"
  ) >"$dir.log" 2>&1
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    passes=$((passes + 1))
    echo "ok   $name"
  else
    failures=$((failures + 1))
    echo "FAIL $name"
    sed 's/^/    /' "$dir.log"
  fi
}

# Writes <count> bytes of the little-endian <value>.
le() {
  local value=$1 count=$2 i escapes=
  for ((i = 0; i < count; i++)); do
    escapes+=$(printf '\\x%02x' $(((value >> (8 * i)) & 255)))
  done
  # shellcheck disable=SC2059 # the format is the \xHH escapes
  printf "$escapes"
}

# One ELF64 program header: type, flags, offset (also its address), size.
program_header() {
  le "$1" 4
  le "$2" 4
  le "$3" 8
  le "$3" 8
  le "$3" 8
  le "$4" 8
  le "$4" 8
  le 8 8
}

# Writes to ./<name> a minimal x86_64 ELF executable, as a static build
# is: one PT_LOAD and nothing else. With `interp` it also asks for a
# dynamic loader (PT_INTERP); with `needed` it also has a dynamic section
# that names a shared library (DT_NEEDED libc.so.6).
tiny_elf() {
  local name=$1 interp=0 needed=0 count=1 flag data interp_at strtab_at dynamic_at size
  shift
  for flag in "$@"; do
    case $flag in
      interp) interp=1 ;;
      needed) needed=1 ;;
    esac
  done
  count=$((1 + interp + needed))
  data=$((64 + 56 * count))
  interp_at=$data           # "/lib/ld-linux.so.2\0", 24 bytes with padding
  strtab_at=$((data + 24))  # "\0libc.so.6\0", 16 bytes with padding
  dynamic_at=$((data + 40)) # NEEDED, STRTAB, STRSZ, NULL: 64 bytes
  size=$((data + 104))
  {
    printf '\177ELF\002\001\001\000\000\000\000\000\000\000\000\000'
    le 2 2       # ET_EXEC
    le 62 2      # EM_X86_64
    le 1 4       # EV_CURRENT
    le "$data" 8 # entry
    le 64 8      # program headers
    le 0 8       # no section headers
    le 0 4
    le 64 2
    le 56 2
    le "$count" 2
    le 64 2
    le 0 2
    le 0 2
    program_header 1 5 0 "$size"
    if [ "$interp" = 1 ]; then
      program_header 3 4 "$interp_at" 19
    fi
    if [ "$needed" = 1 ]; then
      program_header 2 6 "$dynamic_at" 64
    fi
    printf '/lib/ld-linux.so.2\000\000\000\000\000\000'
    printf '\000libc.so.6\000\000\000\000\000\000'
    le 1 8
    le 1 8
    le 5 8
    le "$strtab_at" 8
    le 10 8
    le 11 8
    le 0 8
    le 0 8
  } >"$name"
  chmod +x "$name"
}

test_check_linux_build_accepts_a_static_musl_build() {
  tiny_elf oskac
  "$check" musl oskac || fail "refused a static executable"
}

test_check_linux_build_refuses_a_musl_build_that_asks_for_a_dynamic_loader() {
  tiny_elf oskac interp
  if "$check" musl oskac 2>err; then
    fail "accepted an executable with a PT_INTERP"
  fi
  grep -q PT_INTERP err || fail "the error does not name PT_INTERP: $(cat err)"
}

test_check_linux_build_refuses_a_musl_build_that_needs_a_shared_library() {
  tiny_elf oskac needed
  if "$check" musl oskac 2>err; then
    fail "accepted an executable with a DT_NEEDED"
  fi
  grep -q DT_NEEDED err || fail "the error does not name DT_NEEDED: $(cat err)"
  grep -q libc.so.6 err || fail "the error does not name the library: $(cat err)"
}

test_check_linux_build_refuses_what_it_cannot_read_as_elf() {
  printf '#!/bin/sh\necho oskac\n' >oskac
  if "$check" musl oskac 2>err; then
    fail "accepted a shell script as a static executable"
  fi
  if "$check" musl missing 2>err; then
    fail "accepted a file that is not there"
  fi
  if "$check" static oskac 2>err; then
    fail "accepted a build that is neither gnu nor musl"
  fi
}

# Puts in ./fakebin an objdump whose `-T` prints a dynamic symbol table, in
# GNU objdump's format, that needs each glibc symbol version given.
fake_objdump() {
  mkdir -p fakebin
  {
    printf '#!/bin/sh\n'
    # shellcheck disable=SC2016 # $2 is the fake objdump's, expanded when it runs
    printf 'printf "\\n%%s:     file format elf64-x86-64\\n\\nDYNAMIC SYMBOL TABLE:\\n" "$2"\n'
    for version in "$@"; do
      printf 'printf "0000000000000000      DF *UND*\\t0000000000000000 (%s) f\\n"\n' "$version"
    done
  } >fakebin/objdump
  chmod +x fakebin/objdump
}

test_check_linux_build_accepts_a_glibc_build_that_needs_at_most_glibc_2_35() {
  tiny_elf oskac interp needed
  # 2.4 sorts after 2.35 as text, but is older.
  fake_objdump GLIBC_2.2.5 GLIBC_2.4 GLIBC_2.35 GLIBC_PRIVATE
  PATH="$PWD/fakebin:$PATH" "$check" gnu oskac >out ||
    fail "refused a build that needs glibc 2.35"
  grep -q 'GLIBC_2\.35' out || fail "does not say which glibc it needs: $(cat out)"
}

test_check_linux_build_refuses_a_glibc_build_that_needs_a_newer_glibc() {
  tiny_elf oskac interp needed
  fake_objdump GLIBC_2.2.5 GLIBC_2.36 GLIBC_2.4
  if PATH="$PWD/fakebin:$PATH" "$check" gnu oskac 2>err; then
    fail "accepted a build that needs glibc 2.36"
  fi
  grep -q 'GLIBC_2\.36' err || fail "the error does not name GLIBC_2.36: $(cat err)"
}

test_check_linux_build_refuses_a_glibc_build_with_no_glibc_symbol_versions() {
  tiny_elf oskac
  fake_objdump
  if PATH="$PWD/fakebin:$PATH" "$check" gnu oskac 2>err; then
    fail "accepted a glibc build that names no glibc version"
  fi
  grep -q 'needs no glibc symbol version' err || fail "the error does not say why: $(cat err)"
}

test_check_linux_build_refuses_a_glibc_build_it_cannot_read() {
  # A real objdump on a file that is not there, and on one it cannot read.
  if "$check" gnu missing 2>err; then
    fail "accepted a file that is not there"
  fi
  grep -q 'no file missing' err || fail "the error does not name the file: $(cat err)"
  printf 'not an executable\n' >oskac
  if "$check" gnu oskac 2>err; then
    fail "accepted a file objdump cannot read"
  fi
  grep -q 'objdump cannot read' err || fail "the error does not say objdump failed: $(cat err)"
}

test_check_linux_build_refuses_a_glibc_build_when_objdump_fails() {
  tiny_elf oskac interp needed
  # An objdump that prints a good table, then fails: its output is not
  # trusted.
  mkdir -p fakebin
  printf '#!/bin/sh\nprintf "0000000000000000      DF *UND*\\t0000000000000000 (GLIBC_2.17) f\\n"\nexit 1\n' \
    >fakebin/objdump
  chmod +x fakebin/objdump
  if PATH="$PWD/fakebin:$PATH" "$check" gnu oskac 2>err; then
    fail "accepted a build objdump failed on"
  fi
  grep -q 'objdump cannot read' err || fail "the error does not say objdump failed: $(cat err)"
}

# A program header table long enough that grep -q stops reading it at the
# first INTERP line, long before the end: the check must still refuse.
test_check_linux_build_refuses_an_interp_at_the_start_of_a_long_header_table() {
  tiny_elf oskac interp
  mkdir -p fakebin
  real=$(command -v readelf)
  cat >fakebin/readelf <<SH
#!/bin/sh
case "\$1" in
  -lW)
    echo "  INTERP         0x0000b0 0x00000000000000b0 0x00000000000000b0 0x000013 0x000013 R   0x8"
    i=0
    while [ \$i -lt 20000 ]; do
      echo "  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x000118 0x000118 R E 0x8"
      i=\$((i + 1))
    done ;;
  *) exec "$real" "\$@" ;;
esac
SH
  chmod +x fakebin/readelf
  if PATH="$PWD/fakebin:$PATH" "$check" musl oskac 2>err; then
    fail "accepted a PT_INTERP that came first in a long table"
  fi
  grep -q PT_INTERP err || fail "the error does not name PT_INTERP: $(cat err)"
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
