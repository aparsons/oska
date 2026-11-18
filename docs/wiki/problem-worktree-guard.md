# Problem: the worktree command guard refuses ordinary shell forms

On 2026-11-17 two implementers reported that the worktree command guard refused commands they needed:
- the #351 implementer: compound git commands;
- the #167 implementer: heredocs.

Both worked around it. The guard's source and rules were not checked as of 2026-11-17. Candidate fix: find where the guard is defined (hook or settings) and either allow these forms or document the expected alternatives in docs/agents/implementer.md.

Related: [problem-logs-in-worktree](problem-logs-in-worktree.md).

On 2026-11-18 the #366 implementer reported a third case: the guard blocked heredoc Python, `git config` and `git -c`, and `setsid` returned at once. That makes three agents in one day, which is past the point of noting it; it needs fixing.

On 2026-11-18 an investigation found that no repo rule causes these refusals. The only hook, `scripts/kill-guard/guard.sh`, blocks only broad kills. The likely cause, unconfirmed, is the auto-mode permission classifier from `~/.claude/settings.json` (`defaultMode: "auto"`) and its built-in defaults. Fixing it means changing either user permission settings or agent habits, so the coordinator took it to the user.
