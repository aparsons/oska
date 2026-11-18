# Problem: finished agents keep re-reporting

On 2026-11-17 several implementer agents (#351, #349, the env fix, the #353 refresh, the ADR 0009 fix) finished, but showed "waiting on its own background work" and sent the same report repeatedly. Most likely cause: they ran `scripts/check.sh` as detached background tasks and kept waking on them. The ADR 0009 fix agent repeated its report three times; when stopped, no processes from its worktree were still running.

On 2026-11-17 the coordinator stopped the agent once its PR was pushed. Candidate fix, not applied as of 2026-11-17: tell implementers in their briefs to run the check in the foreground with a timeout, or to wait for the background check before reporting.

Related: [problem-pi-cpu](problem-pi-cpu.md).

On 2026-11-17 the coordinator told the #362 conflict agent to run its check in the foreground, and it still went to background work. The #351 implementer had already reported that foreground waits are cut off at 180s, so a 35-minute check cannot run in the foreground. Revised candidate fix (2026-11-17): accept the background check, and tell agents to report only after it finishes, then stop.

On 2026-11-18 the #169 agent was "made to report" twice while its own check or CI was still running, even though its brief said to wait. Agents seem to hit a turn limit while waiting. Working pattern (2026-11-18): the coordinator watches the agent's `.rc` file or `gh pr checks` with a background job, then resumes the agent with SendMessage to finish.
