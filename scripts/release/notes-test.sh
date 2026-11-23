#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Tests for the release notes, scripts/release/notes.sh. Run from anywhere:
#
#   scripts/release/notes-test.sh
#
# Every GitHub release body is its version's CHANGELOG.md section, then a
# footer: the download page, the release key's fingerprint, the image by
# digest and the version's anchor on /changelog.html. Each test works in
# its own scratch directory, on a fixture changelog and a throwaway key.
# scripts/release/test.sh checks the notes step's place in release.yml.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
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
  # Run outside any `if`, `&&` or `||` so that `set -e` holds inside the
  # test: a test stops at its first failing command.
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

# Writes a throwaway key pair to ./<name> and ./<name>.pub.
throwaway_key() {
  ssh-keygen -q -t ed25519 -N '' -C "$1" -f "$1"
}

# The SHA256 fingerprint of the OpenSSH public key in file $1, as
# `ssh-keygen -l -E sha256` prints it, worked out without ssh-keygen: the
# unpadded base64 of the SHA-256 of the key blob.
fingerprint_of() {
  local hex
  hex=$(awk '{print $2}' "$1" | base64 -d | sha256sum | cut -c1-64)
  # shellcheck disable=SC2059 # the format is the hex digest as \xHH escapes
  printf "SHA256:%s\n" "$(printf "$(printf '%s' "$hex" | sed 's/../\\x&/g')" | base64 | tr -d '=\n')"
}

# Writes ./CHANGELOG.md with an Unreleased section and two versions.
fixture_changelog() {
  cat >CHANGELOG.md <<'MD'
# Changelog

Every release's notes, newest first.

## Unreleased

- Something on main, not released yet.

## v1.10.0

Released 2026-12-01.

v1.10.0 makes things faster. These are the changes you can see:

- **`oskac run` is faster.** Nothing to do.

### Security fixes

- The installer checks one more thing.

## v1.9.0

Released 2026-11-01.

- Older notes.
MD
}

# Writes ./digest: one image digest line, as assemble.sh writes it.
fixture_digest() {
  printf 'sha256:%s\n' 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef >digest
}

