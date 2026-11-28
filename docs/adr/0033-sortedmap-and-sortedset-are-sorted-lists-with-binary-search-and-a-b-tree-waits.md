# `SortedMap` and `SortedSet` are sorted lists with binary search, written in Oska; a B-tree waits for a measured need

## Decision

Oska gets two generic collections for output in key order, `SortedMap<K, V>` and `SortedSet<T>`. Each is a value, like `List`, `Map` and `Set`: it is copied on write and a mutating method needs a `var` ([ADR 0001](0001-value-semantics-with-reference-counting.md)). Each is written in Oska in `crates/oska-check/src/std/sorted.osk` and joined to a program that writes its name, through `PRELUDE_STD` by name, as `Map`, `Set` and `Deque` are, so both backends run the same code and every call and loop back-edge counts as a step ([ADR 0011](0011-oska-generates-native-code-itself.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md), [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md), [ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md), [ADR 0032](0032-set-and-deque-are-values-written-in-oska-with-no-literals.md)). Neither needs a new built-in beyond `List.set`, `pop` and `truncate` ([ADR 0031](0031-list-gets-set-pop-and-truncate-and-no-fixed-arrays-iterators-tuples-or-literals-yet.md)) and the `Ord` of [ADR 0034](0034-ord-and-ordering-order-every-comparable-type.md). There is no literal: `SortedMap.of(entries)` and `SortedSet.of(list)` build from the one literal that exists.

### Keys and items

