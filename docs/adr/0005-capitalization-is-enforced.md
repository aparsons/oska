# Capitalization is part of the language

Some names must start with an uppercase letter, and others with a lowercase one. The checker enforces this; it is not just a convention.

| Must start uppercase | Must start lowercase |
| --- | --- |
| types, enum variants, traits | variables, functions, fields, parameters |

Because of this rule, a bare uppercase name in a pattern always refers to a fieldless variant. A name that doesn't resolve is an error, with a suggested fix such as "did you mean `None`?". It never silently binds a new variable.

## Why

The parser can't tell a pattern like `None` from a binding without name resolution (#30). In languages where capitalization is only a convention, a misspelled variant such as `Nnoe => 0` silently becomes a catch-all that matches everything. That is exactly the kind of bug agents produce and reviewers miss.

Making capitalization part of the language removes the ambiguity, without needing heuristics. A second rule closes the remaining gap: a binding or `_` arm followed by further arms is an "unreachable arm" error.

## Considered options

- **Heuristics in the checker.** For example, warn when a binding's name matches a variant name. Rejected: still ambiguous, and easy to miss.
- **Requiring qualified variants.** Always write `Option.None`. Rejected: too verbose for the most common patterns.

## Consequences

Programs that break the capitalization rule do not compile. Agents already follow these conventions, so in practice the cost is close to zero.
