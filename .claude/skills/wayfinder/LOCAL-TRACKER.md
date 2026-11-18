# Local markdown tracker for wayfinder

Read this file only when `docs/agents/issue-tracker.md` has no "Wayfinding operations" section.

## Layout

```
.scratch/<effort>/
  map.md                  the map, with the same body sections as a tracker map
  issues/
    01-<slug>.md          one file for each ticket
    02-<slug>.md
```

`<effort>` is a short slug for the destination. Number the ticket files in the order that you create them.

## Ticket file

Start each ticket file with these lines:

```markdown
# <Ticket title>

Type: research | prototype | grilling | task
Status: open | claimed by <user> | closed
Blocked by: 01-<slug>, 03-<slug>

<The one question that this ticket answers.>
```

Write `Blocked by: none` when the ticket has no blocker.

## Operations

- **Claim.** Change the `Status:` line to `claimed by <user>`.
- **Blocking.** A ticket is unblocked when every file in its `Blocked by:` line has `Status: closed`.
- **Frontier.** The ticket files with `Status: open` that are unblocked. The lowest number comes first.
- **Resolve.** Add an `## Answer` section at the end of the file. Change the status to `closed`. Add a line to "Decisions so far" in `map.md`, with a relative link to the ticket file.
