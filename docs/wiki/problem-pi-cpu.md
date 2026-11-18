# Problem: the Pi's CPU is the bottleneck

On 2026-11-15 the measured Pi load was 10 to 22 on 4 cores for hours during specs #183, #153, #185 and #184 (user memory `spec-191-pr-per-ticket-trial`). Review was not the bottleneck.

On 2026-11-17 the coordinator capped concurrent implementers at 2, matching `OSKA_BUILD_SLOTS=2` (docs/agents/implementer.md at `fc411f9`).

Related: [goal-drain-backlog](goal-drain-backlog.md).

Lint note (2026-11-17): "bottleneck" is the 2026-11-15 measurement, not a present-tense claim. Related: [problem-refresh-after-merge](problem-refresh-after-merge.md), [problem-logs-in-worktree](problem-logs-in-worktree.md).
