---
name: git-guardrails
description: Install a Claude Code PreToolUse hook that blocks destructive git commands before they run. Use to stop an agent from force-pushes, hard resets, branch deletes, or pushes to main or to release tags.
---

# git-guardrails

This skill installs a guard script as a Claude Code `PreToolUse` hook on the Bash tool. Before each Bash call, the hook reads the command text. If the text has a destructive git command, the hook blocks the call (exit 2) and tells the agent why.

The guard is a safety net against mistakes. It is not a security boundary against a hostile agent. A determined process can get past a text filter. For example, it can use a script file, or build the command at run time. Server-side branch protection is the real control. Recommend it to the user in step 7.

## What the guard blocks

The shipped script `scripts/block-dangerous-git.sh` blocks these in every policy:

- `git push` with `--force`, `-f`, `--force-with-lease`, `--mirror`, `--all`, `--prune`, `--delete`, or `-d`.
- `git push` with a refspec that starts with `+` (force) or `:` (delete).
- `git push` to a protected branch. The default list is `main` and `master`.
- `git push` of a protected tag, and `git push --tags` or `--follow-tags`. The default pattern is `v[0-9]*`, the release tags.
- `git reset --hard`.
- `git clean` with `-f` or `--force`.
- `git branch -D`, and `git branch --delete --force`.
- `git checkout .`, `git checkout -- .`, and `git checkout -f`.
- `git restore .` and `git restore --staged --worktree .` with a whole-tree path.
- `git stash drop` and `git stash clear`.
- `git reflog expire`.
- `git gc --prune=now`.
- `git update-ref -d`.
- `git filter-branch` and `git filter-repo`.
- Any git command with `--no-verify`, `git commit -n`, and `git -c core.hooksPath=...`.
- `git -c alias.<name>=...`, because an alias on the command line hides the real command.

The "strict" policy also blocks every `git push`.

The guard reads the command line like a shell does, but without running it:

- It checks each sub-command after `;`, `&&`, `||`, `|`, `&`, and newlines.
- It checks the text inside `$( )` and backticks, also inside double quotes.
- It removes prefixes before the command word: `FOO=1`, `env`, `command`, `exec`, `sudo`, and others.
- It reads `git` with a path prefix, such as `/usr/bin/git`.
- It skips git global options such as `-C <dir>`, `-c <k=v>`, `--git-dir=...`, and `--no-pager`.
- It checks the script of `bash -c`, `sh -c`, and `eval`, up to 3 levels deep.

Quoted text is one word, not a command. So the guard allows `git commit -m "reset --hard docs"`. Text inside `$( )` is a command, also when it is in quotes.

The guard does not run git. Thus it cannot see the current branch. A plain `git push` from `main` gets past it. Branch protection on the server stops that case.

The guard fails closed. It blocks the call when `jq` is missing, when the JSON is not valid, or when it cannot parse the command. It needs bash 4.4 or later. On macOS, the system bash is 3.2, so install a newer bash first.

## In Oska

Oska (the repo has `crates/oskac/` and `scripts/check.sh`) already has a `PreToolUse` hook in `.claude/settings.json`: the kill guard, `scripts/kill-guard/guard.sh`. Install the git guard beside it. Do not replace it or change it.

- Add the git guard as a second entry in the `hooks` list of the existing `Bash` matcher.
- Keep the default `PROTECTED_TAGS=('v[0-9]*')`. Oska releases are signed, and only the maintainer pushes a `v*` tag. See `docs/agents/release.md`.
- Keep `main` in `PROTECTED_BRANCHES`. Agents push `ticket/<n>-<slug>` and `spec/<n>-<slug>` branches, which the standard policy allows.
- The guard blocks `--no-verify`. This agrees with `CLAUDE.md`: the `.githooks/pre-push` hook runs `scripts/check.sh`.

## Steps

### 1. Ask for the scope

Ask the user where to install the guard:

- **This project** (recommended): the settings file is `.claude/settings.json`, and the script goes in `.claude/hooks/`.
- **All projects**: the settings file is `~/.claude/settings.json`, and the script goes in `~/.claude/hooks/`.

If the user is not present, use "this project" and write down the assumption. In Oska, always use "this project".

Done when: you know the settings file and the hooks folder.

### 2. Ask for the policy

Ask the user which policy to use:

