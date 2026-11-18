---
name: setup-pre-commit
description: Add a fast git pre-commit hook, in Oska a format and ShellCheck hook beside the pre-push check. Use when the user wants pre-commit hooks, wants to check formatting or scripts at commit time, or wants to set up Husky or lint-staged.
---

# setup-pre-commit

This skill adds a fast git `pre-commit` hook to a repo. The hook catches cheap mistakes before a commit. It does not build or test, because the slow check runs at push time or in CI.

Terms in this skill:

- **Hook**: a script that git runs at a fixed point. If a `pre-commit` hook fails, git does not make the commit.
- **Staged files**: the files in the git index, which the next commit will hold.
- **Check command**: the one command that a repo uses to check itself. In Oska it is `scripts/check.sh`.

## Oska or another repo

The repo is Oska when it has `crates/oskac/` and `scripts/check.sh`. Use steps 1 to 6 for Oska. For another repo, use "Other repos" at the end.

Oska already has `.githooks/pre-push`. It runs `scripts/check.sh`, or `scripts/check.sh --fast` when every pushed ref is a `ticket/**` branch. The pre-commit hook does not repeat that check. It runs only these:

- `cargo fmt --all --check` on the workspace.
- `shellcheck` on each staged `*.sh` file, when `shellcheck` is installed.

The skill ships both files:

- `scripts/pre-commit.sh`: the hook. It fails closed when `cargo` is missing. A missing `shellcheck` is only a note, because `scripts/check.sh` and CI run it.
- `scripts/test.sh`: its test suite. It runs the hook in temporary repos with stub `cargo` and `shellcheck`, so nothing builds.

## Steps

### 1. Read what exists

1. Read `CLAUDE.md`, `.githooks/pre-push`, and the "Check steps" section of `docs/agents/implementer.md`.
2. Run `git config core.hooksPath`.
3. List `.githooks/`. If `.githooks/pre-commit` exists, show it to the user and extend it. Do not replace it.

Done when: you know the value of `core.hooksPath`, and you know if a pre-commit hook exists.

### 2. Point git at `.githooks`

1. If `core.hooksPath` is `.githooks`, go to step 3.
2. If it is empty or has another value, ask the user before you change it. Then run `git config core.hooksPath .githooks`.

If the user is not present, do not change it. Write down that the hook is not active until the user runs the command.

Done when: `git config core.hooksPath` prints `.githooks`, or you wrote down why it does not.

### 3. Install the hook

1. Copy `scripts/pre-commit.sh` from this skill folder to `.githooks/pre-commit`.
2. Run `chmod +x .githooks/pre-commit`.
3. Read the copy. Make sure that it starts with `#!/usr/bin/env bash` and `set -euo pipefail`.
4. Add `.githooks/pre-commit` to the file list in `scripts/shellcheck/run.sh`, next to `.githooks/pre-push`.

Caution: a hook must not print environment variables. Do not add `env`, `printenv`, or `set -x` to it.

Done when: `.githooks/pre-commit` is executable, and `scripts/shellcheck/run.sh` names it.

### 4. Install the tests and a check step

1. Copy `scripts/test.sh` from this skill folder to `scripts/pre-commit/test.sh`. With no argument, it finds `.githooks/pre-commit` of its repo.
2. Add one step block to `scripts/check.sh`, near the other `scripts/*/test.sh` steps. Follow the "Check steps" rules of `docs/agents/implementer.md`:

   ```bash
   if step "pre-commit hook tests" --fast-when 'scripts/pre-commit/*' --fast-when .githooks/pre-commit; then
     bash scripts/pre-commit/test.sh
   fi
   ```

3. Run `scripts/check.sh --list`. Make sure that the new step is in the list.

Done when: `scripts/check.sh --list` shows "pre-commit hook tests".

### 5. Run the tests

1. Run `bash scripts/pre-commit/test.sh`.
2. Run `shellcheck .githooks/pre-commit scripts/pre-commit/test.sh` and `bash -n` on each file.
3. Run `bash scripts/check-steps/test.sh`. It proves that the new step is registered correctly.

If a test fails, read the output and fix the cause. Do at most 3 rounds. Then stop and report to the user.

Done when: the test suite prints `all N cases passed`, and ShellCheck and `check-steps/test.sh` pass.

### 6. Verify and report

1. Stage the new and changed files.
2. Run `.githooks/pre-commit` once by hand.
3. Run the full `scripts/check.sh` once before the commit, as `CLAUDE.md` says.
4. Tell the user which files changed, the test result, and that each clone runs `git config core.hooksPath .githooks` once.

Done when: the hook passed on the staged files, the full check passed, and you sent the report.

## Other repos

Keep this path short. Use the repo's own tools.

1. Find the repo's check command in the README, `CLAUDE.md`, `AGENTS.md`, the `Makefile`, or the CI config. If there is a pre-push hook or CI that runs it, the pre-commit hook runs only the fast format check of the language.
2. With a `package.json`, Husky and lint-staged are an option. Install them as dev dependencies at exact versions, with the lock file of the repo's package manager. Keep the existing Prettier config.
3. Without a `package.json`, write `.githooks/pre-commit` from `scripts/pre-commit.sh` and change its commands. Then run `git config core.hooksPath .githooks`, after you ask the user.
4. Copy `scripts/test.sh` and change its cases to match the new commands. Run it.
5. Add the one-time `core.hooksPath` step to the README.

Done when: the hook exists, it is executable, its tests pass, and a test commit ran it.

## Security

- **S5 Git safety.** Do not suggest `--no-verify` to get past a failed hook. Fix the cause. Do not weaken the hook or its tests to make them pass.
- **S9 Scripts fail closed.** The hook uses `set -euo pipefail`. A missing `cargo` refuses the commit.
- **S2 Secrets.** The hook does not print environment variables. Do not use `env`, `printenv`, or `set -x` in it. The test suite checks this with a planted value.
- **S3 Least privilege.** Read hooks and configs before you write them. Ask before you change `core.hooksPath`. Do not push.
- **S7 Supply chain.** In a JavaScript repo, pin each tool to an exact version, and commit the lock file.
