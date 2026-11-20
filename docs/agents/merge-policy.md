# Merge policy

Auto-merge: on

Adam set this policy on 2026-11-18: agents merge code when it is complete and tested. The skills (`implement`, `implement-spec`, `chief-of-staff`) follow this file. To stop all agent merges, change the line above to `Auto-merge: off`. Then agents stop at "ready for review" and report.

## Which pull requests

- A ticket pull request into its spec branch (`ticket/<n>-<slug>` into `spec/<n>-<slug>`).
- A spec pull request into `main`.
- Any other agent pull request into `main`.

## Merge only when every condition is true

1. The pull request is not a draft, and it has no merge conflict.
2. `scripts/expected-workflows/check.sh <pr>` passes. Every workflow that must report did report.
3. Every check on the head commit passed. A pending, failed, cancelled, or missing check stops the merge.
4. The full `scripts/check.sh` passed on the head commit, on CI or locally with a pass stamp.
5. `diff-review` ran on the final diff. Each finding is fixed, or deferred to a filed issue that links the pull request ("Follow-ups from the PR #<pr> review (#<spec>)").
6. The acceptance criteria of every linked ticket are met. The Spec axis of `diff-review` reports nothing missing.
7. The head commit did not change after steps 2 to 6.
8. No exception below applies.

## Exceptions: a human merges

Stop at "ready for review" and tell Adam why when the pull request:

- changes `.github/workflows/`, `scripts/release/`, `scripts/install/`, `site/oska-release.pub`, or anything that `docs/agents/release.md` covers;
- changes `.claude/settings.json`, `.githooks/`, a guard script under `scripts/*-guard/`, or this file;
- removes, skips, or weakens a test or a `scripts/check.sh` step;
- adds a dependency edge to `oska-sys`, or adds `unsafe` code;
- comes from an author outside the agent flow, such as an external contributor;
- is a one-way door in its "Merge risk" section (see the `pr` skill).

## How to merge

1. Read the head commit SHA: `gh pr view <pr> --json headRefOid,isDraft,mergeable`.
2. Check every condition above against that SHA.
3. Merge with a merge commit, pinned to the SHA you checked:

   ```sh
   gh pr merge <pr> --merge --match-head-commit <sha> \
     --subject "Merge PR #<pr>: <title> (#<issue>)"
   ```

4. Post one comment on the pull request: the conditions you checked and the SHA. Mark it as written by an AI agent.
5. Close the merged tickets as `docs/agents/issue-tracker.md` says.

Do not use `gh pr merge --auto`. When the base branch has no required status checks, GitHub merges at once, with no check. Spec branches never have required checks.

If a condition fails, do not merge. Fix the cause and check again, at most 2 times. Then stop and report the failing condition.
