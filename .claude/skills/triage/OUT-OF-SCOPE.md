# Out-of-scope records

Read this file when you check `.out-of-scope/` during triage, or when you reject an enhancement.

An out-of-scope record says that the project will not support a concept, and why. It stops the same request from coming back again and again.

## Format

- One file for each concept, not for each issue: `.out-of-scope/<concept>.md`. Use a short kebab-case concept name, for example `plugin-system.md`.
- Write it as a short design doc that a human can read in one minute.

```markdown
# <Concept>

## Not supported
What the project does not support, in one or two sentences.

## Reason
The durable reason: scope, a technical constraint, or strategy.
Do not give a reason that depends on the current week, such as "no time now".

## Prior requests
- #12, 2026-03-02: one line on what was asked.
- #57, 2026-08-19: one line on what was asked.
```

## Matching

Match an incoming item to a record by concept, not by keyword. Two requests with different words can ask for the same concept. Two requests with the same words can ask for different things.

When you find a match, show it to the maintainer. The maintainer decides:

- **Confirms**: add the item to "Prior requests", link the record in the comment, and close the item.
- **Reconsiders**: delete or edit the record, then continue triage.
- **Says the items differ**: continue triage. Do not change the record.

## When to write a record

Write or update a record only for a rejected enhancement. Do not write one for an item that is already implemented. For that, point to the implementation instead.
