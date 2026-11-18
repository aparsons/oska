# Baseline lists for the Standards axis

Paste both lists into the Standards brief in full. A repo standard overrides an item here. Skip each item that a tool already enforces.

## Code smells

These smells come from the catalogue in Martin Fowler's book *Refactoring*. Each smell is a judgement call, never a hard violation.

| Smell | What it looks like | Usual fix |
|---|---|---|
| Unclear name | A name does not say what the thing is or does. | Rename it with a domain term from the glossary. |
| Duplication | The same logic is in two or more places. | Move the logic into one function and call it from each place. |
| Feature envy | A function uses the data of another type more than its own. | Move the function to the type whose data it uses. |
| Data clump | The same group of values travels together through many signatures. | Put the group into one named type. |
| Primitive obsession | A raw string or number stands for a domain idea with rules. | Make a small type that holds and checks the value. |
| Repeated switch | The same `match` or `switch` on one kind appears in many places. | Put the behaviour on the kind, or keep one table. |
| Shotgun surgery | One small change needs edits in many files. | Bring the related parts into one module. |
| Divergent change | One module changes for many unrelated reasons. | Split the module by reason for change. |
| Speculative generality | Hooks, parameters, or layers exist for needs that nobody has. | Remove them until a real need comes. |
| Message chain | A caller walks a long chain such as `a.b().c().d()`. | Give the first object a method that returns what the caller needs. |
| Middle man | A type only passes calls to another type. | Remove it and call the real type. |
| Refused bequest | A subtype ignores or overrides most of what it gets from its parent. | Use composition, or split the parent. |

## Security checklist

A repo security standard overrides an item here.

| Item | What to look for |
|---|---|
| Unvalidated input | Untrusted input reaches a sink without validation. Sinks include a shell, a query, a path, HTML, and a deserializer. |
| Leaked secret | A secret is in code, in a log line, or in a test fixture. |
| Unpinned dependency | A new dependency has no exact pinned version. |
| Weakened check | A test or a check is disabled, skipped, or made weaker. |
| Wide permission | A permission, token scope, or file access is wider than the change needs. |
| Fail-open error path | An error path allows the action when it should refuse it. |
