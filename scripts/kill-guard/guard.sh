#!/usr/bin/env bash
# Claude Code PreToolUse hook (registered in .claude/settings.json) that
# refuses Bash commands which kill by pattern, name or session. Agents share
# one machine, and `pkill -f "scripts/check.sh"` kills every agent's check, not
# just the caller's: Claude Code reports a shell that gets SIGTERM as exit 144,
# and those silent cut-offs each cost a 15 to 25 minute rerun.
#
# Claude Code sends the event as JSON on stdin; for a Bash call the command is
# `tool_input.command`. The hook denies with exit 2 and a reason on stderr, and
# allows with exit 0 and no output.
#
# Refused:  pkill (unless scoped with -P to the caller's own shell: `$$`,
#           `${$}`, `$BASHPID` or `$!`), killall, and kill of a pgrep/pidof
#           result (`kill $(pgrep ...)`, `pgrep ... | xargs kill`).
# Allowed:  `kill <pid>`, `pkill -P $$`, and everything that only mentions
#           these words (grep, echo, git commit messages).
#
# Best effort against accidents, not a security boundary: it reads the command
# text without a shell parser, so a command built at run time gets through. It
# needs only bash (no jq). A line of a heredoc that starts with `pkill` is read
# as a command; write such text with the Write tool.

set -euo pipefail

input=$(cat || true)

refuse() {
  {
    printf 'kill-guard: refused: %s\n' "$1"
    printf 'A broad kill (pkill by pattern, name or session, killall, kill of a pgrep result) can end\n'
    printf "other agents' builds and checks on this shared machine (they show up as exit 144).\n"
    printf 'Kill only a PID your own command started: `kill <pid>`, for example\n'
    printf '`cmd & pid=$!; ...; kill "$pid"`, or `pkill -P $$` for your own shell'"'"'s children.\n'
  } >&2
  exit 2
}

# Not a Bash call: nothing to guard.
tool_re='"tool_name"[[:space:]]*:[[:space:]]*"([^"]*)"'
if [[ $input =~ $tool_re ]] && [ "${BASH_REMATCH[1]}" != Bash ]; then
  exit 0
fi

# Without a readable command, refuse only if the raw text has a broad kill.
command_re='"command"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)"'
if ! [[ $input =~ $command_re ]]; then
  loose_re='(^|[^[:alnum:]_-])(pkill|killall)([[:space:]]|$)'
  if [[ $input =~ $loose_re ]]; then
    refuse "the tool input could not be read, and it mentions ${BASH_REMATCH[2]}"
  fi
  exit 0
fi

# Undo the JSON string escapes that matter to reading a command.
cmd=${BASH_REMATCH[1]}
cmd=${cmd//\\\\/$'\x01'}
cmd=${cmd//\\n/$'\n'}
cmd=${cmd//\\r/ }
cmd=${cmd//\\t/ }
cmd=${cmd//\\\"/\"}
cmd=${cmd//\\\//\/}
cmd=${cmd//$'\x01'/\\}

# kill of a name search: `kill $(pgrep -f x)`, `pgrep -f x | xargs kill`.
# A pgrep scoped with -P to the caller's own children is fine.
subst_re='(^|[^[:alnum:]_-])kill[^;&|]*(\$\(|`)[[:space:]]*(pgrep|pidof)[^)`]*'
pipe_re='(pgrep|pidof)[^;&]*\|[[:space:]]*(sudo[[:space:]]+)?xargs[^;&|]*[^[:alnum:]_-]kill'
for re in "$subst_re" "$pipe_re"; do
  if [[ $cmd =~ $re ]] && [[ ${BASH_REMATCH[0]} != *' -P'* ]]; then
    refuse "kill of the processes that pgrep or pidof finds"
  fi
done

# Strip quotes from a word.
unquote() {
  local w=$1
  w=${w//\"/}
  printf '%s' "${w//\'/}"
}

# Check one simple command (the text of one segment).
check_segment() {
  local -a words
  local i=0 w name
  read -ra words <<<"$1"

  # Skip what can come before the command: assignments, wrappers and their
  # options, shell keywords.
  while [ "$i" -lt "${#words[@]}" ]; do
    w=$(unquote "${words[i]}")
    case $w in
      *=* | sudo | doas | env | exec | command | nohup | setsid | time | nice | ionice | timeout | xargs | stdbuf | '{' | '!' | if | then | do | else | elif | while | until | -* | [0-9]*)
        i=$((i + 1))
        ;;
      *) break ;;
    esac
  done
  [ "$i" -lt "${#words[@]}" ] || return 0

  name=$(unquote "${words[i]}")
  name=${name##*/}
  i=$((i + 1))

  case $name in
    killall)
      refuse "killall kills by name"
      ;;
    pkill)
      local scoped=0 next
      while [ "$i" -lt "${#words[@]}" ]; do
        w=$(unquote "${words[i]}")
        case $w in
          -P | --parent)
            next=$(unquote "${words[i + 1]:-}")
            case $next in
              '$$' | '${$}' | '$BASHPID' | '$!') scoped=1 ;;
            esac
            ;;
          -P'$$' | -P'${$}' | -P'$BASHPID' | -P'$!' | --parent='$$' | --parent='${$}' | --parent='$BASHPID' | --parent='$!')
            scoped=1
            ;;
        esac
        i=$((i + 1))
      done
      if [ "$scoped" -eq 0 ]; then
        refuse "pkill kills by pattern, name or session, and is not scoped with -P to your own shell"
      fi
      ;;
    bash | sh | zsh | dash)
      # `bash -c "pkill -f x"`: check the command string.
      while [ "$i" -lt "${#words[@]}" ]; do
        w=$(unquote "${words[i]}")
        i=$((i + 1))
        case $w in
          -*c*)
            check_segment "${words[*]:i}"
            break
            ;;
        esac
      done
      ;;
  esac
}

# Cut the command into simple commands at separators, then check each.
seg=$cmd
seg=${seg//&&/$'\n'}
seg=${seg//||/$'\n'}
for sep in ';' '&' '|' '`' '(' ')'; do
  seg=${seg//"$sep"/$'\n'}
done
while IFS= read -r line; do
  check_segment "$line"
done <<<"$seg"

exit 0
