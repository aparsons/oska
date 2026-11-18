#!/usr/bin/env bash
# Tests for the pass stamp: the real .githooks/pre-push and the real
# stamp.sh, driven by real `git push`es from scratch clones to a scratch bare
# remote. Each scratch clone has a fake scripts/check.sh that records that it
# ran (and with which arguments), fails when FAKE_CHECK_MODE=fail, and
# otherwise writes the stamp the way the real check.sh does. The cases assert
# only on exit codes and on how the fake check ran. They never look at the
# clock.
#
#   scripts/pass-stamp/test.sh

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)

# This script may run inside a git hook, which exports variables that would
# point the scratch repositories' git commands at the real repository.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_COMMON_DIR GIT_PREFIX

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

passed=0
failures=0

ok() {
  printf 'ok   %s\n' "$1"
  passed=$((passed + 1))
}
bad() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures=$((failures + 1))
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid
export GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid

# The fake check: records a run, fails on request, and writes the stamp as
# the real check.sh does once every step has passed. FAKE_CHECK_TIER (full by
# default) is the tier it stamps. Each run logs one line, `ran` and its
# arguments. Like the real one, a --fast run writes no stamp.
# shellcheck disable=SC2016 # the fake check script is written out, not expanded here
fake_check='#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
checked_tree=$(scripts/pass-stamp/stamp.sh tree)
echo "ran${*:+ $*}" >>"$FAKE_CHECK_LOG"
if [ "${FAKE_CHECK_MODE:-pass}" = edit ]; then echo edit >>file; fi
if [ "${FAKE_CHECK_MODE:-pass}" = fail ]; then exit 1; fi
if [ "${1:-}" = --fast ]; then exit 0; fi
scripts/pass-stamp/stamp.sh write "${FAKE_CHECK_TIER:-full}" "$checked_tree"
'

# new_clone NAME: a bare remote and a clone of it, both under $work, with
# the real hook and stamp script and the fake check committed, and `main`
# pushed. Sets `clone`, `remote` and `log` (the file the fake check appends to).
new_clone() {
  local name=$1
  remote="$work/$name.git"
  clone="$work/$name"
  log="$work/$name.log"
  git init -q --bare -b main "$remote"
  git clone -q "$remote" "$clone" 2>/dev/null
  : >"$log"
  (
    cd "$clone"
    git checkout -q -b main
    mkdir -p .githooks scripts/pass-stamp
    cp "$repo_root/.githooks/pre-push" .githooks/pre-push
    cp "$repo_root/scripts/pass-stamp/stamp.sh" scripts/pass-stamp/stamp.sh
    printf '%s' "$fake_check" >scripts/check.sh
    chmod +x .githooks/pre-push scripts/check.sh scripts/pass-stamp/stamp.sh
    echo one >file
    git config core.hooksPath .githooks
    git add -A
    git commit -q -m initial
    git push -q --no-verify origin main 2>/dev/null
  )
}

# commit_change TEXT: commits a change to `file` in the current clone.
commit_change() {
  (cd "$clone" && echo "$1" >file && git add -A && git commit -q -m "$1")
}

# run_check: what a developer or agent does: runs the check by hand.
run_check() {
  (cd "$clone" && FAKE_CHECK_LOG=$log scripts/check.sh) >/dev/null 2>&1
}

# push ARGS...: runs `git push ARGS` in the current clone with the real hook.
# Sets `status`, `ran` (how many times the fake check ran) and `how` (its
# arguments, one line per run: empty for the full check).
push() {
  : >"$log"
  set +e
  (cd "$clone" && FAKE_CHECK_LOG=$log git push -q "$@") >/dev/null 2>&1
  status=$?
  set -e
  ran=$(wc -l <"$log" | tr -d ' ')
  how=$(sed 's/^ran *//' "$log")
}

expect() { # name expected_status expected_ran
  if [ "$status" -ne "$2" ]; then
    bad "$1" "push exit status $status, expected $2"
  elif [ "$ran" -ne "$3" ]; then
    bad "$1" "check ran $ran time(s), expected $3"
  else
    ok "$1"
  fi
}

stamp_exists() { (cd "$clone" && [ -e "$(git rev-parse --git-path oska-check-passed)" ]); }

