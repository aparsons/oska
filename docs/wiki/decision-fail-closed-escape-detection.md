# Decision: eval escape detection fails closed

On 2026-11-18 the second review of PR #380 (#215, `oska-eval analyse`) still found bypasses in detecting work outside the workspace (`$VAR`, brace expansion, `.*`) after the first round of fixes. Shell commands can't be fully analysed statically, so the coordinator directed a fail-closed rule: any word with a shell-expansion character, or a pattern that could reach above the workspace, is "unresolved, possibly outside" unless it plainly resolves inside. Inside is the proven case, never the default.

The same principle should apply to `grade` (PR #384): refuse symlinks rather than resolve them.

Related: [goal-drain-backlog](goal-drain-backlog.md).

Lint note (2026-11-18, `origin/main@fd7696a`): PR #384 (#214, `grade`) merged 2026-11-18 after two fix rounds in grading confinement, so the "should apply" line above was acted on. PR #380 merged 2026-11-18; its follow-ups are #388 (open). See [handoff-2026-11-18](handoff-2026-11-18.md).
