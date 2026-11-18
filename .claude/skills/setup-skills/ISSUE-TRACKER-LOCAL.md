# Issue tracker: local Markdown

This repo tracks issues as Markdown files under `.scratch/`. There is no remote service.

## Layout

```
.scratch/<feature>/
  spec.md                 the spec for the feature
  map.md                  wayfinding map, only for wayfinding work
  issues/
    01-<slug>.md          one file for each ticket
    02-<slug>.md
```

`NN` is a two-digit number in creation order. `<slug>` is a short kebab-case title.

## Ticket file

```markdown
# <Title>

Status: needs-triage
Type: task
Blocked by: 01, 03

<Body>

## Comments

- 2026-11-18, agent: <comment>
```

- `Status:` holds the triage state, with the label from `docs/agents/triage-labels.md`, or `closed`.
- `Type:` and `Blocked by:` are only for wayfinding. Remove them otherwise.

## Operations

| Operation | Do this |
| --- | --- |
| Create | Write a new file with the next free number. |
| View | Read the file. |
| List | Read the `Status:` line of each file in `issues/`. |
| Comment | Append a dated line under the "Comments" heading. |
| Change the state | Edit the `Status:` line. |
| Close | Append a comment with the reason, then set `Status: closed`. |

## Generic phrases

| A skill says | Do this |
| --- | --- |
| "Publish to the tracker" | Create a ticket file. |
| "Fetch the ticket" | Read the ticket file. |
| "Apply the label for a role" | Set the `Status:` line. |

## Wayfinding

- **Map.** `.scratch/<feature>/map.md` holds the notes, the decisions so far, and the open areas. It lists each child ticket in order.
- **Child.** A ticket file with a `Type:` line: `research`, `prototype`, `grilling`, or `task`.
- **Blocking.** The `Blocked by:` line lists ticket numbers. A ticket is unblocked when each listed ticket has `Status: closed`.
- **Claim.** Set `Status: in-progress`.
- **Frontier.** The first ticket in map order that is not closed, not in progress, and not blocked.
- **Resolve.** Append the answer as a comment, set `Status: closed`, then add one line to `map.md`: a short summary and the file name.
