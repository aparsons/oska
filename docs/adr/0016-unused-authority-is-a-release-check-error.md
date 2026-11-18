# Unused authority is a release-check error; Oska has no warning tier

## Decision

**Unused authority** is a capability-bearing parameter, or a field of a `cap struct`, that a function never uses, either directly or by passing it on. The **release check** (`oskac check --release`) rejects it, as it already rejects `dbg(...)` (`E0216`) and `todo()` (`E0293`). An ordinary `oskac check` does not report it.

Oska gains no warning tier. A diagnostic's severity stays `error` or `note`, and every diagnostic a check reports fails it.

The rule is deliberately simple:

- **Any use counts.** A method call on the capability, passing it on, attenuating it, storing it, returning it or capturing it in a closure is a use. The checker does not try to prove that the capability is reached.
- **What is capability-bearing** is what [ADR 0002](0002-capabilities-are-the-effect-system.md) says: `cap` types, `cap fn` closures, types that carry a capability through a type argument, and parameters of a bounded type parameter.
- **The fix.** On `main`, removing the parameter is a `machine` fix, since nothing calls `main`. Anywhere else, it is a `maybe` fix, because callers change too.
- **Methods whose signature a trait fixes are exempt.** An implementation cannot drop a parameter the trait declares.
- **There is no escape hatch.** Naming a parameter `_net` does not silence it. A capability a function does not use is removed, not hidden.

## Why

A function's authority is exactly the capabilities its parameters can reach (ADR 0002), and the authority report is how a reviewer sees it. Agents over-ask: a probe accepted `net: Net` on a `main` that never touched the network. The report then overstates what the program does, and a report that overstates is a weaker review signal. Least authority is the principle the language is built on, and this makes it checkable.

Rejecting it only in the release check keeps work in progress quiet. A half-written function often takes a capability before its body uses it. The release check is already the gate for code that is fine while working but must not ship, and the Claude Code Stop gate runs it, so an agent cannot finish with unused authority.

## Considered options

- **A new `warning` severity, which `--release` turns into a failure.** Rejected: it adds a concept, a second kind of output agents must learn to read or ignore, and a habit of ignoring it. "Every diagnostic is an error" stays true with the release-check rule.
- **An error in every check.** Rejected: it would flag code while it is being written, and the post-edit hook would report it after every edit.
- **A `_name` escape hatch, as Rust has for unused variables.** Rejected: it would let an agent silence the check without narrowing anything, and the authority report would still overstate.

## Consequences

Unused locals and imports, and a hint that a capability could be narrowed to `read_only()`, are later follow-ups. If they are added, they follow this decision: release-check errors, not warnings.
