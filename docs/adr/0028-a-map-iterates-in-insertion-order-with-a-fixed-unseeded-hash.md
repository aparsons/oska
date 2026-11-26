# A map iterates in insertion order with a fixed, unseeded hash

## Decision

Oska gets a generic `Map<K, V>` in the prelude. It is a value, like `List`: it is copied on write and a mutating method needs a `var` ([ADR 0001](0001-value-semantics-with-reference-counting.md)).

- **Insertion order.** `entries()`, `keys()` and `values()` walk the **entries** in **insertion order**. `put` on an existing key keeps that key's place. `remove` then `put` of the same key moves it to the end. Two maps are equal when they hold the same entries in the same order.
- **A fixed hash.** The hash of a key is a fixed 64-bit function with documented constants for each key type. It has no seed and does not read an address, a clock or the platform. Two platforms and both backends give the same bucket for the same key.
- **Layout.** A dense array of entries in insertion order, and an index table of positions for lookup (open addressing). Iteration walks the array, so it never depends on the hash. `remove` leaves a tombstone in the array and compacts when tombstones pass a fixed fraction. Order of the remaining entries is kept.
- **Written in Oska.** The table logic is Oska code in `std.rt`. Only the allocation is a built-in. Every probe is a loop back-edge, so it counts as a step on both backends without extra rules ([ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)).
- **Flood defence.** A lookup that probes more than a fixed limit rebuilds the index with the next fixed hash from a documented list. If the last hash also exceeds the limit, the map panics with a stable code. A lookup for keys an attacker picks therefore costs at most the probe limit, or it panics. It never scans without bound.
- **Entry limit.** A map has a maximum entry count. It comes from the run limits and has a default. An insert past it panics with a stable `P` code. The index size is computed with checked `Int` math.
- **Keys.** A key type needs `Eq` and `Hash`. These are keys: `Int`, the sized integers, `Bool`, `String`, `Bytes`, `Path`, `Option<T>` and `List<T>` when `T: Hash`, and a struct or enum with a compiler-derived `Hash`. `Float` is refused as a key, because NaN and negative zero make equality unstable. `Secret` values are refused, because a hash or a probe order leaks timing, as `==` on them is refused today. Each refusal has a stable `E` code.
- **Trust.** A key keeps its trust flag ([ADR 0006](0006-trust-is-a-flag-on-text.md)). `keys()` and `entries()` return it unchanged.
- **Missing key.** `get` returns `Option<V>`. A missing key is never a panic.

## Why

The interpreter is the reference for every program, and every platform and both backends must print the same output ([ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)). A map whose order depends on a seed, an address or a platform hash breaks that rule, and an agent cannot see why a test passes once and fails later. Insertion order is easy to state and to test. A fixed hash is predictable, so an attacker who picks keys can force collisions. The probe limit, the second hash and the entry limit bound that damage.

## Considered options

- **A sorted tree (balanced).** No hash, so no flooding. Rejected: iteration is by key order, not insertion order, every key type would need a total order, and the cost is `O(log n)` per operation. It stays the fallback if the flood defence proves too weak.
- **A keyed hash with a per-map key.** A key derived from the program keeps it deterministic. Rejected for now: the key would have to be the same on both backends, and an attacker who can read the program can recompute it, so it adds code without a stronger guarantee.
- **A seeded or random hash.** Rejected, now and later: it breaks the same-output rule.
- **A built-in written in Rust.** Rejected: the steps of a runtime built-in would have to be charged by formula on both backends. Oska code counts by itself ([ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md)).

## Consequences

Insertion order costs 8 bytes per entry for the index. We accept it. The hash constants and the list of second hashes are part of the contract: changing one changes the order of no map (order is insertion order) but changes where a flood attack trips. The differential suite ([ADR 0011](0011-oska-generates-native-code-itself.md)) gains random insert, update and remove sequences and flood tests. The Lean model gets the order and update rules ([ADR 0018](0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)). `std.json` keeps `Value.Object(List<Member>)` and its wire format; a map view is added beside it.

How strong the flood defence must be is the weakest part of this design. The attack test (100,000 keys that collide under the first hash must finish within the step budget or panic with the stable code) decides it. If it fails, this ADR is amended.

## Amendment: what was built

The build of spec #591 changed these points. The decisions above about order, the fixed hash, the flood defence, keys and trust stand.

- **A module, not the prelude.** `Map<K, V>` and `Entry<K, V>` are in the standard module `std.map`, written in Oska in `crates/oska-check/src/std/map.osk`, and a program writes `use std.map`. Only the two limit panics (`P0044`, `P0045`) are prelude functions, so both backends raise the same panic. Nothing is in `std.rt`. Moving `Map` into the prelude later is a follow-up.
- **Layout.** There is no tombstone array. The map is an append-only log of entries and two `Bytes` tables: `order` (the live entries in insertion order) and an open-addressing `table` for lookup. An update or a remove copies one whole `Bytes` table, so it costs O(table size). Ticket #603's limits bound it; a cheaper update is a follow-up.
- **Probe and rebuild.** A lookup stops after 64 probes. `put` then rebuilds the table with the next hash, which is a fixed mix of the 64-bit hash. If the last hash also passes the limit, the map panics with `P0045`. Keys with the same 64-bit hash therefore end in `P0045`; this is the intended outcome of the flood test.
- **Entry limit.** The default is 4,194,304 entries, and `Map.with_max_entries(n)` lowers it (a negative `n` is 0). A past-limit `put` panics with `P0044`. No run-limit flag sets it. The memory limit covers a map instead: a map's tables and log are ordinary values, so `--max-memory` stops a growing map with `P0040` on both backends (Adam chose this over a new flag, 2026-11-26).
- **Hash.** FNV-1a for the integers, `Bool`, `String`, `Path`, `Option<T>` and `List<T>`, and a derived `Hash` for structs and enums. A hash is a plain `Int` with no trust flag, so the hash of untrusted text is not itself untrusted. `Float` is refused when `Float` exists (#592).
- **Codes.** `E0266` for a type that cannot be a key; a `Secret` key gives `E0351` or `E0354`.
- **`std.json`.** `Value.as_map()` returns the members as a map. A duplicate key gives the reason `duplicate key`, with the place of the later member (from 0) as the offset, not the byte offset that `parse` reports.
- **Added beyond this ADR.** `Map.of(entries)` builds a map from a list of entries, because the `{"a": 1}` literal is an open language decision.
