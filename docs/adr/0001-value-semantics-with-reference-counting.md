# Value semantics with reference counting, no borrow checker

Oska guarantees memory safety through value semantics and automatic reference counting:
- There are no lifetimes or borrow checker.
- There are no shared mutable references, so reference cycles can't form and no cycle collector is needed.
- A `var` is mutated in place when its reference count is 1 and copied on write otherwise.

The research behind this design ([findings 1 and 8](../research/2026-11-ai-first-secure-language.md)) requires memory safety, but not any particular mechanism for it. We want signatures to describe *authority* (capabilities), not lifetimes. We also want to avoid the borrow-checker compile-fix loop for agents. That second point is a judgment call; it has not been measured.

## Considered options

- **Ownership and borrowing (Rust).** Rejected. Lifetimes make signatures noisier, and borrow errors make agents iterate more.
- **Tracing GC.** Not chosen. Reference counting without cycles is simpler and its performance is predictable.
- **Regions.** Not chosen. Too novel to rely on, given models have little training data for them.

## Consequences

Graph-shaped data has to use IDs or arena-style collections instead of pointers.

Mutation goes through **mutating methods** (`fn push(var self, x: T)`), which can only be called on `var` bindings. No parameter other than `self` can be mutated, so all other data has to flow out through return values.