# --- an unstamped push runs the check, and the passing check stamps ---------
new_clone unstamped
commit_change two
push origin main
expect "no stamp: the push runs the check and succeeds" 0 1
if stamp_exists; then ok "a passing check writes the stamp"; else bad "a passing check writes the stamp" "no stamp file"; fi
if [ -z "$(cd "$clone" && git status --porcelain)" ]; then ok "the stamp is not in the working tree"; else bad "the stamp is not in the working tree" "git status is not clean"; fi
commit_change three
push origin main
expect "a stamp of another tree: the push runs the check" 0 1

# --- stamped, clean, matching tree: the push skips the check ----------------
new_clone match
commit_change two
run_check
push origin main
expect "stamped clean matching tree: the check is skipped" 0 0
if [ "$(git --git-dir="$remote" rev-parse refs/heads/main)" = "$(cd "$clone" && git rev-parse HEAD)" ]; then
  ok "the skipped push still pushes"
else
  bad "the skipped push still pushes" "the remote tip is not the pushed commit"
fi

# --- checked before committing, then committed: the push skips --------------
new_clone precommit
(cd "$clone" && echo two >file && echo new >untracked)
run_check
(cd "$clone" && git add -A && git commit -q -m two)
push origin main
expect "checked before committing: the check is skipped" 0 0

# --- racy git: an edit in the same instant as the last index write ----------
# Git trusts an index entry whose size and mtime match the file, unless the
# entry is as new as the index file itself ("racily clean"), when it rehashes.
# Pinning every mtime to one instant makes that case certain here: `file`
# changes to a same-size text with the mtime the index recorded. On a fast
# machine it happens by chance, as on CI for the case above.
new_clone racy
(
  cd "$clone"
  t=@1700000000
  touch -d "$t" file
  git add file
  git commit -q --allow-empty -m pinned
  git push -q --no-verify origin main 2>/dev/null
  touch -d "$t" "$(git rev-parse --git-path index)"
  echo two >file
  touch -d "$t" file
)
run_check
(cd "$clone" && git add -A && git commit -q -m two)
push origin main
expect "checked a racily clean edit before committing: the check is skipped" 0 0

# --- a different tree: runs -------------------------------------------------
new_clone different
commit_change two
run_check
commit_change three
push origin main
expect "stamped tree differs from the pushed tree: the check runs" 0 1

# --- a dirty working tree: runs ---------------------------------------------
new_clone dirty
commit_change two
run_check
echo edit >>"$clone/file"
push origin main
expect "modified tracked file: the check runs" 0 1

new_clone untracked
commit_change two
run_check
echo new >"$clone/untracked"
push origin main
expect "untracked file: the check runs" 0 1

new_clone status-fails
commit_change two
run_check
(cd "$clone" && echo garbage >"$(git rev-parse --git-path index)")
push origin main
expect "git status fails (a corrupt index): the check runs" 0 1

# --- a failed check: no stamp, push refused ---------------------------------
new_clone failing
commit_change two
if (cd "$clone" && FAKE_CHECK_MODE=fail FAKE_CHECK_LOG=$log scripts/check.sh) >/dev/null 2>&1; then
  bad "a failing check exits non-zero" "exited 0"
else
  ok "a failing check exits non-zero"
fi
if stamp_exists; then bad "a failed check writes no stamp" "stamp file exists"; else ok "a failed check writes no stamp"; fi
FAKE_CHECK_MODE=fail push origin main
expect "a failing check refuses the push" 1 1
if stamp_exists; then bad "a failed push-time check writes no stamp" "stamp file exists"; else ok "a failed push-time check writes no stamp"; fi
if git --git-dir="$remote" log -1 --format=%s main | grep -qx two; then
  bad "the refused push pushes nothing" "the remote has the commit"
else
  ok "the refused push pushes nothing"
fi
push origin main
expect "after a failed check the next push runs the check" 0 1

# --- an edit while the check runs: no stamp ---------------------------------
new_clone edited-during
commit_change two
(cd "$clone" && FAKE_CHECK_MODE=edit FAKE_CHECK_LOG=$log scripts/check.sh) >/dev/null 2>&1
if stamp_exists; then bad "a check during which a file changed writes no stamp" "stamp file exists"; else ok "a check during which a file changed writes no stamp"; fi
(cd "$clone" && git checkout -q -- file)
push origin main
expect "after an edit during the check the next push runs the check" 0 1

