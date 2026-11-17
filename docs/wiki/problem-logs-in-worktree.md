# Problem: check logs inside the worktree defeat the pass stamp

On 2026-11-17 both implementers (#351 → PR #352, #350 → PR #353) independently wrote `scripts/check.sh` logs into their worktree. The untracked files made the tree dirty, so no pass stamp was written (#351) or the pre-push hook re-ran the full check, about 35 minutes (#350). docs/agents/implementer.md's `LOG`/`RC` example at `fc411f9` doesn't say where logs go.

On 2026-11-17 the coordinator dispatched an environment fix (see the ledger in [goal-drain-backlog](goal-drain-backlog.md)).
