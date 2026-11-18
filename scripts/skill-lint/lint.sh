#!/usr/bin/env bash
# Checks the agent skills under .claude/skills/ against the mechanical rules
# of docs/agents/skills.md. The judgement rules (plain language, clean room)
# are for review.
#
#   scripts/skill-lint/lint.sh [SKILLS_DIR]    default: .claude/skills
#
# For each <name>/SKILL.md it checks that:
#   - the file starts with YAML frontmatter, and `name:` equals the folder name;
#   - `description:` is present, at most 1024 characters, and at most 300
#     when the skill is model-invoked (no `disable-model-invocation: true`);
#   - the file has at most 250 lines and a `## Security` section;
#   - every relative Markdown link in the skill folder, outside code fences,
#     names a file that exists;
#   - no file in the skill folder loads a remote script, style sheet or font;
#   - every .sh file is executable, starts with `#!/usr/bin/env bash`, sets
#     `set -euo pipefail`, and parses with `bash -n`.
# It fails closed: an unreadable folder or a missing SKILL.md is an error.
set -euo pipefail

dir=${1:-.claude/skills}
if [ ! -d "$dir" ]; then
  echo "skill-lint: $dir is not a directory" >&2
  exit 1
fi

errors=0
fail() {
  printf 'skill-lint: %s\n' "$1" >&2
  errors=$((errors + 1))
}

# The value of a frontmatter key, with surrounding quotes removed.
front_value() {
  local file=$1 key=$2
  awk -v key="$key" '
    NR == 1 { next }
    /^---[[:space:]]*$/ { exit }
    index($0, key ":") == 1 {
      v = substr($0, length(key) + 2)
      sub(/^[[:space:]]+/, "", v); sub(/[[:space:]]+$/, "", v)
      if (v ~ /^".*"$/ || v ~ /^'\''.*'\''$/) v = substr(v, 2, length(v) - 2)
      print v
      exit
    }' "$file"
}

shopt -s nullglob
found=0
for skill in "$dir"/*/; do
  skill=${skill%/}
  name=${skill##*/}
  file="$skill/SKILL.md"
  found=$((found + 1))
  if [ ! -f "$file" ]; then
    fail "$name: no SKILL.md"
    continue
  fi

  if [ "$(head -n 1 "$file")" != "---" ] || [ "$(grep -c '^---[[:space:]]*$' "$file")" -lt 2 ]; then
    fail "$name: SKILL.md does not start with YAML frontmatter"
    continue
  fi

  fm_name=$(front_value "$file" name)
  [ "$fm_name" = "$name" ] || fail "$name: frontmatter name is '$fm_name', not the folder name"

  desc=$(front_value "$file" description)
  if [ -z "$desc" ]; then
    fail "$name: no description"
  elif [ "${#desc}" -gt 1024 ]; then
    fail "$name: description is ${#desc} characters (maximum 1024)"
  elif [ "$(front_value "$file" disable-model-invocation)" != "true" ] && [ "${#desc}" -gt 300 ]; then
    fail "$name: model-invoked description is ${#desc} characters (maximum 300)"
  fi

  lines=$(wc -l <"$file")
  [ "$lines" -le 250 ] || fail "$name: SKILL.md has $lines lines (maximum 250)"
  grep -q '^## Security' "$file" || fail "$name: SKILL.md has no '## Security' section"

  while IFS= read -r -d '' doc; do
    rel=${doc#"$skill"/}
    docdir=$(dirname "$doc")
    # Relative links: ](path) or ](path#anchor), not URLs, anchors or mail.
    while IFS= read -r target; do
      target=${target%%#*}
      [ -n "$target" ] || continue
      [ -e "$docdir/$target" ] || fail "$name/$rel: link to missing file '$target'"
    done < <(awk '/^[[:space:]]*(```|~~~)/ { fenced = !fenced; next } !fenced' "$doc" | grep -oE '\]\([^)[:space:]]+\)' | sed -E 's/^\]\((.*)\)$/\1/' | grep -vE '^([a-zA-Z][a-zA-Z0-9+.-]*:|#|/|<)' || true)
  done < <(find "$skill" -type f -name '*.md' -print0)

  while IFS= read -r -d '' any; do
    rel=${any#"$skill"/}
    if grep -qiE '<script[^>]+src=["'\'']?(https?:)?//|<link[^>]+href=["'\'']?(https?:)?//|@import[[:space:]]+(url\()?["'\'']?(https?:)?//' "$any"; then
      fail "$name/$rel: loads a remote script, style sheet or font"
    fi
  done < <(find "$skill" -type f \( -name '*.md' -o -name '*.html' -o -name '*.css' -o -name '*.js' \) -print0)

  while IFS= read -r -d '' sh; do
    rel=${sh#"$skill"/}
    [ -x "$sh" ] || fail "$name/$rel: not executable"
    [ "$(head -n 1 "$sh")" = "#!/usr/bin/env bash" ] || fail "$name/$rel: first line is not #!/usr/bin/env bash"
    grep -q '^set -euo pipefail' "$sh" || fail "$name/$rel: no 'set -euo pipefail'"
    bash -n "$sh" 2>/dev/null || fail "$name/$rel: bash -n fails"
  done < <(find "$skill" -type f -name '*.sh' -print0)
done

if [ "$found" -eq 0 ]; then
  fail "no skills in $dir"
fi

if [ "$errors" -gt 0 ]; then
  printf 'skill-lint: %d problem(s) in %s\n' "$errors" "$dir" >&2
  exit 1
fi
printf 'skill-lint: %d skills OK\n' "$found"
