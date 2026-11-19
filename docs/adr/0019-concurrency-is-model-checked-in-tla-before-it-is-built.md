# Concurrency is model-checked in TLA+ before it is built

## Decision

Before parallel tasks, `Sched` channels or `select` are implemented, their design is written as a TLA+ specification, and the TLC model checker must pass it. Implementation tickets open only after that. The specification covers the scheduler, channels and `select`, and the memory model: how reference counts change when tasks share values, and the rule that a `var` is changed in place only when its reference count is 1 (ADR 0001). How counts are made safe across tasks, for example with atomic updates, is a design choice that the specification settles.

The specifications live in the repository and run in the formal step of `scripts/check.sh`, with a pinned TLC that Linux CI installs. As with Lean (ADR 0018), a missing TLC fails the step in CI.

## Why

Concurrency bugs depend on rare interleavings that tests seldom hit, and they are cheapest to fix before code exists. In v0, spawned tasks run one at a time, so the design can still change.

Oska has no garbage collector to model. ADR 0001 chose reference counting with no shared mutable references, so the part of memory management that parallel tasks can break is the reference count and the in-place mutation check.

## Considered options

- **Model the v0 sequential tasks now.** Not chosen: they run in spawn order, and TLC would find little.
- **Model the design after it is built.** Rejected: design bugs found then cost rework.
- **Reopen ADR 0001 for a tracing collector.** Rejected: nothing in this work needs one.
