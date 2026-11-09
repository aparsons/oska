# Trust is a flag on text, and plain `String` means trusted

## Decision

Untrusted text is written as `Untrusted<String>`. Despite the syntax, it is **not** a generic wrapper type. The checker tracks it as a trust flag on text types, using one-way flow (the same mechanism that stops a `cap fn` from flowing into a `fn`):

- Trusted text may flow into any slot that accepts untrusted text. Untrusted text may never flow into a trusted slot.
- Prelude text operations follow trust automatically. If any text input is untrusted, every text type in the result is untrusted. Results that are not text (`Int`, `Bool`, …) are trusted.
- In user code, a `String` parameter, field, or return type means **trusted**. Code that accepts raw input must say so by declaring `Untrusted<String>`. User functions are explicit about trust: they cannot be generic over it.

## Why

A real `Untrusted<T>` wrapper type would have two costs:

- every text method declared twice
- a way to accept trusted `String` wherever untrusted text is allowed

Oska has no subtyping and no overloading, so the second cost alone would rule it out: `Path.from` could not accept both kinds of text.

A trust flag gives the behavior the design describes ("converting to a non-text type makes it trusted") directly, with nothing duplicated. Treating plain `String` as trusted makes every ordinary text slot a **structural position** by default. Accepting raw input becomes visible in the signature, as principle 3 requires.

## Considered options

- **A generic wrapper type.** Rejected because it duplicates the text API and needs subtyping.
- **`String` means untrusted by default.** Rejected because it would be secure only if every structural parameter were annotated, and a single forgotten annotation would let untrusted text through.

## Consequences

- **Trust assertions.** A validator turns untrusted text into trusted text by calling `.assume_trusted()`. This is a **trust assertion**: it is listed as an **audit site**, alongside `Secret.expose()`.
- **Inspection is free.** Checks that produce non-text results (`len`, `contains`, `parse_int`, matching against literals) need no trust assertion.
- **Some functions need two versions.** A user function that works on text may need both a trusted and an untrusted version. If that becomes painful, a trust-generic parameter could be added later, and adding it would not break existing code.
