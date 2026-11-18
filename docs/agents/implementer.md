# Implementer brief

The fast inner loop for an agent implementing a ticket. The machine is shared and CPU-bound, so every wasted build slows every agent.

## The loop

1. `cargo fmt` first.
2. While iterating, run only the changed crate's tests: `cargo test -p <crate>`, or one `--test <name>`. Do not run the full `scripts/check.sh` per edit.
3. Push early: as soon as the ticket's tests exist, commit and push the ticket branch (`ticket/<number>-<slug>`), so CI (Windows and macOS included) runs alongside your local work. Push again as you go. Never `--no-verify`.
4. Run the full `scripts/check.sh` once, right before the final commit, then push.

## Fast check

`scripts/check.sh --fast` runs `cargo fmt`, `cargo clippy`, `cargo test -p` for only the crates changed since the merge-base with `origin/main` and every crate that depends on them, in one `cargo test` (the full step's `cargo test --all` when that would be half the workspace or more, or when a cargo input outside the crates changed, such as `Cargo.toml`; a docs-only change runs just the tests that read `docs/`, as the full check does), and the test suites of only the `scripts/` directories changed (plus shellcheck when a shell script changed). When it cannot tell what changed (no `origin/main`, or a shallow clone) it runs every step. The rule: the fast tier is never slower than the full check for the same change; a step that would be belongs in the full check only. It ends by saying it is NOT the full check, and it never writes a pass stamp. Like the full check, it checks the working tree, not the commit a push sends: commit what you checked.

The pre-push hook runs `--fast` for a push whose refs are all `ticket/**` branches. A push that includes anything else (`main`, `spec/**`, a tag, another branch) runs the full check. A fast pass does not make a ticket ready to merge: CI's full check on the ticket branch is the gate, and a ticket PR merges only when CI is green.

## Lean cargo environment

Export it for every cargo command (otherwise builds waste CPU and disk on debug info):

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
```

## Logs and waiting

- Put logs and result files outside the worktree, e.g. `LOG=${TMPDIR:-/tmp}/check-t286.log RC=${TMPDIR:-/tmp}/check-t286.rc`. A file inside the worktree makes the tree dirty, so the check writes no pass stamp and the pre-push hook runs the whole check again.
- Name every log and result file per ticket (`check-t286.log`), so parallel agents never overwrite each other's.
- Run a long check detached and wait on a file or a PID, not on a process name: `setsid bash -c 'scripts/check.sh > "$LOG" 2>&1; echo $? > "$RC"' < /dev/null` (export `LOG` and `RC` first), then wait until `$RC` exists.
- A `pgrep -f` wait loop matches its own command line and never ends. Do not use one.
- Report once, when the work is done. No progress reports.

## Never kill by name

Other agents' checks run on this machine. `pkill -f`, `pkill` by name or session, `killall`, and `kill $(pgrep ...)` can end their checks (they show up as exit 144) and cost a 15 to 25 minute rerun.

- A kill guard enforces it: a `PreToolUse` hook on Bash in `.claude/settings.json` runs `scripts/kill-guard/guard.sh`, which refuses those commands (exit 2, with a message).
- Kill only a PID your own command started: `cmd & pid=$!; ...; kill "$pid"`. `pkill -P $$` (your own shell's children) is allowed.
- The guard reads command text; it stops accidents, not determined bypasses. Do not try to get around it.

## Pass stamp

When every step of `scripts/check.sh` passes, it writes the run's tier and the tree it checked to `oska-check-passed` in the clone's git directory (never committed), but only if the working tree did not change during the run and no step was skipped with `skip_step --no-stamp` (then it writes no stamp at all). Each worktree has its own stamp. The `.githooks/pre-push` hook skips its check only when all of these hold:

- the stamp is from a full run (a fast run's stamp is never honoured);
- the working tree is clean;
- every ref the push does not delete has the stamped tree at its tip, whether or not it brings new commits. A rewind or an older commit pushed to a branch counts too. For a tag, it is the tree of the commit the tag points to, so a release tag never pushes an unchecked tree.

A deletion adds no condition. Commits below a tip are not checked, so a multi-commit push (a merge of `origin/main` included) skips when its tip was checked. Otherwise the hook runs the check: `scripts/check.sh --fast` when every ref goes to a `ticket/**` branch (see Fast check), else the full `scripts/check.sh`.

- Commit what you checked, then push with no edits in between. Any edit, or a merge that changes the tree, needs a new check.
- Enable the hook once per clone: `git config core.hooksPath .githooks`.
- Never `--no-verify`: the stamp makes it unnecessary.

## Check steps

`scripts/check.sh --list` prints the check's steps in order and runs nothing. Each step is one self-contained `if step "NAME"; then ... fi` block in `scripts/check.sh`; to add a step, add one block where it should run. Tag it after the name: `--fast` also puts it in the fast tier, `--fast-when PATTERN` puts it there when a changed path matches the shell pattern (a new `scripts/<dir>/test.sh` step needs `--fast-when 'scripts/<dir>/*'`; `scripts/check-steps/test.sh` fails without it), `--no-slot` runs a cheap step without a build slot, and `--cargo` marks a cargo step, which skips (without a build slot) when no cargo input changed. A step whose tool is missing calls `skip_step "REASON"`; a skip the pass stamp cannot vouch for calls `skip_step --no-stamp "REASON"`, and that run writes no stamp. There is no other list to update. `scripts/check-steps/test.sh` proves that `--list` matches what a run prints and that every `scripts/*/test.sh` runs.

Outside CI, the cargo steps skip (and the run writes no stamp) when nothing that cargo reads differs from the merge-base with `origin/main`: for example a branch that changes only `CLAUDE.md`, scripts or workflows. Rust tests read all of `docs/`, `examples/`, `site/` and more; `cargo_input` in `scripts/check.sh` holds the list. When the only cargo inputs that changed are in `docs/`, the run skips fmt, clippy and the Windows type-check, and runs just the Rust tests that read `docs/`, printing which (`scripts/docs-tests/list.sh` finds them from the test source, so a new reader joins with no list to update). Its drift guard, `list.sh --check`, fails on any file in `crates/` that names `docs/` and does file I/O but that the list does not find; make the list see it, or add it with a reason to `scripts/docs-tests/not-readers.txt`. That run writes no stamp either, and CI runs everything. A `docs/` file a crate compiles in (`docs/checker.md`, `docs/runtime.md`, `docs/guide/install-receipt.md`), a deleted file, or any other input runs all of cargo. A Rust test that starts reading a new path outside `crates/` must keep that list right. `scripts/check-steps/test.sh` fails if a literal `include_str!` or `CARGO_MANIFEST_DIR` join in `crates/` reads a path that list calls a non-input.

## Build slots

`OSKA_BUILD_SLOTS=N` makes `scripts/check.sh` wait for one of N machine-wide slots (lock files under `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp`) before its build and test steps. It prints one "waiting for a free build slot" line; that is not a hang. The kernel frees the slot when the check ends, however it ends. Unset means no limit (CI, other clones). On the Pi use `OSKA_BUILD_SLOTS=2`, exported with the lean environment. Only `scripts/check.sh` takes a slot; the cargo commands you run while iterating are not limited.

## Portable tests

Any test that builds paths, puts a path in JSON, or sets a home uses `crates/oskac/tests/common/portable.rs` (`Scratch`, `json_str`, `json_path`, `set_home`, `literal_path`), so it passes on Windows the first time. See `CODING_STANDARDS.md` (Tests).

## CI on ticket branches

A push to `ticket/**` or `review-fixes-*` runs `ci.yml`, `windows.yml` and `confinement.yml`, so every platform verifies a ticket before the spec's pull request. Name ticket branches `ticket/<number>-<slug>`.

## Review findings

The loop orchestrator reviews each ticket PR on two axes (Standards and Spec) while its CI runs.

- Fix review findings on the same branch (`ticket/<number>-<slug>`) and push. Do not open a second PR.
- A ticket PR merges only when CI is green and every finding is fixed or deferred to a filed issue that links the PR.
- The orchestrator files the issue for a deferred finding (you do, when told to). The issue links the PR.
- Before merging a ticket PR, run `scripts/expected-workflows/check.sh <pr>` (from the PR's branch if it changes a workflow, once CI has had time to start). It lists every workflow with a `pull_request` trigger whose `branches`, `paths` and `paths-ignore` match the PR, and confirms each reported checks in `gh pr checks`. A missing workflow is a failure, not a pass: GitHub creates no jobs for a workflow file it cannot parse (PR #329). `scripts/check.sh` also runs actionlint on the workflow files to catch that locally.

## Reading CI failures

- Find a failed job: `gh pr checks <pr> --json name,bucket,link` (bucket `fail` marks failures). The job id is the last number in the link.
- Read its log: `gh api repos/aparsons/oska/actions/jobs/<id>/logs --allow-escape-sequences`. Find the failing step with `| grep -nE '##\[error\]|FAIL|panicked'` and read around those lines, not the whole log.
- In your report, list everything not run locally: PowerShell, Windows and macOS, and say whether CI passed on each.

## Windows on demand

To re-test one crate or test on Windows after a push, dispatch the workflow: `gh workflow run windows.yml --ref <branch> -f crate=oskac -f test=self_cli`. `crate` is `all` (default), `oska-run` or `oskac`; `test` is an optional integration-test target (needs one crate); `filter` is an optional test name substring. A dispatch that runs no test fails. Push and pull-request runs are unchanged.
