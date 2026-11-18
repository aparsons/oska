# Problem: duplicate dispatch across threads

On 2026-11-18 several project threads were working the same backlog at once. The coordinator dispatched #355, which the eval-harness thread's PR #384 already closed. The agent noticed and stopped before writing any code.

Check used since then: `gh api -X GET search/issues -f q="repo:aparsons/oska is:pr is:open <N> in:body"` before dispatching ticket N.

Related: [goal-drain-backlog](goal-drain-backlog.md).

Related (2026-11-18): the #381/#389 case (closed as a duplicate on 2026-11-18) is in [goal-drain-backlog](goal-drain-backlog.md) and [decision-macos-libsystem-design](decision-macos-libsystem-design.md); the queue in [handoff-2026-11-18](handoff-2026-11-18.md) names this check.
