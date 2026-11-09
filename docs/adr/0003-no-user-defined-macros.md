# No user-defined macros

Oska has no user-defined macros or other compile-time code generation that users can write. All code that runs appears on the page. A reviewer, human or model, can read a function's real behavior and authority straight from the source, without expanding anything first.

We accept more boilerplate in exchange, because agents can write boilerplate cheaply. Hidden expansion would undermine principle 3 ("a function signature should tell you everything it can do"). It would also undermine the [finding 7](../research/2026-11-ai-first-secure-language.md) premise that everything relevant should be explicit and on the page.

## Consequences

Repetitive patterns that macros would otherwise cover need a different solution: built-in, compiler-defined derivations for a fixed set of traits (equality, hashing, serialization) or code generators that write ordinary source files. Do not add user macros to save typing without revisiting this ADR.
