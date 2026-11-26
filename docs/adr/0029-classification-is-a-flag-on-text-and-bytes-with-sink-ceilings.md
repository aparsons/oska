# Classification is a flag on text and bytes, sinks have ceilings, and only a `Declassify` capability lowers it

## Decision

A value of text or bytes can carry a **classification**: one of `Public < Internal < Confidential < Restricted`. It is a flag, like the trust flag in ADR 0006, and not a wrapper type.

- A classification flows one way. A value may flow into a slot with the same or a higher classification, never a lower one. Prelude text and byte operations give the result the highest classification of their inputs.
- A result that is not text or bytes (`Int`, `Bool`, …) is unclassified, as it is trusted under ADR 0006. This leaves **implicit flows** open, for example `if name.contains("x") { log("yes") }`. The documentation says so, and the feature does not claim to stop them.
- Each sink capability has a **ceiling**: the highest classification it accepts. Logging, network send and `std.llm` come first. The default ceiling is `Public`. Attenuation can only lower a ceiling, and raising one is a widening.
- Only code that holds a `Declassify` capability can lower a classification. `main` passes it down like any other capability, and every use is also an audit site.
- The four names are fixed by the language. The syntax stays open so that a per-package order can be added later without breaking programs.
- `Secret` is a separate axis and does not change. Trust and classification are two independent flags: each has its own rule and its own join, and a value can be untrusted and confidential at once.

## Why

Oska already tracks one flag by one-way flow, and capabilities already name every sink, so classification reuses both and needs no new concept for authors to learn. A fixed ladder keeps the checker and the authority report small. A capability for declassification makes the authority to release data visible in `main`'s signature, which ADR 0002 prefers to a call that any code can write, and ADR 0016 catches one that nothing uses.

## Considered options

- **Label every result of inspecting classified text.** Rejected for now: labels would spread to most values and force many declassifications.
- **Refuse to branch on a `Bool` from classified text.** Rejected for now: it also refuses legitimate code such as a length check.
- **A per-package lattice from the start.** Rejected for now: packages would need rules to map their orders onto each other.
- **Declassification as an audit site only.** Rejected: any code, or an agent clearing an error, could add the call.
- **Declassification by a validator-style function with a typed result.** Rejected for now: it needs a new language feature.
- **`Secret` as the top of the ladder.** Rejected: secret integers have extra rules (no branching, constant time) that do not belong to the ladder.
- **One combined label for trust and classification.** Rejected: it needs a new lattice and changes ADR 0006.

## Consequences

- The checker, the interpreter and every native backend need the same behaviour (ADR 0014). Labels are erased after checking, so no runtime label is needed.
- The Lean model changes in the same PR as the checker rule (ADR 0018; see `docs/agents/formal.md`).
- The authority report lists each sink's ceiling, each `Declassify` holder and each widening. These are self-assessment evidence (ADR 0017). A classification name borrowed from a standard does not make a program conform to it.
- Classification gives no timing guarantee. Constant time holds for native code only.
- Reading from a capability gives the capability's own classification, as everything read from a connection is untrusted under ADR 0025. How that is set is for the spec.