test_notes_print_the_section_of_the_tag_then_the_footer() {
  fixture_changelog
  fixture_digest
  throwaway_key key
  "$here/notes.sh" v1.10.0 CHANGELOG.md key.pub digest >notes.md
  cat >expected <<MD
Released 2026-12-01.

v1.10.0 makes things faster. These are the changes you can see:

- **\`oskac run\` is faster.** Nothing to do.

### Security fixes

- The installer checks one more thing.

---

- Download and verify: https://oskalang.dev/releases/v1.10.0/
- Release key fingerprint: \`$(fingerprint_of key.pub)\`. Check it as [Verify a release by hand](https://oskalang.dev/guide/install.html#verify-a-release-by-hand) says.
- Container image: \`ghcr.io/aparsons/oskac@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\`
- These notes on oskalang.dev: https://oskalang.dev/changelog.html#v1100
MD
  diff expected notes.md || fail "unexpected release notes"
}

test_notes_of_a_pre_release_use_its_base_version_section() {
  fixture_changelog
  throwaway_key key
  "$here/notes.sh" v1.10.0-rc.1 CHANGELOG.md key.pub >notes.md
  grep -qx 'v1.10.0 makes things faster. These are the changes you can see:' notes.md ||
    fail "not the v1.10.0 section: $(cat notes.md)"
  grep -q 'Older notes' notes.md && fail "printed the v1.9.0 section too"
  grep -qx -- '- Download and verify: https://oskalang.dev/releases/v1.10.0-rc.1/' notes.md ||
    fail "the download page is not the pre-release's"
  grep -qx -- '- These notes on oskalang.dev: https://oskalang.dev/changelog.html#v1100' notes.md ||
    fail "the changelog link is not the v1.10.0 anchor"
  true
}

# Releases before the image (v0.6.0 and older) have no digest file.
test_notes_without_a_digest_file_name_no_image() {
  fixture_changelog
  throwaway_key key
  "$here/notes.sh" v1.9.0 CHANGELOG.md key.pub >notes.md
  grep -q 'ghcr.io' notes.md && fail "names an image: $(cat notes.md)"
  grep -q "Release key fingerprint: \`$(fingerprint_of key.pub)\`" notes.md || fail "no fingerprint"
  [ "$(head -n 3 notes.md)" = $'Released 2026-11-01.\n\n- Older notes.' ] || fail "not the v1.9.0 section"
}

# Each refusal prints nothing to stdout, so a body is never half written.
test_notes_refuse_a_version_with_no_section_or_an_empty_one() {
  fixture_changelog
  throwaway_key key
  if "$here/notes.sh" v1.11.0 CHANGELOG.md key.pub >out 2>err; then
    fail "wrote notes for a version with no section"
  fi
  grep -q "no section '## v1.11.0'" err || fail "error does not say why: $(cat err)"
  [ ! -s out ] || fail "printed a body: $(cat out)"
  # "Unreleased" is not a version's section, nor is a heading that only
  # starts with the version.
  sed -i 's/^## v1.10.0$/## v1.10.0-beta/' CHANGELOG.md
  if "$here/notes.sh" v1.10.0 CHANGELOG.md key.pub >out 2>err; then
    fail "took '## v1.10.0-beta' for v1.10.0"
  fi
  # A section with only its date, or nothing, is empty.
  printf '# Changelog\n\n## v2.0.0\n\nReleased 2027-01-01.\n\n## v1.0.0\n\n- x\n' >CHANGELOG.md
  if "$here/notes.sh" v2.0.0 CHANGELOG.md key.pub >out 2>err; then
    fail "wrote notes for a section with only its date"
  fi
  grep -q 'empty' err || fail "error does not say the section is empty: $(cat err)"
  [ ! -s out ] || fail "printed a body: $(cat out)"
  printf '# Changelog\n\n## v2.0.0\n\n\n## v1.0.0\n\n- x\n' >CHANGELOG.md
  if "$here/notes.sh" v2.0.0 CHANGELOG.md key.pub >out 2>err; then
    fail "wrote notes for an empty section"
  fi
  # Two sections for one version: which is the release's is not clear.
  printf '# Changelog\n\n## v2.0.0\n\n- a\n\n## v2.0.0\n\n- b\n' >CHANGELOG.md
  if "$here/notes.sh" v2.0.0 CHANGELOG.md key.pub >out 2>err; then
    fail "wrote notes with two v2.0.0 sections"
  fi
}

test_notes_refuse_a_malformed_digest_file() {
  fixture_changelog
  throwaway_key key
  local hex=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef bad
  for bad in "sha256:$hex" "sha256:$hex"$'\n'"sha256:$hex"$'\n' "sha256:${hex}0"$'\n' \
    "sha512:$hex"$'\n' "sha256:$hex x"$'\n' "latest"$'\n' "" "sha256:${hex^^}"$'\n'; do
    printf '%s' "$bad" >digest
    if "$here/notes.sh" v1.10.0 CHANGELOG.md key.pub digest >out 2>err; then
      fail "wrote notes with the digest file $(printf '%q' "$bad")"
    fi
    grep -q 'not one line' err || fail "error does not say why: $(cat err)"
    [ ! -s out ] || fail "printed a body: $(cat out)"
  done
}

test_notes_refuse_unreadable_files_and_a_tag_that_is_not_a_version() {
  fixture_changelog
  fixture_digest
  throwaway_key key
  "$here/notes.sh" v1.10.0 CHANGELOG.md key.pub digest >/dev/null || fail "the fixtures do not work"
  local args
  for args in "v1.10.0 missing.md key.pub digest" "v1.10.0 CHANGELOG.md missing.pub digest" \
    "v1.10.0 CHANGELOG.md key.pub missing" "v1.10.0 CHANGELOG.md CHANGELOG.md digest" \
    "1.10.0 CHANGELOG.md key.pub digest" "v1.10.0/../x CHANGELOG.md key.pub digest" \
    "v1.10.0 CHANGELOG.md" "v1.10.0 CHANGELOG.md key.pub digest extra"; do
    # shellcheck disable=SC2086 # split into arguments on purpose
    if "$here/notes.sh" $args >out 2>err; then
      fail "wrote notes from: $args"
    fi
    [ ! -s out ] || fail "printed a body from: $args"
  done
}

# CHANGELOG.md links relative to the repository root, which a release page
# is not: those links point at the tag's files. Others stay as they are.
test_notes_point_relative_links_at_the_tags_files() {
  throwaway_key key
  cat >CHANGELOG.md <<'MD'
# Changelog

## v1.10.0

- See [the guide](docs/guide/install.md#the-two-linux-builds), [ADR 22](docs/adr/0022-x.md), [the site](https://oskalang.dev/), [below](#v190) and [mail](mailto:a@example.test).
MD
  "$here/notes.sh" v1.10.0-rc.1 CHANGELOG.md key.pub >notes.md
  grep -qxF -- '- See [the guide](https://github.com/aparsons/oska/blob/v1.10.0-rc.1/docs/guide/install.md#the-two-linux-builds), [ADR 22](https://github.com/aparsons/oska/blob/v1.10.0-rc.1/docs/adr/0022-x.md), [the site](https://oskalang.dev/), [below](#v190) and [mail](mailto:a@example.test).' notes.md ||
    fail "links: $(head -n 1 notes.md)"
}

# Prints the `run: |` block of the step named $2 in workflow $1, with its
# indentation taken off, so a test can run the workflow's own commands.
step_run() {
  awk -v want="- name: $2" '
    { line = $0; sub(/^[[:space:]]*/, "", line) }
    line == want { step = 1; next }
    step && line ~ /^- / { exit }
    step && line == "run: |" { run = 1; ind = -1; next }
    run {
      if ($0 ~ /^[[:space:]]*$/) { print ""; next }
      cur = $0; sub(/[^ ].*$/, "", cur); cur = length(cur)
      if (ind < 0) ind = cur
      if (cur < ind) exit
      print substr($0, ind + 1)
    }
  ' "$1"
}

# The release run's own steps, end to end: assemble's notes step writes
# the body beside the release, outside what SHA256SUMS lists, and publish
# hands that file to `gh release create --notes-file`, never through a
# command line. A section full of shell metacharacters reaches gh as
# bytes and runs nothing.
test_the_release_run_passes_the_notes_to_gh_as_a_file() {
  local wf=$here/../../.github/workflows/release.yml
  step_run "$wf" 'Write the release notes' >notes-step.sh
  step_run "$wf" 'Store the files in a GitHub Release' >publish-step.sh
  [ -s notes-step.sh ] || fail "assemble has no step 'Write the release notes'"
  [ -s publish-step.sh ] || fail "publish has no step 'Store the files in a GitHub Release'"
  # A checkout: the scripts, the key and a changelog whose section is
  # hostile.
  mkdir -p checkout/scripts checkout/site bin
  ln -s "$here" checkout/scripts/release
  throwaway_key checkout/site/oska-release.pub.key
  mv checkout/site/oska-release.pub.key.pub checkout/site/oska-release.pub
  # shellcheck disable=SC2016 # literal metacharacters, never expanded
  local hostile='- $(touch pwned-1) `touch pwned-2` "; touch pwned-3; " '"'"'; touch pwned-4; '"'"' $HOME ${TAG} \ & | > pwned-5 <<EOF'
  printf '# Changelog\n\n## v1.10.0\n\nReleased 2026-12-01.\n\n%s\n' "$hostile" >checkout/CHANGELOG.md
  # A release as assemble.sh leaves it: the notes step must not touch it.
  mkdir -p checkout/release/v1.10.0
  (cd checkout && fixture_digest && mv digest release/v1.10.0/oskac-image-digest)
  printf '# oska-release v1.10.0\n' >checkout/release/v1.10.0/SHA256SUMS
  cp -R checkout/release checkout/release.before
  # A fake gh that records its arguments and the notes file it is given.
  cat >bin/gh <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$GH_LOG"
while [ $# -gt 0 ]; do
  if [ "$1" = --notes-file ]; then cp "$2" "$GH_NOTES"; fi
  shift
done
SH
  chmod +x bin/gh
  (
    cd checkout
    export TAG=v1.10.0 GH_LOG=$PWD/../gh.log GH_NOTES=$PWD/../gh.notes PATH=$PWD/../bin:$PATH
    bash -euo pipefail ../notes-step.sh >../notes-step.out
    bash -euo pipefail ../publish-step.sh
  )
  # Beside the release, not in it: SHA256SUMS and the assets are unchanged.
  [ -f checkout/release/release-notes.md ] || fail "the notes step wrote no release/release-notes.md"
  diff -r checkout/release.before/v1.10.0 checkout/release/v1.10.0 || fail "the notes step changed the release"
  [ "$(grep -A1 -x -- '--notes-file' gh.log | tail -n 1)" = release/release-notes.md ] ||
    fail "gh is not given --notes-file release/release-notes.md: $(cat gh.log)"
  # Named once, after --notes-file: not uploaded as an asset as well.
  [ "$(grep -c 'release-notes' gh.log)" = 1 ] || fail "the notes are uploaded as an asset: $(cat gh.log)"
  grep -qx -- '--notes' gh.log && fail "gh is given the notes on its command line"
  diff <("$here/notes.sh" v1.10.0 checkout/CHANGELOG.md checkout/site/oska-release.pub \
    checkout/release/v1.10.0/oskac-image-digest) gh.notes || fail "gh got other notes"
  grep -qxF -- "$hostile" gh.notes || fail "the hostile line did not arrive as it is"
  ! ls checkout/pwned-* ./pwned-* 2>/dev/null || fail "a shell ran part of the notes"
}

# Headings are not notes: a section with only its date and empty
# subheadings, such as a "Security fixes" list with nothing in it, is empty.
test_notes_refuse_a_section_with_only_headings() {
  throwaway_key key
  printf '# Changelog\n\n## v2.0.0\n\nReleased 2027-01-01.\n\n### Security fixes\n\n#### More\n\n## v1.0.0\n\n- x\n' >CHANGELOG.md
  if "$here/notes.sh" v2.0.0 CHANGELOG.md key.pub >out 2>err; then
    fail "wrote notes for a section with only headings: $(cat out)"
  fi
  grep -q 'empty' err || fail "error does not say the section is empty: $(cat err)"
  [ ! -s out ] || fail "printed a body: $(cat out)"
}

# A `## ` line inside a fenced code block, indented in a list item, or
# after a fence of the other marker, is code, not the next section.
test_notes_keep_a_heading_inside_a_code_block_in_the_section() {
  throwaway_key key
  cat >CHANGELOG.md <<'MD'
# Changelog

## v2.0.0

- Run this:

  ```sh
## v1.0.0
  ~~~
## not a heading either
  ```

- After the block.

~~~text
```
## still code
~~~

- Last line.

## v1.0.0

- Older.
MD
  "$here/notes.sh" v2.0.0 CHANGELOG.md key.pub >notes.md
  grep -qx -- '- After the block.' notes.md || fail "the section ended inside the indented block: $(cat notes.md)"
  grep -qx -- '- Last line.' notes.md || fail "the section ended at a fence of the other marker: $(cat notes.md)"
  grep -qx -- '- Older.' notes.md && fail "printed the v1.0.0 section too"
  true
}

for t in $(declare -F | awk '{print $3}' | grep '^test_' | grep -E -- "${1:-.}"); do
  run_test "$t"
done

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
