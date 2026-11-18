# ADR format

Read this file before you write or edit an ADR (Architecture Decision Record).

## Oska

This repo already has ADRs in `docs/adr/`. Follow their existing style: a `#` title that states the decision, short paragraphs of prose, and an optional `## Consequences` section. Read two or three of them before you write a new one.

## Location and number

- Put ADRs in the ADR folder from `docs/agents/domain.md`. The default is `docs/adr/`.
- Name each file `NNNN-slug.md`. `NNNN` is a four-digit number. `slug` is the title in lower case with hyphens.
- To find the number, find the highest number in the folder and add one.
- Do not reuse or change the number of an existing ADR.

## Minimal template

```markdown
# <Decision, stated as a fact>

<Context: 1 sentence on the problem or the force.> <Decision: 1 sentence on what we chose.> <Reason: 1 sentence on why.>
```

A minimal ADR is a title and 1 to 3 sentences. That is enough for most decisions.

## Optional sections

Add a section only when it helps a future reader:

- `## Status`: when the ADR replaces an older one, or when it is not yet accepted.
- `## Options considered`: when the rejected options are not obvious.
- `## Consequences`: when the decision changes what other work must do.

## When to write an ADR

Write an ADR only when all three are true:

1. The decision is hard to reverse.
2. A future reader will be surprised by it, or will ask "why not the obvious way?".
3. There was a real trade-off between alternatives.

## Kinds of decision that usually qualify

- The shape of the architecture, such as one process or many.
- The style of integration between two contexts, such as events or direct calls.
- A technology that locks the project in, such as a storage engine or a language.
- A scope boundary: what the system will not do.
- A deliberate deviation from the obvious approach.
- A constraint that the code does not show, such as a legal or platform limit.
- A non-obvious rejection: an option that looks good but that we refused.
