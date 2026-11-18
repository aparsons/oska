# Decision: eval escape detection fails closed

On 2026-11-18 the second review of PR #380 (#215, `oska-eval analyse`) still found bypasses in detecting work outside the workspace (`$VAR`, brace expansion, `.*`) after the first round of fixes. Shell commands can't be fully analysed statically, so the coordinator directed a fail-closed rule: any word with a shell-expansion character, or a pattern that could reach above the workspace, is "unresolved, possibly outside" unless it plainly resolves inside. Inside is the proven case, never the default.

The same principle should apply to `grade` (PR #384): refuse symlinks rather than resolve them.

Related: [goal-drain-backlog](goal-drain-backlog.md).