new_clone edited-then-committed
commit_change two
(cd "$clone" && FAKE_CHECK_MODE=edit FAKE_CHECK_LOG=$log scripts/check.sh) >/dev/null 2>&1
(cd "$clone" && git commit -q -a -m edited)
push origin main
expect "the edited tree was never stamped: the push runs the check" 0 1

# --- several commits ---------------------------------------------------------
new_clone several
commit_change two
commit_change three
commit_change four
run_check
push origin main
expect "several commits, the tip stamped: the check is skipped" 0 0

new_clone several-tip-unstamped
commit_change two
run_check
commit_change three
commit_change four
push origin main
expect "several commits, an earlier one stamped but not the tip: the check runs" 0 1

new_clone merged
(cd "$clone" && git checkout -q -b topic)
commit_change two
(cd "$clone" && git push -q --no-verify origin topic 2>/dev/null)
(
  cd "$clone"
  git checkout -q main
  echo other >other
  git add other
  git commit -q -m other
  git push -q --no-verify origin main 2>/dev/null
  git checkout -q topic
  git merge -q --no-edit main
)
commit_change three
run_check
push origin topic
expect "a merge and a commit on top, the tip stamped: the check is skipped" 0 0

new_clone worktree
(cd "$clone" && git worktree add -q -b topic "$work/worktree-wt" 2>/dev/null)
main_clone=$clone
clone=$work/worktree-wt
commit_change two
commit_change three
run_check
push origin topic
expect "a worktree, several commits, the tip stamped: the check is skipped" 0 0
if (cd "$main_clone" && [ ! -e "$(git rev-parse --git-path oska-check-passed)" ]); then
  ok "a worktree's stamp is its own"
else
  bad "a worktree's stamp is its own" "the main clone has a stamp"
fi
commit_change four
push origin topic
expect "a worktree, a new unchecked commit: the check runs" 0 1

new_clone by-url
commit_change two
commit_change three
run_check
push "$remote" main
expect "pushing to a URL, the tip stamped: the check is skipped" 0 0

new_clone same-tree
commit_change two
(cd "$clone" && git commit -q --allow-empty -m empty-one && git commit -q --allow-empty -m empty-two)
run_check
push origin main
expect "several commits all with the stamped tree: the check is skipped" 0 0

# --- a dirty tree names its files and says to move logs out ------------------
new_clone dirty-message
commit_change two
run_check
echo log >"$clone/check-t1.log"
dirty_out=$(cd "$clone" && scripts/pass-stamp/stamp.sh covers origin </dev/null 2>&1 || true)
if grep -q 'check-t1.log' <<<"$dirty_out" && grep -q 'outside the worktree' <<<"$dirty_out"; then
  ok "an ignored stamp names the untracked file and says to move logs outside"
else
  bad "an ignored stamp names the untracked file and says to move logs outside" "$dirty_out"
fi
if (cd "$clone" && scripts/pass-stamp/stamp.sh covers origin </dev/null) >/dev/null 2>&1; then
  bad "a dirty tree still declines the stamp" "covers exited 0"
else
  ok "a dirty tree still declines the stamp"
fi
dirty_tree=$(cd "$clone" && scripts/pass-stamp/stamp.sh tree)
echo more >"$clone/check-t2.log"
write_out=$(cd "$clone" && scripts/pass-stamp/stamp.sh write full "$dirty_tree" 2>&1)
if grep -q 'check-t2.log' <<<"$write_out" && grep -q 'outside the worktree' <<<"$write_out"; then
  ok "a declined stamp write names the file and says to move logs outside"
else
  bad "a declined stamp write names the file and says to move logs outside" "$write_out"
fi
# Every listed file is indented alike, and a cut list says how many it left out.
for i in $(seq 1 22); do echo log >"$clone/many-$i.log"; done
many_out=$(cd "$clone" && scripts/pass-stamp/stamp.sh covers origin </dev/null 2>&1 || true)
listed=$(grep -c '^    ?? ' <<<"$many_out" || true)
if [ "$listed" -eq 20 ] && grep -qx '    \.\.\. and 4 more (24 in all)' <<<"$many_out"; then
  ok "a long dirty list indents every file and says how many it cut"
else
  bad "a long dirty list indents every file and says how many it cut" "$many_out"
