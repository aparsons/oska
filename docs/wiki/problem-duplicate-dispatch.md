# Problem: duplicate dispatch across threads

On 2026-11-18 several project threads were working the same backlog at once. The coordinator dispatched #355, which the eval-harness thread's PR #384 already closed. The agent noticed and stopped before writing any code.

Check used since then: `gh api -X GET search/issues -f q="repo:aparsons/oska is:pr is:open <N> in:body"` before dispatching ticket N.

Related: [goal-drain-backlog](goal-drain-backlog.md).