- **standard** (default): allow a normal push of a feature branch, and block the list above.
- **strict**: block the list above and every `git push`.

Ask which branch is the default branch of each remote. If it is not `main` or `master`, add it to the protected list in step 5.

Ask if an agent may push release tags. If not, keep the default `PROTECTED_TAGS`. If the release tags use another name, change the pattern in step 5.

Done when: you know the policy, the protected branches, and the protected tags.

### 3. Copy the script

1. Make the hooks folder with `mkdir -p`.
2. Copy `scripts/block-dangerous-git.sh` from this skill folder to the hooks folder.
3. Run `chmod +x` on the copy.
4. If the policy is strict, set `POLICY="strict"` in the config block of the copy.

Make sure that `jq` is on the `PATH`. If it is not, tell the user to install it. Without `jq`, the guard blocks every Bash call.

Done when: the copy exists, it is executable, and its `POLICY` line has the chosen value.

### 4. Add the hook to the settings file

Caution: the settings file can hold other hooks and permissions. Read it first. Keep every other setting and every existing hook. In Oska, keep the kill-guard entry first and unchanged.

1. Read the settings file. If it does not exist, start from `{}`.
2. Find the `hooks.PreToolUse` array. If an entry with matcher `Bash` exists, add the new hook to its `hooks` list. If not, add a new entry.
3. Use this hook command for the project scope. The quotes keep a path with spaces in one word.

   ```json
   {
     "matcher": "Bash",
     "hooks": [
       {
         "type": "command",
         "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/block-dangerous-git.sh"
       }
     ]
   }
   ```

4. For the all-projects scope, use `"$HOME"/.claude/hooks/block-dangerous-git.sh` as the command.
5. Do not add the hook twice. If the same command is already there, keep one copy.
6. Write the file. Make sure that it is valid JSON with `jq empty <file>`.

Done when: `jq empty` passes, the new hook is in the file once, and all earlier settings and hooks are still there.

### 5. Change the patterns

Ask the user if they want to add or remove rules.

- To protect more branches, edit `PROTECTED_BRANCHES` in the config block of the copy.
- To change the protected tags, edit `PROTECTED_TAGS`. Each entry is a bash glob pattern. `PROTECTED_TAGS=()` allows every tag push.
- To block more commands, add entries to `EXTRA_PATTERNS`. Each entry is a bash extended regular expression. The guard matches it against each sub-command in a normal form, for example `git rebase -i main`.
- To remove a built-in rule, edit the check in the copy. Tell the user which rule you removed.

Edit only the copy in the hooks folder. Keep the copy in this skill folder as the reference.

If the user is not present, keep the defaults and write down the assumption.

Done when: the config block of the copy has the user's choices.

### 6. Test the installed guard

Run the shipped test script on the installed copy:

```bash
bash <skill-folder>/scripts/test.sh <hooks-folder>/block-dangerous-git.sh
```

The test script makes copies with each policy, so the installed policy does not change the results. If an extra pattern blocks an "allowed" case on purpose, that case fails. Tell the user which case it is.

If a case fails for another reason, fix the copy and run the tests again. Do at most 3 rounds. Then stop and report the failures to the user.

Done when: the test script prints `all N cases passed`, or the user accepts each failure that an extra pattern causes.

### 7. Report

Tell the user:

1. Where the script and the settings change are.
2. The policy, the protected branches, and the protected tags.
3. The test result.
4. That the hook starts in new sessions. Some clients need a restart, or a check in the `/hooks` menu.
5. That the guard is a safety net, not a security boundary. Recommend branch protection on the server: no force-push and no direct push to the default branch.

Done when: you sent the report.

## Security

- **Not a boundary.** The guard stops mistakes, not a hostile process. Recommend server-side branch protection as the real control.
- **Other hooks.** Keep every existing hook, such as Oska's kill guard. Add the git guard beside it.
- **S3 Least privilege.** Read the settings file before you write it. Ask before you change `~/.claude/settings.json`, because it applies to all projects.
- **S5 Git safety.** Do not weaken a rule or skip a test to make the test script pass. Fix the cause, or ask the user.
- **S9 Fail closed.** The guard uses `set -euo pipefail`. A missing `jq`, bad JSON, or text that it cannot parse gives a block, not an allow. Keep this behaviour when you edit the copy.
- **Blocked command.** When the guard blocks a command, do not try another way to run it. Ask the user to run it, or to change the config.
