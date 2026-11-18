# Phase boundaries

Read this file when one phase of work ends and the next phase starts. Examples: the spec is done and the tickets come next, or a ticket is merged and the next ticket waits.

## Why the choice matters

This session holds the full record of the work: each message, each tool result, and each small decision. Every option below except "continue" replaces that record with a summary. A summary loses detail. Thus choose the option that loses only detail that the next phase does not need.

Make this choice only at a boundary. In the middle of a phase, the record is still in use, so continue.

## The order of the options

Ask these questions in order. Use the first option that gets a "yes".

1. **Continue in this session.** Does the next phase need the full record of this phase? Or is there enough context room left for the next phase? If yes, continue.
2. **Clear the context.** Does nothing from this session matter to the next phase? For example, the next ticket stands alone. If yes, clear.
3. **Hand off.** Must the work move to another tool, another directory, or another person? Or do you want to fork a side task and keep this session? If yes, use the `handoff` skill, or `claude-handoff` for a background agent.
4. **Use a subagent.** Can the next task run without the user, and report a result back? If yes, give it to a subagent. This session keeps its record.
5. **Compact.** In all other cases, compact the context. Give an instruction that says what to keep, for example "Keep the decisions and the open questions about the parser."

## Notes

- A handoff is not a general tool to save context. Use it only for the cases in option 3.
- When you compact, the instruction decides the quality of the summary. Name the decisions, open questions, and file paths that the next phase needs.
