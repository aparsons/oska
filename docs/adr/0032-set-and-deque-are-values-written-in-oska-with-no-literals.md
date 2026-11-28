# `Set` keeps insertion order and `Deque` is a value with amortized O(1) ends; both are written in Oska and have no literal

## Decision

Oska gets two generic collections in the prelude, `Set<T>` and `Deque<T>`. Each is a value, like `List` and `Map`: it is copied on write and a mutating method needs a `var` ([ADR 0001](0001-value-semantics-with-reference-counting.md)). Each is written in Oska in `crates/oska-check/src/std/` (`set.osk`, `deque.osk`) and joined to a program that writes its name, as `Map` is, so both backends run the same code and every call and loop back-edge counts as a step ([ADR 0011](0011-oska-generates-native-code-itself.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md), [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md), [ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md)). Neither needs a new built-in beyond `List.set`, `pop` and `truncate` ([ADR 0031](0031-list-gets-set-pop-and-truncate-and-no-fixed-arrays-iterators-tuples-or-literals-yet.md)). There is no set or deque literal: `Set.of(list)` and `Deque.of(list)` build from the one literal that exists.

### Set

- **Items.** `T` needs `Eq` and `Hash`. The items that qualify are the keys of ADR 0028. `Float` is refused as an item, because NaN and negative zero make equality unstable, and a `Secret` value is refused, because a hash or a probe order leaks timing. Each refusal has a stable `E` code: `E0266` for a type that cannot be an item (the code a `Map` key gets), `E0351` or `E0354` for a `Secret` item. `Float` is refused today with `E0266`, because it has no `Hash`.
- **Layout.** A `Set<T>` is a struct over one `Map<T, Bool>`: the item is the key and the value is always `true`. It adds no table code of its own. The table, the fixed hash, the 64-probe limit, the rebuild with the next hash and every constant are those of ADR 0028, so the two types cannot drift apart.
- **Order.** `to_list()` walks the items in **insertion order**. `add` of an item already present keeps its place. `remove` then `add` of the same item moves it to the end. Two sets are equal when they hold the same items in the same order, as for `Map`. `Set.of(list)` drops repeated items and keeps the first-seen place of each.
- **Operations.** `new`, `of`, `add`, `remove`, `contains`, `len`, `is_empty`, `to_list`, `union`, `intersection` and `difference`. `add` and `remove` return `Bool` (whether the set changed). `contains` is O(1) on average. `union(a, b)` is the items of `a` in order, then the items of `b` not in `a`. `intersection(a, b)` and `difference(a, b)` keep the order of `a`. A result is a new set; neither operand changes.
- **Limits.** The same two limit panics as `Map`: an `add` past the entry limit panics with `P0044`, and items that collide under every hash panic with `P0045`. The default entry limit is that of `Map` (4,194,304), and `Set.with_max_entries(n)` lowers it (a negative `n` is 0). No run-limit flag sets it; `--max-memory` bounds a growing set with `P0040` on both backends. A set operation that builds a larger set than the limit panics with `P0044`.
- **Trust.** An item keeps its trust flag ([ADR 0006](0006-trust-is-a-flag-on-text.md)). `to_list()` returns the items unchanged. `len`, `contains`, `add` and `remove` return trusted values.
- **A missing item.** `remove` of a missing item returns `false`. It is never a panic.

### Deque

- **Items.** `T` has no bound: a deque never hashes or compares an item. A `Secret` item is allowed, as in a `List`.
- **Layout.** A struct of one `List<Option<T>>` ring, a head index and a count. The ring is a power of two in length. `push_back` and `push_front` write into a free slot with `List.set`. When the ring is full, the deque rebuilds into a ring of twice the length, which copies the items once and is paid for by the pushes before it: the cost per push is amortized O(1). `pop_front` and `pop_back` write `None` into the slot they free and move the head or the count. When the count falls below a quarter of the ring, the deque rebuilds into a ring of half the length, never below a fixed minimum, so a queue that grew once does not keep its memory. Every end operation is O(1) apart from a rebuild.
- **Order.** `to_list()` returns the items from front to back. `get(i)` counts from the front: index 0 is the front. Two deques are equal when `to_list()` gives equal lists, whatever their rings hold; the head index and the ring length are not part of the value.
- **Operations.** `new`, `of`, `push_back`, `push_front`, `pop_back`, `pop_front`, `get`, `len`, `is_empty` and `to_list`. `Deque.of(list)` puts the first item at the front. `pop_back` and `pop_front` on an empty deque return `None`. `get` out of range returns `None`. None of these panics.
- **Limits.** A deque has a maximum item count, with a default of 4,194,304 and `Deque.with_max_entries(n)` to lower it (a negative `n` is 0). A push past it panics with `P0051`, a new stable `P` code (the next free one on main on 2026-11-27; the build ticket takes the next free code if another spec took it first). The ring size is computed with checked `Int` math. `--max-memory` bounds a growing deque with `P0040`. No run-limit flag sets either limit.
- **Trust.** An item keeps its trust flag. `pop_back`, `pop_front` and `get` return the item with the flag it had. `len` and the indexes are trusted.

### Both

