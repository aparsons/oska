# Implementer brief

The fast inner loop for an agent implementing a ticket. The machine is shared and CPU-bound, so every wasted build slows every agent.

## The loop

1. `cargo fmt` first.
2. While iterating, run only the changed crate's tests: `cargo test -p <crate>`, or one `--test <name>`. Do not run `scripts/check.sh` per edit.
3. Run the full `scripts/check.sh` once, right before committing.
4. Commit, then `git push`. Never `--no-verify`: see the pass stamp.

## Lean cargo environment

Export it for every cargo command (otherwise builds waste CPU and disk on debug info):

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
```

## Logs and waiting

- Name every log and result file per ticket (`check-t286.log`), so parallel agents never overwrite each other's.
- Run a long check detached and wait on a file or a PID, not on a process name: `setsid bash -c 'scripts/check.sh > LOG 2>&1; echo $? > RC' < /dev/null`, then wait until `RC` exists.
- A `pgrep -f` wait loop matches its own command line and never ends. Do not use one.
- Report once, when the work is done. No progress reports.

## Never kill by name

Other agents' checks run on this machine. `pkill -f`, `pkill` by name or session, `killall`, and `kill $(pgrep ...)` can end their checks (they show up as exit 144) and cost a 15 to 25 minute rerun.

- A kill guard enforces it: a `PreToolUse` hook on Bash in `.claude/settings.json` runs `scripts/kill-guard/guard.sh`, which refuses those commands (exit 2, with a message).
- Kill only a PID your own command started: `cmd & pid=$!; ...; kill "$pid"`. `pkill -P $$` (your own shell's children) is allowed.
- The guard reads command text; it stops accidents, not determined bypasses. Do not try to get around it.

## Pass stamp

When every step of `scripts/check.sh` passes, it writes the tree hash it checked to `oska-check-passed` in the clone's git directory (never committed), but only if the working tree did not change during the run. Each worktree has its own stamp. The `.githooks/pre-push` hook skips its check when the working tree is clean and the tip of every pushed ref with new commits has the stamped tree, so a multi-commit push (a merge of `origin/main` included) skips when its tip was checked. Otherwise it runs `scripts/check.sh` as before.

- Commit what you checked, then push with no edits in between. Any edit, or a merge that changes the tree, needs a new check.
- Enable the hook once per clone: `git config core.hooksPath .githooks`.
- Never `--no-verify`: the stamp makes it unnecessary.

## Check steps

`scripts/check.sh --list` prints the check's steps in order and runs nothing. Each step is one self-contained `if step "NAME"; then ... fi` block in `scripts/check.sh`; to add a step, add one block where it should run. Tag it after the name: `--fast` also puts it in the fast tier, and `--no-slot` runs a cheap step without a build slot. A step whose tool is missing calls `skip_step "REASON"`. There is no other list to update. `scripts/check-steps/test.sh` proves that `--list` matches what a run prints and that every `scripts/*/test.sh` runs.

## Build slots

`OSKA_BUILD_SLOTS=N` makes `scripts/check.sh` wait for one of N machine-wide slots (lock files under `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp`) before its build and test steps. It prints one "waiting for a free build slot" line; that is not a hang. The kernel frees the slot when the check ends, however it ends. Unset means no limit (CI, other clones). On the Pi use `OSKA_BUILD_SLOTS=2`, exported with the lean environment. Only `scripts/check.sh` takes a slot; the cargo commands you run while iterating are not limited.

## Portable tests

Any test that builds paths, puts a path in JSON, or sets a home uses `crates/oskac/tests/common/portable.rs` (`Scratch`, `json_str`, `json_path`, `set_home`, `literal_path`), so it passes on Windows the first time. See `CODING_STANDARDS.md` (Tests).

## CI on ticket branches

A push to `ticket/**` or `review-fixes-*` runs `ci.yml`, `windows.yml` and `confinement.yml`, so every platform verifies a ticket before the spec's pull request. Name ticket branches `ticket/<number>-<slug>`.

## Reading CI failures

- Find a failed job: `gh pr checks <pr> --json name,bucket,link` (bucket `fail` marks failures). The job id is the last number in the link.
- Read its log: `gh api repos/aparsons/oska/actions/jobs/<id>/logs --allow-escape-sequences`. Find the failing step with `| grep -nE '##\[error\]|FAIL|panicked'` and read around those lines, not the whole log.
- In your report, list everything not run locally: PowerShell, Windows and macOS, and say whether CI passed on each.
