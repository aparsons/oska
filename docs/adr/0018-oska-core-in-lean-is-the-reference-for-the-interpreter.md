# Oska Core in Lean is the reference for the interpreter

## Decision

A subset of Oska without generics or traits is formalized as **Oska Core**, a calculus written in Lean. Lean proves three theorems about it: progress, preservation and **authority safety** (running a function performs only effects of capability kinds in its authority). Proofs of trust rules come later, on the same calculus.

Oska Core has an executable semantics. The checker exports each checked program that is in Oska Core as **core IR**, a versioned JSON form, and the Lean reference runs it. The interpreter must agree with the Lean reference on every runtime test whose program is in Oska Core, just as the native backend must agree with the interpreter (the differential suite). The export refuses a program outside Oska Core, and the test prints how many runtime tests it covered.

Proofs and the Lean differential test block merge. A change to how a construct of Oska Core is typed or run updates the Lean model in the same pull request, and no proof may be left unfinished (`sorry`). Proving compiler passes correct is out of scope until the core proofs are done.

## Why

"No ambient authority" is Oska's main claim, and tests can only sample it. A proof covers every program in the subset. An executable semantics ties the proof to the toolchain: without it, the Lean model and the interpreter could drift apart and the proof would describe a language nobody runs.

## Considered options

- **Lean reads the checked program.** Rejected: ADR 0007 keeps the checked program's shape internal so that it can change freely.
- **Lean parses Oska source.** Rejected: a second resolver is the drift ADR 0007 was written to prevent.
- **Generics and traits from the start.** Rejected for now: the first proofs would take much longer.
- **Allow `sorry` with an issue.** Rejected by the maintainer: an unfinished proof proves nothing, so it may not merge.
- **Verify the native backend's passes now.** Deferred: the differential suite already compares the backend with the interpreter.

## Consequences

- The checker gains a core IR export, and a change to the core IR's shape is a new version. ADR 0007 still holds: every backend runs the checked program. The Lean reference is not a backend, and the core IR is the one stable form taken from the checked program for it.
- `scripts/check.sh` gains a formal step. Like the shellcheck step, it skips where Lean is not installed, except in CI, where a missing Lean fails the step, so the gate cannot pass by skipping. Linux CI installs a pinned Lean.
- A pull request that touches Oska Core's typing or running needs Lean work, which raises the cost of changing those parts of the language.
- For programs in Oska Core, the interpreter is no longer the last word: a difference from the Lean reference is a bug in one of the two.