fi

# --- a deleted branch --------------------------------------------------------
# A deletion pushes no commits, so a stamped, clean tree covers it. Without a
# stamp the hook runs the check as before.
new_clone delete
(cd "$clone" && git push -q --no-verify origin main:refs/heads/topic 2>/dev/null)
run_check
push origin --delete topic
expect "deleting a branch, stamped and clean: the check is skipped" 0 0
if git --git-dir="$remote" rev-parse --verify -q refs/heads/topic >/dev/null; then
  bad "the branch is deleted" "still on the remote"
else
  ok "the branch is deleted"
fi
(cd "$clone" && git push -q --no-verify origin main:refs/heads/topic 2>/dev/null)
echo edit >>"$clone/file"
push origin --delete topic
expect "deleting a branch from a dirty tree: the check runs" 0 1

new_clone delete-unstamped
(cd "$clone" && git push -q --no-verify origin main:refs/heads/topic 2>/dev/null)
push origin --delete topic
expect "deleting a branch with no stamp: the check runs" 0 1

# --- a new branch -------------------------------------------------------------
new_clone new-branch
(cd "$clone" && git checkout -q -b topic)
commit_change two
run_check
push origin topic
expect "new branch, stamped tip: only its own commits count, the check is skipped" 0 0

new_clone new-branch-two-commits
(cd "$clone" && git checkout -q -b topic)
commit_change two
commit_change three
run_check
push origin topic
expect "new branch with two commits, the tip stamped: the check is skipped" 0 0

new_clone new-branch-empty
(cd "$clone" && git checkout -q -b topic)
run_check
push origin topic
expect "new branch with no new commits, stamped: the check is skipped" 0 0

# --- refs with no new commits still need the stamped tree at their tip ------
new_clone old-commit
commit_change two
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null)
commit_change three
run_check
push origin HEAD~1:refs/heads/topic
expect "an older, unchecked commit pushed to a new branch: the check runs" 0 1

new_clone rewind
commit_change two
commit_change three
run_check
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null)
push -f origin HEAD~2:main
expect "a forced rewind of main to an unchecked commit: the check runs" 0 1
if [ "$(git --git-dir="$remote" log -1 --format=%s main)" = initial ]; then
  ok "the rewind still pushes once the check passes"
else
  bad "the rewind still pushes once the check passes" "the remote main is not the rewound commit"
fi

new_clone rewind-checked
commit_change two
run_check
commit_change three
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null)
push -f origin HEAD~1:main
expect "a forced rewind to the stamped tree: the check is skipped" 0 0

# --- tags: the tagged commit's tree must be the stamped one ------------------
new_clone tag-unchecked
commit_change two
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null && git tag -a -m v1 v1 HEAD~1)
run_check
push origin v1
expect "an annotated tag on an unchecked, pushed commit: the check runs" 0 1

new_clone tag-light-unchecked
commit_change two
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null && git tag v1 HEAD~1)
run_check
push origin v1
expect "a lightweight tag on an unchecked, pushed commit: the check runs" 0 1

new_clone tag-checked
commit_change two
run_check
(cd "$clone" && git push -q --no-verify origin main 2>/dev/null && git tag -a -m v1 v1)
push origin v1
expect "an annotated tag on the stamped tree: the check is skipped" 0 0

new_clone tag-with-branch
commit_change two
(cd "$clone" && git tag -a -m v1 v1 HEAD~1)
run_check
push origin main v1
expect "a checked branch and a tag on an unchecked commit: the check runs" 0 1

new_clone delete-with-push
(cd "$clone" && git push -q --no-verify origin main:refs/heads/topic 2>/dev/null)
commit_change two
run_check
push origin main :topic
expect "a stamped push and a deletion together: the check is skipped" 0 0

# --- only a full run's stamp counts ------------------------------------------
new_clone fast-stamp
commit_change two
(cd "$clone" && FAKE_CHECK_TIER=fast FAKE_CHECK_LOG=$log scripts/check.sh) >/dev/null 2>&1
if stamp_exists; then ok "a fast run writes a stamp"; else bad "a fast run writes a stamp" "no stamp file"; fi
push origin main
expect "a fast run's stamp on the pushed tree: the check runs" 0 1

