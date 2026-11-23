#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# block-dangerous-git.sh: a Claude Code PreToolUse hook for the Bash tool.
#
# Claude Code sends the hook event as JSON on stdin. The command is in
# .tool_input.command. This script reads that text and blocks destructive git
# commands before they run.
#   Blocked: exit 2. The reason goes to stderr.
#   Allowed: exit 0. No output.
#
# It fails closed. These cases all give exit 2:
#   - jq is missing, the input is not JSON, or the command field is missing;
#   - the command text has an unclosed quote or substitution;
#   - the config block below is not valid;
#   - the script stops on an error.
#
# How it reads a command line:
#   - It splits the line on ; & | ( ) newlines, $( ) and backticks, and checks
#     each sub-command. Text inside $( ) or backticks is a sub-command too,
#     also inside double quotes.
#   - Quoted text is one word. So `git commit -m "reset --hard"` is allowed:
#     the quoted text is a message, not a command.
#   - It removes prefixes before the command word: VAR=value, command, exec,
#     sudo, doas, env, nice, nohup, time, timeout, xargs, stdbuf, builtin.
#   - It reads `git` with any path prefix (/usr/bin/git), and skips git global
#     options (-C <dir>, -c <k=v>, --git-dir, --work-tree, --no-pager, ...).
#   - It checks the script of `bash -c`, `sh -c`, `eval` and `env -S`, and the
#     body of a heredoc that goes into a shell, up to MAX_DEPTH levels.
#   - The body of a quoted heredoc (<<'EOF') is data. It is not checked.
#
# It is a safety net against mistakes. It is NOT a security boundary. It reads
# text only and it does not run git. So it cannot see the current branch or
# the remote's default branch, a command in a script file, a variable used as
# the command word, or text piped into a shell. Use server-side branch
# protection as the real control.

set -euo pipefail

# ============================== CONFIG START ===============================
# POLICY: "standard" or "strict".
#   standard: allow a normal push of a feature branch. Block the destructive
#             list (see the SKILL.md of git-guardrails).
#   strict:   block the destructive list and also every git push.
POLICY="standard"

# A push to one of these branches is always blocked. Add the default branch
# of each remote if it is not main or master (for example trunk or develop).
PROTECTED_BRANCHES=(main master)

# A push of a tag whose name matches one of these shell patterns is blocked,
# and so is `git push --tags` or `--follow-tags` while the list is not empty.
# The default blocks release tags such as v1.2.3, because in many repos (Oska
# too) only a maintainer cuts a release. A bare name such as `v1.2` is read as
# a tag, because the guard cannot tell a tag from a branch. Set
# PROTECTED_TAGS=() to allow every tag push.
PROTECTED_TAGS=('v[0-9]*')

# Extra patterns to block. Each entry is a bash extended regular expression.
# The guard matches it against each sub-command in a normal form: the words
# joined by one space, with the prefixes removed. For git, the form is
# "git <subcommand> <args>" without the global options.
EXTRA_PATTERNS=(
  # '^git rebase( |$)'
  # '^git commit .*--amend( |$)'
)
# =============================== CONFIG END ================================

MAX_DEPTH=3          # nested bash -c / eval / heredoc levels
MAX_PREFIX_WORDS=32  # prefix words removed before the command word
US=$'\x1f'           # separates the words of one sub-command
SEGMENTS=()

block() {
  {
    printf 'git-guardrails: blocked: %s\n' "$1"
    printf 'The user has not allowed this command, so do not try another way to run it.\n'
    printf 'Ask the user to run it themselves, or to change the git-guardrails config.\n'
  } >&2
  exit 2
}

# shellcheck disable=SC2329  # called by the EXIT trap
on_exit() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "$rc" -ne 2 ]; then
    printf 'git-guardrails: blocked: the guard stopped on an error (exit %s), so it fails closed.\n' "$rc" >&2
    exit 2
  fi
}
trap on_exit EXIT