- **Not iterable.** `for` accepts only a `List` or a range (`E0235`). `to_list()` is the way in, and it costs a copy ([ADR 0031](0031-list-gets-set-pop-and-truncate-and-no-fixed-arrays-iterators-tuples-or-literals-yet.md)).
- **Names.** `Set` and `Deque` are prelude names. A program that declares its own gets `E0202`, as for `Map`. `use std.set` and `use std.deque` also work.
- **Classification.** Collections add nothing to [ADR 0029](0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md): the flag is on the text and bytes inside.

## Why

A program that needs a seen-set or a queue otherwise fakes it with a `List` scan, which is O(n) per step, or with a `Map<T, Bool>`, which has the wrong name. An insertion-ordered `Set` follows the rule of ADR 0028: every platform and both backends must print the same output, so no order may depend on a seed, an address or a hash. A `Deque` with a ring and `List.set` is the smallest change to the cost of a queue: popping the front of a `List` is O(n). Writing both in Oska means they count steps by themselves, where a Rust built-in would need a charged formula on both backends (ADR 0028, ADR 0031). The refusals are the ones `Map` already has, so a secret cannot leak through a new door.

## Why `Set` wraps `Map` and does not copy its table code

This is a recorded assumption, not a measurement. No benchmark could be run when this was decided, because `Set` did not exist yet and `Map` stores `Bool` values that a set never reads. A first measurement is below.

- **Assumed cost.** A `Map<T, Bool>` carries one `Bool` per entry in its log. A copy of the table code without values would save that word per item and one field read per lookup. We assume that is under 10% of the time of `add` and `contains`, and not worth a second copy of the probe, rebuild and flood code that ADR 0028 says is "the weakest part of this design".
- **What a copy would cost.** Two copies of the flood defence must be tested and amended together. A fix to one that misses the other is a security bug.
- **The check.** The first ticket that builds `Set` (spec #798) writes it over `Map<T, Bool>` and records the steps and the time of 100,000 `add` and `contains` calls against the same calls on `Map<T, Bool>`. The pull request of that ticket (#879) measured 2,000 `add` and 2,000 `contains` calls: 85,963 steps for `Set` against 67,009 for `Map<Int, Bool>`, about 1.28 times, steps only, which is under the threshold. The 100,000-call measurement (follow-up issues #884 and #892) ran 100,000 `add` then 100,000 `contains` on `Int` items, release build: 3,709,260 steps for `Set` against 2,965,654 for `Map<Int, Bool>` (`put` and `contains_key`), about 1.25 times; wall time 1.30 s against 0.94 s on the interpreter (about 1.4 times) and 0.27 s against 0.19 s in native code (about 1.4 times). Both are under the threshold. If a copy would be at least 2 times faster in steps, this ADR is amended and the table code is shared as a helper in `std.map`, not copied. A copy of the code is rejected in either case.

## Considered options

- **`Set` over a sorted `List`.** Rejected: `contains` is O(log n), `add` is O(n), and it needs `Ord`, which `Set` does not otherwise need. The sorted family is its own ADR.
- **A `Set` that keeps hash order.** Rejected, now and later, for the reason in ADR 0028: it breaks the same-output rule.
- **A `Deque` over two lists (a front list reversed, and a back list).** Not chosen. It needs no `set`, but `get(i)` and `pop` after a one-sided run cost O(n) to rebalance, and the amortized bound holds only for a single owner (a copy of the deque that both ends pop from can repeat the rebuild). The ring has the same bound for every copy. It stays the fallback if `List.set` is refused by the ADR 0014 gate.
- **A `Deque` as a `List` and a head index with no ring.** Not chosen: pops from the front leave dead slots until a rebuild, and a queue that pushes and pops at the same rate grows without bound until the rebuild.
- **A set or deque literal.** Rejected for now. It is the same open language decision as the `Map` literal, and it needs `oskac fmt` changes (ADR 0028, amendment; ADR 0031).
- **Allowing a `Float` item.** Rejected: `Float` has no `Hash` today (`E0266`). It is reopened when the rules for `Float` say how it hashes (#592) with the same answer as for `Map`.
- **A flag for the item limit.** Rejected, as ADR 0028 rejected one for `Map`: `--max-memory` already stops the growth.
- **Collections as Rust built-ins.** Rejected, for the step-count reason in ADR 0028.

## Consequences

`Set` costs what `Map<T, Bool>` costs, including the O(table size) copy on every update that ADR 0028 names; a cheaper update is the same follow-up for both. A `Deque` costs one `Option` word per ring slot, and up to twice its length in slots. `P0051` is a new code and joins the table in `crates/oska-run/src/panics.rs`; no new `E` code is added. The differential suite ([ADR 0011](0011-oska-generates-native-code-itself.md)) gains random add, remove and union sequences for `Set`, random push and pop sequences from both ends for `Deque` (including a ring that grows and shrinks), a flood test for `Set` that must stop with `P0045`, a copy that must not see a change, and step-count comparisons. The checker tests show `E0266`, `E0351` and `E0354` for a `Set` item and `E0202` for a user type named `Set` or `Deque`. The syntax crate and the formatter do not change. The Lean model does not change, because lists stay outside it ([ADR 0018](0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)), so `docs/agents/formal.md` does not apply. The sorted collections are a later ADR that names this one. `GLOSSARY.md` gains **Set** and **Deque**.