new_clone bare-hash-stamp
commit_change two
(cd "$clone" && git rev-parse 'HEAD^{tree}' >"$(git rev-parse --git-path oska-check-passed)")
push origin main
expect "a stamp with no tier (the old form): the check runs" 0 1

new_clone unknown-tier
commit_change two
(cd "$clone" && scripts/pass-stamp/stamp.sh write partial "$(scripts/pass-stamp/stamp.sh tree)") >/dev/null
if stamp_exists; then bad "an unknown tier writes no stamp" "stamp file exists"; else ok "an unknown tier writes no stamp"; fi

# --- ticket/** branches get the fast check, every other ref the full one ---
# The hook picks by the remote ref each line pushes to. Only a push whose
# refs are all ticket/** branches runs `check.sh --fast`; CI runs the full
# check on those branches before anything merges.
expect_tier() { # name expected_status tier(full|fast)
  local want=
  [ "$3" = fast ] && want=--fast
  if [ "$status" -ne "$2" ]; then
    bad "$1" "push exit status $status, expected $2"
  elif [ "$ran" -ne 1 ]; then
    bad "$1" "check ran $ran time(s), expected 1"
  elif [ "$how" != "$want" ]; then
    bad "$1" "check ran with '$how', expected '$want'"
  else
    ok "$1"
  fi
}

new_clone ticket
(cd "$clone" && git checkout -q -b ticket/1-x)
commit_change two
push origin ticket/1-x
expect_tier "a ticket/** branch: the fast check runs" 0 fast
if [ "$(git --git-dir="$remote" rev-parse refs/heads/ticket/1-x)" = "$(cd "$clone" && git rev-parse HEAD)" ]; then
  ok "the fast-checked ticket push pushes"
else
  bad "the fast-checked ticket push pushes" "the remote has no ticket/1-x at the tip"
fi
if stamp_exists; then bad "a fast hook run writes no stamp" "stamp file exists"; else ok "a fast hook run writes no stamp"; fi

commit_change three
(cd "$clone" && FAKE_CHECK_MODE=fail git push -q origin ticket/1-x) >/dev/null 2>&1 && st=0 || st=$?
if [ "$st" -ne 0 ] && [ "$(git --git-dir="$remote" log -1 --format=%s ticket/1-x)" = two ]; then
  ok "a failing fast check refuses the ticket push"
else
  bad "a failing fast check refuses the ticket push" "exit $st"
fi

push origin ticket/1-x:refs/heads/ticket/2-y ticket/1-x:refs/heads/ticket/3-z
expect_tier "two ticket/** branches in one push: one fast check" 0 fast

push origin ticket/1-x:refs/heads/ticket/sub/4-w
expect_tier "a nested ticket/** branch: the fast check runs" 0 fast

push origin --delete ticket/2-y
expect_tier "deleting a ticket/** branch, no stamp: the fast check runs" 0 fast

push origin ticket/1-x ticket/1-x:main
expect_tier "a ticket/** branch and main together: the full check runs" 0 full

new_clone ticket-to-main
(cd "$clone" && git checkout -q -b ticket/1-x)
commit_change two
push origin ticket/1-x:main
expect_tier "a local ticket/** branch pushed to main: the full check runs" 0 full

new_clone other-refs
commit_change two
push origin main
expect_tier "main: the full check runs" 0 full
commit_change three
push origin HEAD:refs/heads/spec/5-s
expect_tier "a spec/** branch: the full check runs" 0 full
commit_change four
push origin HEAD:refs/heads/topic
expect_tier "another branch: the full check runs" 0 full
commit_change five
push origin HEAD:refs/heads/tickets/6
expect_tier "a branch that only starts with ticket: the full check runs" 0 full
commit_change six
(cd "$clone" && git tag -a -m ticket/7 ticket/7)
push origin refs/tags/ticket/7
expect_tier "a tag named ticket/**: the full check runs" 0 full
commit_change seven
(cd "$clone" && git tag v1)
push origin v1
expect_tier "a tag: the full check runs" 0 full

new_clone ticket-stamped
(cd "$clone" && git checkout -q -b ticket/1-x)
commit_change two
run_check
push origin ticket/1-x
expect "a ticket/** branch with a full stamp on its tip: the check is skipped" 0 0

printf '\n%d passed, %d failed\n' "$passed" "$failures"
[ "$failures" -eq 0 ]