if [ "${BASH_VERSINFO[0]}" -lt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -lt 4 ]; }; then
  block "the guard needs bash 4.4 or later (this is bash ${BASH_VERSION})"
fi

export LC_ALL=C

case $POLICY in
  standard | strict) ;;
  *) block "the config has an unknown POLICY \"$POLICY\" (use standard or strict)" ;;
esac

# ------------------------------------------------------------------ lexer ---
# The helpers below change the locals of lex() (bash uses dynamic scope).

lex_flush() {
  if [ "$have" -eq 1 ]; then
    cur+="$tok$US"
  fi
  tok=""
  have=0
}

lex_end_segment() {
  lex_flush
  if [ -n "$cur" ]; then
    SEGMENTS+=("$cur")
  fi
  cur=""
}

lex_push() {
  st_q+=("$q")
  st_kind+=("$1")
  st_cur+=("$cur")
  st_tok+=("$tok")
  cur=""
  tok=""
  have=0
  q=n
}

lex_pop() {
  local last=$((${#st_kind[@]} - 1))
  lex_end_segment
  q=${st_q[last]}
  cur=${st_cur[last]}
  tok="${st_tok[last]}\$()"
  have=1
  unset 'st_q[last]' 'st_kind[last]' 'st_cur[last]' 'st_tok[last]'
}

# lex TEXT MODE DEPTH
#   MODE n: shell text. MODE h: the body of an unquoted heredoc (it acts like
#   a double-quoted string with no closing quote).
#   Adds each sub-command to SEGMENTS. Returns 1 if TEXT cannot be parsed.
lex() {
  local s=$1 q=$2 depth=$3
  local start_mode=$2
  local n=${#s} i=0 c c2 c3 tok="" have=0 cur=""
  local rest part line cmp body delim quoted strip k shell_in
  local -a st_q=() st_kind=() st_cur=() st_tok=()
  local -a hd_delim=() hd_quoted=() hd_strip=() hd_shell=()

  if [ "$depth" -gt "$MAX_DEPTH" ]; then
    return 1
  fi

  while [ "$i" -lt "$n" ]; do
    c=${s:i:1}
    c2=${s:i+1:1}

    # Single quotes: everything is literal.
    if [ "$q" = s ]; then
      if [ "$c" = "'" ]; then q=n; else tok+=$c; fi
      i=$((i + 1))
      continue
    fi
    # $'...' quotes: a backslash keeps the next character.
    if [ "$q" = a ]; then
      if [ "$c" = $'\\' ]; then
        tok+=$c2
        i=$((i + 2))
        continue
      fi
      if [ "$c" = "'" ]; then q=n; else tok+=$c; fi
      i=$((i + 1))
      continue
    fi

    # Backslash escapes and line joins.
    if [ "$c" = $'\\' ]; then
      if [ $((i + 1)) -ge "$n" ]; then
        tok+=$c
        have=1
        i=$((i + 1))
        continue
      fi
      if [ "$c2" = $'\n' ]; then
        i=$((i + 2))
        continue
      fi
      case $q in
        d) [[ $c2 == [\"\$\`\\] ]] || tok+=$c ;;
        h) [[ $c2 == [\$\`\\] ]] || tok+=$c ;;
      esac
      tok+=$c2
      have=1
      i=$((i + 2))
      continue
    fi

    # Command substitution $( ... ) and arithmetic $(( ... )).
    if [ "$c" = '$' ] && [ "$c2" = '(' ]; then
      if [ "${s:i+2:1}" = '(' ]; then
        rest=${s:i+3}
        if [[ $rest != *'))'* ]]; then
          return 1
        fi
        part=${rest%%'))'*}
        i=$((i + 3 + ${#part} + 2))
        tok+='$'
        have=1
        continue
      fi
      lex_push p
      i=$((i + 2))
      continue
    fi
    # Backticks.
    if [ "$c" = '`' ]; then
      if [ "$q" = n ] && [ "${#st_kind[@]}" -gt 0 ] && [ "${st_kind[-1]}" = b ]; then
        lex_pop
      else
        lex_push b
      fi
      i=$((i + 1))
      continue
    fi

    # Double quotes and heredoc bodies.
    if [ "$q" = d ]; then
      if [ "$c" = '"' ]; then q=n; else tok+=$c; fi
      i=$((i + 1))
      continue
    fi
    if [ "$q" = h ]; then
      tok+=$c
      i=$((i + 1))
      continue
    fi

    # Plain shell text.
    case $c in
      ' ' | $'\t')
        lex_flush
        ;;
      $'\n')
        lex_end_segment
        i=$((i + 1))
        for k in "${!hd_delim[@]}"; do
          body=""
          while [ "$i" -lt "$n" ]; do
            rest=${s:i}
            line=${rest%%$'\n'*}
            i=$((i + ${#line} + 1))
            cmp=$line
            if [ "${hd_strip[k]}" -eq 1 ]; then
              cmp=${line#"${line%%[!$'\t']*}"}
            fi
            if [ "$cmp" = "${hd_delim[k]}" ]; then
              break
            fi
            body+=$line$'\n'
          done
          if [ "${hd_shell[k]}" -eq 1 ]; then
            # The body is a script for a shell: check it as commands.
            lex "$body" n $((depth + 1)) || return 1
          elif [ "${hd_quoted[k]}" -eq 0 ]; then
            # The body is data, but $( ) and backticks in it still run.
            lex "$body" h $((depth + 1)) || return 1
          fi
        done
        hd_delim=()
        hd_quoted=()
        hd_strip=()
        hd_shell=()
        continue
        ;;
      ';' | '|' | '(')
        lex_end_segment
        ;;
      '&')
        if [[ $tok == *[\<\>] ]]; then
          tok+=$c
        else
          lex_end_segment
        fi
        ;;
      ')')
        if [ "${#st_kind[@]}" -gt 0 ] && [ "${st_kind[-1]}" = p ]; then
          lex_pop
        else
          lex_end_segment
        fi
        ;;
      "'")
        q=s
        have=1
        ;;
      '"')
        q=d
        have=1
        ;;
      '#')
        if [ "$have" -eq 0 ]; then
          rest=${s:i}
          line=${rest%%$'\n'*}
          i=$((i + ${#line}))
          continue
        fi
        tok+=$c
        ;;
      '$')
        if [ "$c2" = "'" ]; then
          q=a
          have=1
          i=$((i + 2))
          continue
        fi
        if [ "$c2" = '"' ]; then
          i=$((i + 1))
          continue
        fi
        tok+=$c
        have=1
        ;;
      '<')
        c3=${s:i+2:1}
        if [ "$c2" = '<' ] && [ "$c3" = '<' ]; then
          tok+='<<<'
          have=1
          i=$((i + 3))
          continue
        fi
        if [ "$c2" != '<' ]; then
          tok+=$c
          have=1
          i=$((i + 1))
          continue
        fi
        # A heredoc: read the delimiter word now, the body after the newline.
        lex_flush
        i=$((i + 2))
        strip=0
        if [ "${s:i:1}" = '-' ]; then
          strip=1
          i=$((i + 1))
        fi
        while [ "$i" -lt "$n" ] && [[ ${s:i:1} == [\ $'\t'] ]]; do
          i=$((i + 1))
        done
        delim=""
        quoted=0
        while [ "$i" -lt "$n" ]; do
          c=${s:i:1}
          case $c in
            ' ' | $'\t' | $'\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>')
              break
              ;;
            "'" | '"')
              rest=${s:i+1}
              if [[ $rest != *"$c"* ]]; then
                return 1
              fi
              part=${rest%%"$c"*}
              delim+=$part
              quoted=1
              i=$((i + 2 + ${#part}))
              ;;
            $'\\')
              delim+=${s:i+1:1}
              quoted=1
              i=$((i + 2))
              ;;
            *)
              delim+=$c
              i=$((i + 1))
              ;;
          esac
        done
        if [ -z "$delim" ]; then
          return 1
        fi
        # Does the heredoc go into a shell (bash <<EOF)?
        shell_in=0
        rest=$cur
        while [ -n "$rest" ]; do
          part=${rest%%"$US"*}
          rest=${rest#*"$US"}
          case ${part##*/} in
            bash | sh | zsh | dash | ksh | mksh | ash) shell_in=1 ;;
          esac
        done
        hd_delim+=("$delim")
        hd_quoted+=("$quoted")
        hd_strip+=("$strip")
        hd_shell+=("$shell_in")
        continue
        ;;
      *)
        tok+=$c
        have=1
        ;;
    esac
    i=$((i + 1))
  done

  case $q in
    s | a | d) return 1 ;;
  esac
  if [ "${#st_kind[@]}" -gt 0 ]; then
    return 1
  fi
  if [ "$start_mode" = n ]; then
    lex_end_segment
  fi
  return 0
}

# ----------------------------------------------------------------- checks ---

# has_short WORD LETTER VALUE_LETTERS
# True if WORD is a short-option group (like -fdx) that has LETTER before any
# letter that takes a value (the rest of the word is then that value).
has_short() {
  local w=$1 want=$2 vals=$3 k ch
  if [[ $w != -[!-]* ]]; then
    return 1
  fi
  for ((k = 1; k < ${#w}; k++)); do
    ch=${w:k:1}
    if [ "$ch" = "$want" ]; then
      return 0
    fi
    if [ -n "$vals" ] && [[ $vals == *"$ch"* ]]; then
      return 1
    fi
  done
  return 1
}

# is_whole_tree PATH: true for a path that means the whole work tree.
is_whole_tree() {
  local p=$1
  while [[ $p == */ && $p != / ]]; do
    p=${p%/}
  done
  case $p in
    . | ./. | '*' | './*' | : | :/ | ':/*' | ':/.' | ':('* | .. | ../*) return 0 ;;
  esac
  return 1
}

check_extra() {
  local text=$1 pat
  for pat in "${EXTRA_PATTERNS[@]}"; do
    if [[ $text =~ $pat ]]; then
      block "the command matches the extra pattern '$pat' in the config"
    fi
  done
}

check_git_config() {
  local v=${1,,}
  case $v in
    alias.*) block "git -c alias.* defines an alias on the command line, which hides the real command" ;;
    core.hookspath*) block "git -c core.hooksPath changes the hooks, which has the same effect as --no-verify" ;;
    clean.requireforce*) block "git -c clean.requireForce lets git clean delete files without -f" ;;
  esac
}

# check_tag DST: block a push destination that is or can be a protected tag.
check_tag() {
  local name=${1#refs/tags/} t
  [ "${#PROTECTED_TAGS[@]}" -gt 0 ] || return 0
  if [[ $1 == refs/tags/* && $name == *'*'* ]]; then
    block "the push refspec \"$1\" can push every tag, a protected release tag too"
  fi
  for t in "${PROTECTED_TAGS[@]}"; do
    # shellcheck disable=SC2053  # the right side is a pattern on purpose
    if [[ $name == $t ]]; then
      block "git push of the protected tag \"$1\" (pattern '$t'); a maintainer cuts releases"
    fi
  done
}

check_push() {
  local -a r=("$@") pos=()
  local k=0 x p dst b endopts=0
  if [ "$POLICY" = strict ]; then
    block "the strict policy blocks every git push"
  fi
  while [ "$k" -lt "${#r[@]}" ]; do
    x=${r[k]}
    k=$((k + 1))
    if [ "$endopts" -eq 1 ]; then
      pos+=("$x")
      continue
    fi
    case $x in
      --) endopts=1 ;;
      --force | --force-with-lease | --force-with-lease=*)
        block "git push $x overwrites remote history" ;;
      --mirror | --all | --branches | --prune)
        block "git push $x can overwrite or delete remote branches, the default branch too" ;;
      --delete)
        block "git push --delete deletes a remote branch or tag" ;;
      --tags | --follow-tags)
        if [ "${#PROTECTED_TAGS[@]}" -gt 0 ]; then
          block "git push $x can push a protected release tag"
        fi
        ;;
      --repo | --receive-pack | --exec | --push-option)
        k=$((k + 1)) ;;
      --*) ;;
      -*)
        if has_short "$x" f o; then
          block "git push $x forces the update and overwrites remote history"
        fi
        if has_short "$x" d o; then
          block "git push $x deletes a remote branch or tag"
        fi
        if [[ $x =~ ^-[A-Za-z0-9]*o$ ]]; then
          k=$((k + 1))
        fi
        ;;
      [0-9]*'>'* | [0-9]*'<'* | '>'* | '<'*)
        if [[ $x =~ ^[0-9]*(\>|\>\>|\<|\>\|)$ ]]; then
          k=$((k + 1))
        fi
        ;;
      *) pos+=("$x") ;;
    esac
  done
  for p in "${pos[@]}"; do
    if [[ $p == *'$'* ]]; then
      block "the git push argument \"$p\" is built at run time, so the guard cannot check it"
    fi
  done
  for p in "${pos[@]:1}"; do
    case $p in
      +*) block "the push refspec \"$p\" starts with +, which forces the update" ;;
      :*) block "the push refspec \"$p\" starts with :, which deletes the remote branch" ;;
    esac
    dst=${p##*:}
    check_tag "$dst"
    dst=${dst#refs/heads/}
    for b in "${PROTECTED_BRANCHES[@]}"; do
      if [ "$dst" = "$b" ]; then
        block "git push to the protected branch \"$b\""
      fi
    done
  done
}

check_git() {
  local depth=$1
  shift
  local -a a=("$@") r=()
  local n=${#a[@]} j=0 o sub x k first endopts
  local hasD del force wt staged
  : "$depth"
  while [ "$j" -lt "$n" ]; do
    o=${a[j]}
    case $o in
      -c | --config-env)
        check_git_config "${a[j + 1]-}"
        j=$((j + 2))
        ;;
      --config-env=*)
        check_git_config "${o#--config-env=}"
        j=$((j + 1))
        ;;
      -C | --git-dir | --work-tree | --namespace | --super-prefix)
        j=$((j + 2))
        ;;
      -*)
        j=$((j + 1))
        ;;
      *) break ;;
    esac
  done
  if [ "$j" -ge "$n" ]; then
    check_extra "git"
    return 0
  fi
  sub=${a[j]}
  r=("${a[@]:j+1}")
  if [[ $sub == *'$'* ]]; then
    block "the git subcommand \"$sub\" is built at run time, so the guard cannot check it"
  fi
  if [ "${#r[@]}" -gt 0 ]; then
    check_extra "git $sub ${r[*]}"
  else
    check_extra "git $sub"
  fi
  for x in "${r[@]}"; do
    if [ "$x" = --no-verify ]; then
      block "--no-verify skips the repository's hooks"
    fi
  done

  case $sub in
    push)
      check_push "${r[@]}"
      ;;
    commit)
      for x in "${r[@]}"; do
        [ "$x" != -- ] || break
        if has_short "$x" n mFCctS; then
          block "git commit $x skips the repository's hooks (-n is --no-verify)"
        fi
      done
      ;;
    reset)
      for x in "${r[@]}"; do
        if [ "$x" = --hard ]; then
          block "git reset --hard throws away uncommitted work"
        fi
      done
      ;;
    clean)
      for x in "${r[@]}"; do
        [ "$x" != -- ] || break
        if [ "$x" = --force ] || has_short "$x" f e; then
          block "git clean $x deletes untracked files"
        fi
      done
      ;;
    branch)
      hasD=0 del=0 force=0
      for x in "${r[@]}"; do
        [ "$x" != -- ] || break
        if has_short "$x" D ""; then hasD=1; fi
        if [ "$x" = --delete ] || has_short "$x" d ""; then del=1; fi
        if [ "$x" = --force ] || has_short "$x" f ""; then force=1; fi
      done
      if [ "$hasD" -eq 1 ] || { [ "$del" -eq 1 ] && [ "$force" -eq 1 ]; }; then
        block "git branch -D deletes a branch even if it is not merged"
      fi
      ;;
    checkout | switch)
      endopts=0
      for x in "${r[@]}"; do
        if [ "$endopts" -eq 0 ]; then
          case $x in
            --) endopts=1; continue ;;
            --force | --discard-changes) block "git $sub $x throws away local changes" ;;
            -*)
              if has_short "$x" f bBcC; then
                block "git $sub $x throws away local changes"
              fi
              continue
              ;;
          esac
        fi
        if is_whole_tree "$x"; then
          block "git $sub with the path \"$x\" throws away changes in the whole work tree"
        fi
        if [ "$endopts" -eq 1 ] && [[ $x == *'$'* ]]; then
          block "the path \"$x\" is built at run time, so the guard cannot check it"
        fi
      done
      ;;
    restore)
      wt=0 staged=0 endopts=0
      local -a paths=()
      k=0
      while [ "$k" -lt "${#r[@]}" ]; do
        x=${r[k]}
        k=$((k + 1))
        if [ "$endopts" -eq 1 ]; then
          paths+=("$x")
          continue
        fi
        case $x in
          --) endopts=1 ;;
          --worktree) wt=1 ;;
          --staged) staged=1 ;;
          --source) k=$((k + 1)) ;;
          --*) ;;
          -*)
            if has_short "$x" W s; then wt=1; fi
            if has_short "$x" S s; then staged=1; fi
            if [[ $x =~ ^-[A-Za-z]*s$ ]]; then k=$((k + 1)); fi
            ;;
          *) paths+=("$x") ;;
        esac
      done
      if [ "$wt" -eq 1 ] || [ "$staged" -eq 0 ]; then
        for x in "${paths[@]}"; do
          if is_whole_tree "$x"; then
            block "git restore with the path \"$x\" throws away changes in the whole work tree"
          fi
          if [[ $x == *'$'* ]]; then
            block "the path \"$x\" is built at run time, so the guard cannot check it"
          fi
        done
      fi
      ;;
    stash | reflog)
      first=""
      for x in "${r[@]}"; do
        if [[ $x != -* ]]; then
          first=$x
          break
        fi
      done
      case "$sub $first" in
        "stash drop" | "stash clear") block "git stash $first deletes stashed work" ;;
        "reflog expire" | "reflog delete") block "git reflog $first deletes the history that lets you recover work" ;;
      esac
      ;;
    gc)
      for ((k = 0; k < ${#r[@]}; k++)); do
        if [ "${r[k]}" = --prune=now ] || { [ "${r[k]}" = --prune ] && [ "${r[k + 1]-}" = now ]; }; then
          block "git gc --prune=now deletes unreachable objects at once"
        fi
      done
      ;;
    update-ref)
      for x in "${r[@]}"; do
        if [ "$x" = --delete ] || has_short "$x" d m; then
          block "git update-ref -d deletes a ref"
        fi
      done
      ;;
    filter-branch | filter-repo)
      block "git $sub rewrites the history of the repository"
      ;;
  esac
}

check_shell() {
  local depth=$1
  shift
  local -a a=("$@")
  local k=0 x has_c=0
  while [ "$k" -lt "${#a[@]}" ]; do
    x=${a[k]}
    k=$((k + 1))
    case $x in
      --) break ;;
      -o | +o | --rcfile | --init-file) k=$((k + 1)) ;;
      --*) ;;
      -* | +*)
        if [[ $x == -*c* ]]; then has_c=1; fi
        ;;
      *)
        if [ "$has_c" -eq 1 ]; then
          check_line "$x" $((depth + 1))
        fi
        return 0
        ;;
    esac
  done
  if [ "$has_c" -eq 1 ] && [ "$k" -lt "${#a[@]}" ]; then
    check_line "${a[k]}" $((depth + 1))
  fi
  return 0
}

check_cmd() {
  local depth=$1
  shift
  local -a t=("$@") rest_words=()
  local n=${#t[@]} i=0 steps=0 w base vals
  while [ "$i" -lt "$n" ]; do
    steps=$((steps + 1))
    if [ "$steps" -gt "$MAX_PREFIX_WORDS" ]; then
      block "the command has too many prefix words to check"
    fi
    w=${t[i]}
    if [[ $w =~ ^[A-Za-z_][A-Za-z0-9_]*\+?= ]]; then
      i=$((i + 1))
      continue
    fi
    case $w in
      '!' | '{' | '}' | if | then | else | elif | do | while | until | '')
        i=$((i + 1))
        continue
        ;;
      sudo) vals=" -u -g -h -p -C -D -R -T -U -r -t " ;;
      doas) vals=" -u -C " ;;
      env) vals=" -u -C --unset --chdir " ;;
      nice) vals=" -n --adjustment " ;;
      timeout) vals=" -s -k --signal --kill-after " ;;
      xargs) vals=" -I -n -L -P -d -E -s -a " ;;
      exec) vals=" -a " ;;
      stdbuf) vals=" -i -o -e " ;;
      command | time | nohup | builtin) vals=" " ;;
      *) break ;;
    esac
    i=$((i + 1))
    while [ "$i" -lt "$n" ] && [[ ${t[i]} == -* ]]; do
      if [ "${t[i]}" = -- ]; then
        i=$((i + 1))
        break
      fi
      if [ "$w" = env ] && [[ ${t[i]} == -S || ${t[i]} == --split-string ]]; then
        rest_words=("${t[@]:i+2}")
        check_line "${t[i + 1]-} ${rest_words[*]}" $((depth + 1))
        return 0
      fi
      if [[ $vals == *" ${t[i]} "* ]]; then
        i=$((i + 2))
      else
        i=$((i + 1))
      fi
    done
    if [ "$w" = timeout ] && [ "$i" -lt "$n" ]; then
      i=$((i + 1))
    fi
  done
  if [ "$i" -ge "$n" ]; then
    return 0
  fi
  w=${t[i]}
  base=${w##*/}
  rest_words=("${t[@]:i+1}")
  case $base in
    git)
      check_git "$depth" "${rest_words[@]}"
      ;;
    bash | sh | zsh | dash | ksh | mksh | ash)
      check_shell "$depth" "${rest_words[@]}"
      check_extra "$base ${rest_words[*]}"
      ;;
    eval)
      check_line "${rest_words[*]}" $((depth + 1))
      ;;
    *)
      if [ "${#rest_words[@]}" -gt 0 ]; then
        check_extra "$base ${rest_words[*]}"
      else
        check_extra "$base"
      fi
      ;;
  esac
}

# check_line TEXT DEPTH: check every sub-command in a command line.
check_line() {
  local text=$1 depth=$2 seg rest
  local -a segs=() w=()
  if [ "$depth" -gt "$MAX_DEPTH" ]; then
    block "the command nests bash -c, eval or heredocs more than $MAX_DEPTH levels deep"
  fi
  SEGMENTS=()
  if ! lex "$text" n "$depth"; then
    block "the guard cannot parse the command (an unclosed quote, substitution or heredoc)"
  fi
  segs=("${SEGMENTS[@]}")
  for seg in "${segs[@]}"; do
    w=()
    rest=$seg
    while [ -n "$rest" ]; do
      w+=("${rest%%"$US"*}")
      rest=${rest#*"$US"}
    done
    check_cmd "$depth" "${w[@]}"
  done
}

# ------------------------------------------------------------------- main ---

input=""
IFS= read -r -d '' input || true

if ! command -v jq >/dev/null 2>&1; then
  block "jq is not installed, so the guard cannot read the command"
fi
if ! cmd=$(jq -er '.tool_input.command | strings' <<<"$input" 2>/dev/null); then
  block "the hook input is not valid JSON, or it has no .tool_input.command string"
fi
cmd=${cmd//$'\x1f'/ }

check_line "$cmd" 0
exit 0
