# Problem: the Pi's CPU is the bottleneck

On 2026-11-15 the measured Pi load was 10 to 22 on 4 cores for hours during specs #183, #153, #185 and #184 (user memory `spec-191-pr-per-ticket-trial`). Review was not the bottleneck.

On 2026-11-17 the coordinator capped concurrent implementers at 2, matching `OSKA_BUILD_SLOTS=2` (docs/agents/implementer.md at `fc411f9`).

Related: [goal-drain-backlog](goal-drain-backlog.md).