- **Bound.** `K` and `T` need `Ord`. The types that qualify are those of ADR 0034: the integer types, `Bool`, `String`, `Option<T: Ord>`, `List<T: Ord>`, and a user type that writes `impl Ord for`. `V` has no bound.
- **Refusals.** `Float` is refused as a key or item, because `NaN` is unordered. `Secret<T>` is refused, because an order that stops at the first differing byte leaks through timing. Each refusal uses the stable `E` code that the `Ord` refusal of ADR 0034 already gives for that type, and a bound that a type does not meet is refused at the call in the program that names the type (`new`, `of`, `add`, `put`), never at a call deep inside `sorted.osk`, as for `Set`. This ADR adds no `E` code. The build ticket (#806) copies the exact codes from the checker's `Ord` refusal and shows them in CLI golden files. They are not the `E0266` of `Map`, because that code means "no `Hash`".
- **Ordering of keys.** The key order is `Ord.cmp` and nothing else. Two keys are the same key when `a.cmp(b)` is `Equal`. The code never uses `==` on a key, so a key type has one comparison to get right.

### Layout and cost

- **`SortedSet<T>`** is a struct over one `List<T>` kept in ascending order with no repeats.
- **`SortedMap<K, V>`** is a struct over one `List<SortedEntry<K, V>>` in ascending key order with no repeated key. `SortedEntry<K, V>` is a struct of `key` and `value`, declared in `sorted.osk`, with the same two field names as `Entry` of `Map` ([ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md)). It is a separate type so a `SortedMap` adds nothing to the `Map` API.
- **Lookup.** `get` and `contains` use a binary search over the list: O(log n) calls of `cmp`.
- **Insert and remove.** O(log n) to find the place, then O(n) to move the items after it. An insert pushes the new item and moves it down to its place with `List.set`. A remove moves the later items up one place with `List.set`, then `pop`s the last. A replace of the value of an existing key is a `List.set` at one index. Every move is a loop step, so the cost shows in `--max-steps`.
- **Ends.** `first` and `last` read index 0 and `len - 1`: O(1).
- **Copy.** A mutating call on a list that another value also holds copies the list first ([ADR 0001](0001-value-semantics-with-reference-counting.md)); that copy is O(n), as for `Map`.

### How `K`'s order is called

The operators `<`, `<=`, `>` and `>=` are not built for a user type with `impl Ord` and give `E0223` there (ADR 0034). `sorted.osk` therefore never writes an operator on a key. It calls `cmp` and `match`es on the `Ordering` (`Ordering.Less`, `Ordering.Equal`, `Ordering.Greater`), which works for every `Ord` type, built in or user. The binary search is one private function `find(list, key) -> Int` that returns the index where `key` is or would go, plus a flag for whether it is there. A change of the operators on user types does not touch `sorted.osk`.

### Operations

- **`SortedSet<T>`.** `new`, `of`, `add`, `remove`, `contains`, `len`, `is_empty`, `first`, `last` and `to_list`.
- **`SortedMap<K, V>`.** `new`, `of`, `get`, `put`, `remove`, `contains`, `len`, `is_empty`, `first`, `last` and `to_list`.
- **Return types.** `get` returns `Option<V>`. `put` returns `Bool` (whether the key was new). `remove` returns `Bool` for a set, and for a map `Option<V>`, the value that was removed. `contains` returns `Bool`. `first` and `last` return `Option<SortedEntry<K, V>>` for a map and `Option<T>` for a set, and `None` when empty. `to_list()` returns the entries or items in ascending order. A missing key is never a panic: `get` and `remove` of a missing key give `None` or `false`.
- **`of(list)`.** For a map, `SortedMap.of(entries)` is a `put` of each entry in order, so the **last** value for a repeated key wins. For a set, `SortedSet.of(list)` is an `add` of each item in order, so a repeat is dropped. The cost is O(n²) in the worst case; a program with a large list builds it from `sorted` and a pass that drops repeats if that matters.
- **Order.** Ascending by `Ord`, whatever the order of the calls. This is the one difference from `Map` and `Set`, which iterate in insertion order (ADR 0028, ADR 0032).
- **Not iterable.** `for` accepts only a `List` or a range (`E0235`); `to_list()` is the way in and costs a copy (ADR 0031).

### Equality

Two sorted collections are equal when `to_list()` gives equal lists. As the order is the key order, equal contents have one layout, so it is the same as comparing the inner lists. `SortedMap` equality needs `V: Eq`. A `SortedMap` or `SortedSet` is not `Ord` itself in this round.

### Trust, secrets and classification

- A key, an item and a value keep their trust flag ([ADR 0006](0006-trust-is-a-flag-on-text.md)). `to_list()`, `get`, `first` and `last` return them unchanged. `len`, `contains`, `is_empty` and every `Bool` and position return trusted values, as for `Map`.
- A `Secret` key or item is refused (above). A `Secret` value in a `SortedMap<K, Secret<V>>` is allowed, because a value is never compared.
- The order of a key leaks nothing about a secret, because a secret cannot be one. An untrusted key is compared by its bytes with no more power than `sorted` has.
- The classification flag of [ADR 0029](0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md) is on the text inside; collections add nothing to it.
- No new authority and no system call.

### A bad `Ord`

An `impl Ord` that is not a total order (a `cmp` that is not antisymmetric or not transitive, or that answers differently on the same pair) gives a **wrong answer, never a corrupted memory**. The binary search keeps its two bounds inside `0..len` on every step and ends after at most `log2(len) + 1` rounds whatever `cmp` returns. Every read and `List.set` index it computes is checked by `List`, which panics with `P0002` outside the list ([ADR 0031](0031-list-gets-set-pop-and-truncate-and-no-fixed-arrays-iterators-tuples-or-literals-yet.md)), and the code never builds an index from anything but those bounds. So a bad `cmp` can make `get` miss a key that is present, make `put` insert a repeat or in the wrong place, and make `to_list()` come out unsorted. It cannot read or write outside the list, loop for ever or get past `--max-steps`: the cost of every call is bounded by its loops, each of which counts steps. A `cmp` that loops or panics is the program's own, and it stops at `--max-steps` or its panic like any call. The same promise holds for `sorted` in ADR 0034. The attack test (below) runs a random `cmp`, a constant `cmp` and a `cmp` that flips each call.

### Limits

There is **no entry-limit panic and no `with_max_entries`**, and so no new `P` code. `Set` and `Deque` need one because a flood or a ring doubling can grow a table in a few calls. A sorted list grows by one slot per item, and a new item costs O(n) steps to place, so `--max-steps` stops a huge build in step order and `--max-memory` bounds the list with `P0040` on both backends (ADR 0028), as for `List`. If the build ticket finds a reason to add a limit, it takes the next free `P` code **checked late on main** (the highest in `crates/oska-run/src/panics.rs` was `P0051` on 2026-11-28, which ADR 0032 reserved for `Deque`) and amends this ADR. No run-limit flag sets one.

### Names

`SortedMap` and `SortedSet` are prelude names. A program that declares its own gets `E0202`, as for `Map`. `use std.sorted` also works.

## Why

A program that needs output in key order otherwise sorts the keys of a `Map` on every print, or keeps a `List` in order by hand and scans it. A sorted list with binary search is the smallest design that gives ordered output, O(log n) lookup and `first` and `last` with no new language feature, no new built-in and no hash. It has none of the flood and probe code of ADR 0028, so there is no flood to defend against: the cost per call does not depend on the keys a caller chooses, only on `n`. The key order is `Ord`, which ADR 0034 already makes the same on all five platforms and both backends (text by code point, no locale), so the output is the same everywhere. Writing it in Oska means the step count is the same on both backends with no charged formula (ADR 0021, ADR 0028).

The O(n) insert is the price. A sorted list is also the one layout where a bulk `to_list()` is a plain copy. For the sizes a program written by an agent usually holds (hundreds to some thousands of entries) the move is a short loop of `List.set`. The research note (`docs/research/2026-11-list-and-collections.md`) ranks it below `Set` and `Deque` because `sorted()` on the keys of a `Map` already gives ordered output once; this type pays off for a collection that is read in order often and changed seldom.

## A B-tree is the later option

A balanced tree gives O(log n) insert and remove. It is **not built now**. It needs more code (splits, merges and a node type), more places for a bug that two backends must agree on, and a layout that `List.set` alone does not make cheap, because each node update still copies a node. It is the fallback ADR 0028 names for a flood-prone hash, and this ADR is that fallback for the case of ordered keys.

- **Trigger.** A real program, in `examples/` or reported in an issue, whose measured cost is dominated by `SortedMap` or `SortedSet` inserts or removes, shown as steps and time from the differential suite for a stated `n`. The research note's rule of thumb is a collection of about 100,000 entries, but the number is a guess and the measurement decides. No such program exists today.
- **What stays.** The public methods, their return types, the order, the equality and the refusals above. A B-tree replaces the layout inside `sorted.osk` and nothing a program writes changes. A program must not rely on the layout, only on the order and the costs named here.
- **What changes.** The cost of `put` and `remove` and the code in `sorted.osk`. It is then a new ADR that names this one and says what the measurement was.

## Considered options

- **A B-tree or another balanced tree now.** Rejected for now (above): more code and no program that needs it.
- **A hash map that sorts its keys on `to_list()`.** Rejected: O(n log n) on every ordered read, and it makes the type `Eq` and `Hash` and `Ord` at once.
- **A key order given by a comparison function stored in the collection (`SortedMap.with_cmp(f)`).** Rejected: a closure in the value makes equality and copying unclear, and `Ord` already gives one order per type. A program that wants another order wraps the key in a type with its own `impl Ord`.
- **A `Float` key ordered by `total_cmp`.** Rejected for now, as in ADR 0034: that choice belongs to `total_cmp` (#590). It is reopened when `Float` has an `Ord` answer.
- **Operators (`<`) on keys in `sorted.osk`.** Rejected: they are `E0223` for a user type (ADR 0034). `cmp` works for every `Ord` type.
- **Allowing equal keys (a multimap).** Rejected: `put` replaces, as for `Map`. A program that wants several values per key stores a `List` as the value.
- **An entry limit with its own `P` code.** Rejected (see Limits): `--max-steps` and `--max-memory` bound the growth.
- **A literal.** Rejected for now, for the reason in ADR 0028 and ADR 0032.
- **Written as Rust built-ins.** Rejected, for the step-count reason in ADR 0028.

## Consequences

`sorted.osk` is a new module in `PRELUDE_STD`, joined when a program writes `SortedMap` or `SortedSet`. No new `E` or `P` code is added by this ADR; the checker tests show the `Ord` refusal for a `Float` and a `Secret` key and `E0202` for a user type named `SortedMap` or `SortedSet`. The differential suite ([ADR 0011](0011-oska-generates-native-code-itself.md)) gains random put, remove and `of` sequences checked against a plain sorted `List` model, `first` and `last` on empty and one-item collections, a copy that must not see a change, step-count comparisons between backends, and an attack test with an inconsistent `cmp` that must end inside `--max-steps` with no memory error. The syntax crate and the formatter do not change. The Lean model does not change, because lists stay outside it ([ADR 0018](0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)), so `docs/agents/formal.md` does not apply. `GLOSSARY.md` gains **SortedMap** and **SortedSet**. This ADR builds on ADR 0001, 0006, 0011, 0014, 0021, 0028, 0029, 0031, 0032 and 0034 and contradicts none of them. It needs Adam's merge.
