# List gets `set`, `pop` and `truncate`; fixed arrays, an iterator trait, tuples and new literals wait

## Decision

`List<T>` gets three built-in methods, each needing a `var` list. Like `push`, each is an intrinsic that both the interpreter and native code implement, with the same result, panic and step count ([ADR 0011](0011-oska-generates-native-code-itself.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md), [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)).

- **`set(i, item)`.** Replaces the item at position `i`. An index below 0 or not below the length stops with `P0002`, the code and report that `at` uses, on every backend. It never grows the list.
- **`pop()`.** Removes the last item and returns `Option<T>`. An empty list gives `None`. It is never a panic, as `get` is never one.
- **`truncate(n)`.** Keeps the first `n` items. An `n` at or above the length changes nothing. A negative `n` counts as 0, as it does for `Map.with_max_entries`. It is never a panic.
- **Value semantics.** All three copy on write ([ADR 0001](0001-value-semantics-with-reference-counting.md)): a copy of a list made before the call never sees the change. In the interpreter they use the `Rc::make_mut` path of `push`. Native code uses the copy-on-write path of native `push`, in `lower/lists.rs`, shared by aarch64 and x86_64.
- **Memory.** Charged as for `push`. A call that must copy a shared list charges the new list's capacity, and a call on a list that only this value holds charges nothing more. None of them reduces capacity, so `pop` and `truncate` free no memory by themselves. `--max-memory` stops a program that grows through them with `P0040` ([ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md), amendment).
- **Trust.** Items keep their trust flag ([ADR 0006](0006-trust-is-a-flag-on-text.md)). `pop` returns the item with the flag it had.
- **Not now.** This round adds no fixed-size array, no iterator trait, no tuple type and no new literal. They are listed below with their reasons.

## Why

`Map` pays O(table size) for every update, and keeps an append-only log, because a `List` cannot change an item in place ([ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md), amendment). A queue has the same problem. `set`, `pop` and `truncate` are the smallest change that lets `Set`, `Deque` and sorted collections be written in Oska, so they count steps by themselves on both backends. A built-in written in Rust for each collection would need a step formula on both backends. `P0002` for a bad `set` index keeps one rule for every index error. `Option` for `pop` keeps the rule that an empty collection is a value.

## Considered options

- **Fixed-size arrays.** Not now. A length in a type needs a new checker feature, which `docs/design.md` has no room for. Arrays would also fall outside Oska Core in Lean, which refuses lists today ([ADR 0018](0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)). A `List` with a `requires` on its length covers the use. `Bytes` is already the packed case. Reopen for FFI or packed buffers.
- **An iterator trait.** Not now. It would change the checker, the interpreter, the `for` lowering in native code and the Lean model. `for` keeps accepting a `List` or a range, and every collection hands back a `List` (`to_list()`, `keys()`, `entries()`), which costs a copy and keeps one loop form. Reopen when that copy is the measured cost of a real program.
- **Tuples.** Not now. A two-field struct does the job, as `Entry<K, V>` does for `Map`. A tuple type is a syntax and checker change that no collection needs.
- **New literals.** Not now. A set, deque or map literal is the same open language decision as the `Map` literal (ADR 0028, amendment). `Set.of(list)`, `Deque.of(list)` and `Map.of(entries)` build values from the one literal that exists. A new literal also needs `oskac fmt` changes.
- **Sharing slices.** Rejected, now and later. A slice that shares storage is shared mutable state, which [ADR 0001](0001-value-semantics-with-reference-counting.md) forbids.
- **Collections as Rust built-ins.** Rejected, for the step-count reason in ADR 0028.

## Consequences

The differential suite ([ADR 0011](0011-oska-generates-native-code-itself.md)) gains programs for each method, with a bad `set` index, `pop` on an empty list, an out-of-range `truncate` and a copy that must not see the change, and compares step counts. The checker, interpreter and both native back ends change; the syntax crate and the formatter do not. The Lean model does not change, because lists stay outside it, so `docs/agents/formal.md` does not apply unless that changes. Later ADRs for `Set`, `Deque` and the sorted collections each name this one.
