# How should Oska add a List type and other collections (Set, Deque, sorted maps, fixed arrays)?

Date of access: 2026-11-27. Repo paths are as of the working tree on that date; external pages were read on that date.

## Short answer

Inference: `List<T>` already exists, so the work is not "add a List" but "finish it, then build the other collections on it". The order is: (1) `Ord` and `Ordering`, (2) the missing `List` methods (spec #660), (3) `Set<T>` as a thin wrapper over `Map<T, ()>`-style code in Oska, (4) `Deque<T>` written in Oska over two lists, (5) a sorted map written in Oska over a sorted `List`, (6) fixed arrays last, or never. Every collection after `List` should be Oska source in a `std` module that the prelude joins on use, as `Map` is, because only Oska code gets step counting and backend parity for free.

## What exists today

- `List<T>` is a prelude struct with a handle field, "A growable list. `[a, b, c]` makes one, and `for item in list` visits its items in order" (`crates/oska-check/src/prelude.osk:760-781`). Its methods are only `new`, `push`, `len`, `count`, `get`, `at` and `map`.
- The interpreter holds a list as `Value::List(Rc<Vec<Value>>)` (`crates/oska-run/src/value.rs:43-44`). Its intrinsics are the table at `crates/oska-run/src/data.rs:23-44`; `push` copies on write through `Rc::make_mut` (`crates/oska-run/src/memory.rs:675-720`).
- Native code lays a list out as an object with a length word, a capacity word and one word per item, of any item type (`crates/oska-native/src/layout.rs:140-155`; `crates/oska-native/src/lower/lists.rs:1-30`). Literals of constants become static objects (`lists.rs:50-70`). `push` adds in place when the count is 1 and there is room, else calls `std.rt`'s `list_reserve` (`lists.rs:8-20`).
- A list literal `[a, b]` is its own expression kind (`crates/oska-syntax/src/ast.rs:68`) and checks as `List<element>` (`crates/oska-check/src/body.rs:962-975`).
- `for` accepts only a `List` or a `Range` over an integer; anything else is `E0235` "expected a `List` or a range" (`crates/oska-check/src/body.rs:2179-2215`; code at `crates/oska-check/src/lib.rs:146`). There is no iterator trait, so every collection that wants a `for` loop must hand back a `List` (as `Map.entries()` does, `crates/oska-check/src/std/map.osk:186`).
- There is no tuple type in the syntax tree (no `Tuple` in `crates/oska-syntax/src/ast.rs`); `Map` uses a two-field `Entry<K, V>` struct instead (`map.osk:57`).
- `Eq` and `Hash` exist for `List<T>` when `T` does (`prelude.osk:1165`, `prelude.osk:1196`). There is no `Ord` trait and no `Ordering` (the prelude lists only the traits `Eq`, `Hash`, `Display`, `From`; `prelude.osk:1144, 1169, 1200, 1216`). Only integers compare with `<`; spec #660 says so too (`/mnt/project-files/gaps-specs/spec-1-core-library.md`, Problem Statement).
- Generics and bounds exist: `<T: Eq + Hash>`, checked once against bounds, with no `where` clauses and no trait objects (`docs/design.md:25-33, 88, 145`).
- `Map<K, V>` and `Entry` are prelude names whose code is Oska in `crates/oska-check/src/std/map.osk`, joined to a program that writes either name (`crates/oska-check/src/prelude.rs:196-203`, `prelude.rs:233-260`). Its layout is an append-only `List<Entry>` log plus two `Bytes` tables, because "a `List` ... has no `set`, so an entry cannot change in place" (`map.osk:30-47`; ADR 0028, "Amendment: what was built").
- Equal and ordered by insertion, with a fixed unseeded hash, entry limit `P0044`, probe limit `P0045`, and `--max-memory` as the only memory bound (ADR 0028, `docs/adr/0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md`).

## What a List needs, point by point

1. **Representation.** Settled. A list is a value copied on write (ADR 0001, `docs/adr/0001-value-semantics-with-reference-counting.md`), so no list can hold a shared mutable reference and no cycle can form. Inference: any new collection keeps this; a `Deque` or `Set` is a struct of lists and `Bytes`, never a new built-in object kind.
2. **Literal syntax.** `[a, b, c]` exists. A map literal `{"a": 1}` is "an open language decision" (ADR 0028 amendment, last bullet; spec draft, Further Notes). Inference: do not add set or deque literals; use `Set.of(list)` and `Deque.of(list)`, as `Map.of(entries)` does.
3. **Generics.** Present, with bounds checked once at the definition. A `Set<T: Eq + Hash>` and a `SortedMap<K: Ord, V>` fit this directly. Fixed-size arrays would need a length in the type, and nothing in `docs/design.md` mentions const generics; Inference: that is a new checker feature, not a library.
4. **Equality and ordering.** `Eq` for lists exists. `Ord` does not exist and is the one blocker for sorted collections and for `sorted()`. Spec #660 proposes `Ord` and `Ordering`, text by code point, lexicographic `List<T: Ord>`, `Float` and `Secret<T>` refused (`spec-1-core-library.md`, Solution and User Stories 1-7). Inference: ship that spec before any sorted collection, as its own text says `Map` needs only `Eq` and `Hash` so keys and sorting "share one comparison story".
5. **Iteration and for-loops.** Only `List` and `Range` (see above). Inference: an iterator trait would touch the checker, the interpreter, `crates/oska-native/src/lower.rs:3332` (the `For` lowering) and the Lean model; defer it. Every collection offers `to_list()`, `keys()` or `entries()` returning a `List`, which costs a copy but keeps one loop form.
6. **Memory limit.** The interpreter charges each list's capacity before it is made and nets frees (`crates/oska-run/src/memory.rs:1-40`). `--max-memory` is "a resource bound, not a parity promise": the backends may stop at different points (`docs/runtime.md`, Run limits). Inference: new collections need no new limit flag, as ADR 0028 chose for `Map`; a per-collection entry cap (like `P0044`) is optional and costs a new `P` code each.
7. **Steps.** A step is a function call or loop back-edge, and the count must match on both backends (ADR 0021, `docs/adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md`). Inference: Oska-written methods count for free; a Rust built-in such as a native `sort` would need a charged formula on both backends, which ADR 0028 rejected for `Map` ("A built-in written in Rust. Rejected"). Spec #660 recommends writing `sorted` and friends in Oska and flags that the prelude today is signature-only, so ticket one must check how to hold bodies (`spec-1-core-library.md`, Implementation Decisions, "Where the code lives").
8. **Trust.** Trust is a flag on text; results that are not text are trusted; generic code follows the trust of its type arguments (ADR 0006, `docs/adr/0006-trust-is-a-flag-on-text.md`; `docs/design.md:268-274`). A `List<Untrusted<String>>` already exists (`prelude.osk:157`). Inference: collections need no new trust rule; `contains`, `len` and positions return trusted values, and elements keep their flag. Spec #660 asks to confirm that a position from untrusted text is trusted (Security Considerations).
9. **Secrets.** `Secret<T>` is refused as a map key (`E0351`, `E0354`; ADR 0028). Spec #660 refuses `Ord` for `Secret`. Inference: the same two refusals apply to `Set` (hash) and sorted collections (order); no new rule.
10. **Classification.** ADR 0029 puts a flag on text and bytes only, erased after checking, with no runtime flag (`docs/adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md`, Consequences). Inference: it is built on the same mechanism as trust, so collections inherit it with no new work once it is built.
11. **Evaluator versus native parity.** ADR 0014 makes a feature done only when it works natively, with the same output, on all five platforms (`docs/adr/0014-every-supported-platform-builds-and-runs-natively.md`); ADR 0011 requires the differential suite to match output, exit codes and panics. Inference: the cheapest way to meet both is Oska-source collections; new native intrinsics are needed only for a changed list primitive (see ticket 2).
12. **Lean model.** Oska Core in Lean has no generics, and the export refuses lists: `ExprKind::List(_) => return refuse("a list", span)` (`crates/oska-check/src/core_ir.rs:952`; ADR 0018, `docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md`). Inference: list and collection programs are outside the Lean differential test today, so adding a list type to Oska Core is a separate, larger decision than any library work here. `docs/agents/formal.md` governs it and was not read for this note.
13. **Error codes.** `P0002` index out of bounds for `List.at` (`crates/oska-run/src/panics.rs:31`); `E0235` not iterable; `E0266` not a map key; `P0044`, `P0045` for `Map` (`panics.rs:77-80`). Inference: a `Deque.pop_front` on empty returns `Option`, like `Map.get`, so most new methods need no new `P` code.
14. **fmt.** The list literal is parsed in `crates/oska-syntax/src/parser.rs:1699` and printed from the syntax tree. Inference: the formatter needs work only if a new literal form is added, which this note advises against. Not verified: where the printer lives.

## The missing primitive: `List.set`

Inference: `Map` pays "O(table size)" for each update because `Bytes` tables are copied (ADR 0028 amendment, Layout) and the log is append-only because `List` has no `set`. A `Deque` has the same problem: a ring buffer needs in-place writes. The two-list design (a front list reversed, and a back list) gives amortized O(1) push and pop without `set`, but `Deque` indexing and `rotate` become awkward. Adding `List.set(var self, index, item)` (and `pop`, `truncate`) is small: the interpreter already uses `Rc::make_mut` and native `push` already has the copy-on-write path (`lists.rs:8-20`). It is the single native change that makes `Deque`, a faster `Map` and a sorted array practical.

## Comparison with other languages (primary docs)

- **Vec.** Rust's `Vec<T>` is a contiguous growable array; `push` is amortized O(1); indexing out of bounds panics, `get` returns `Option` ([Rust std, Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html)). Oska's `List` already matches this shape (`at` panics, `get` is an `Option`).
- **VecDeque.** A growable ring buffer, not contiguous; `push_back`, `push_front`, `pop_front`; index 0 is the front ([Rust std, VecDeque](https://doc.rust-lang.org/std/collections/struct.VecDeque.html)). It needs in-place writes, so it needs `List.set` or a two-list design.
- **BTreeMap and BTreeSet.** Keys must be `Ord`; iteration is ascending; an inconsistent `Ord` gives unspecified behavior (panics, wrong results, non-termination) but not undefined behavior ([BTreeMap](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html), [BTreeSet](https://doc.rust-lang.org/std/collections/struct.BTreeSet.html)). Oska's equivalent claim is already in spec #660: a bad user `cmp` "cannot corrupt memory" because a merge sort never reads out of bounds. ADR 0028 names a sorted tree as its fallback if the flood defence fails.
- **Arrays.** In Rust, `[T; N]` has a length that is a constant expression, known at compile time ([Rust Reference, Array types](https://doc.rust-lang.org/reference/types/array.html)). In Go the length is part of the array type and must be a non-negative constant; a slice is a descriptor of a segment of an underlying array, and slices share storage, so a write through one changes all ([Go spec, Array and Slice types](https://go.dev/ref/spec#Slice_types)). Inference: slice sharing is the shared mutable state that ADR 0001 forbids, so Oska's `List` must stay a value; a `Bytes.slice` copies for the same reason.
- **Map order.** The Go spec page says a map is "an unordered group of elements" (same page, as summarized by the fetch tool; the full range-over-map text was past the part read and is not verified here). ADR 0028 already chose insertion order against this.
- **Sort stability.** Not verified from a primary source: the fetches of the Rust slice page did not include the `sort` text. Spec #660 requires a stable merge sort (Implementation Decisions, "Sort"); confirm the Rust wording at <https://doc.rust-lang.org/std/primitive.slice.html#method.sort> before an ADR quotes it.
- Python and Swift docs were not read; nothing here needs them.

## Recommended design

Inference, in one list:

- **List** stays the one built-in sequence, a value, copy on write. Add methods in Oska where possible: `sorted`, `sorted_by`, `filter`, `fold`, `any`, `all`, `find`, `contains`, `first`, `last`, `is_empty`, `reverse`, `concat` (the spec #660 list), plus `set`, `pop` and `truncate` as built-ins.
- **Set<T: Eq + Hash>**: an Oska struct over `Map<T, Bool>` or a copy of the map's table code without values; insertion-ordered like `Map` so iteration is deterministic; `to_list()`. `union`, `intersection`, `difference` return sets in the order of the left operand.
- **Deque<T>**: an Oska struct over a `List` and a head index with `List.set`, or two lists. `push_back`, `push_front`, `pop_front`, `pop_back`, `get`, `len`, `to_list()`.
- **SortedMap<K: Ord, V>** and **SortedSet<T: Ord>**: a sorted `List` with binary search, O(log n) lookup and O(n) insert, in Oska. Rank it below `Set` and `Deque`: a use for it is "ordered output", which `sorted()` on a `Map`'s keys already gives. A real B-tree is a later ticket if a program needs 100,000 entries.
- **Fixed arrays**: do not add now. They need a length in a type (new checker feature), they would not run in the Lean model, and `List` plus a `requires` contract on its length covers the use. Revisit for FFI or packed buffers; `Bytes` already is the packed case (`prelude.osk:783-`).
- **No iterator trait, no tuple, no new literals** in this round.

## Ticket order and ADRs

Inference:

1. ADR: `Ord`, `Ordering`, text by code point, `Float` and `Secret` not `Ord`, `unwrap` absent (spec #660 already asks for it). Ticket: can the prelude hold Oska bodies (`spec-1-core-library.md`). Then `Ord` and the comparison operators.
2. ADR (small, amends nothing, may be a section of the ADR above): `List.set`, `pop`, `truncate` as intrinsics on both backends; panic `P0002` for a bad index on `set`. Tickets: interpreter, native (aarch64 and x86_64 share `lower/lists.rs`), differential programs, memory charge as for `push`.
3. `List` methods in Oska (`sorted` stable merge sort, `filter`, `fold` ...), with differential tests that compare step counts (ADR 0021, ADR 0011).
4. ADR: `Set` is an insertion-ordered value like `Map`, keys as in ADR 0028, no literal. Ticket: `Set` in a `std.set` module added to `PRELUDE_STD` (`crates/oska-check/src/prelude.rs:203`), `Set.of(list)`, set operations, glossary terms.
5. ADR: `Deque`, same pattern. Ticket: `std.deque` module.
6. ADR: sorted map and sorted set (needs ticket 1), with the choice of sorted list now and B-tree later.
7. Lean model and `docs/agents/formal.md`: only if lists are added to Oska Core, which is its own ADR 0018 decision.
8. Fixed arrays: no ticket; record "not now" in the ADR of ticket 2 or 6.

Each ADR must name ADR 0001 (value semantics), 0006 (trust), 0011 and 0014 (parity), 0021 (steps) and 0028 (map) in its text.

## Open questions

- Whether prelude bodies can be written in Oska without the `std.map`-style "joined when the name appears" trick (spec #660 leaves this to its first ticket).
- Whether `List.set` is allowed as a built-in under the ADR 0014 feature freeze language: the gate applies to new features done natively on all platforms (`docs/adr/0014-...md`, Decision, "Catch up, then gate"); the repo says v0.6.0 turned it on, which was not checked against a release file.
- Whether the sorted-map fallback stated in ADR 0028 should be the sorted-list design here.
- `docs/agents/formal.md` and other agent docs were not read; the Lean statement rests on `core_ir.rs:952` and ADR 0018 alone.
- No injection attempts were found in the pages fetched.

## Sources

Repo (read on 2026-11-27):
- `GLOSSARY.md` (Map, Entry, Insertion order, Bytes)
- `docs/adr/0001-value-semantics-with-reference-counting.md`
- `docs/adr/0006-trust-is-a-flag-on-text.md`
- `docs/adr/0011-oska-generates-native-code-itself.md`
- `docs/adr/0014-every-supported-platform-builds-and-runs-natively.md`
- `docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md`
- `docs/adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md`
- `docs/adr/0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md`
- `docs/adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md`
- `docs/design.md`, `docs/runtime.md`, `docs/guide/language-tour.md`
- `crates/oska-check/src/prelude.osk`, `prelude.rs`, `body.rs`, `core_ir.rs`, `lib.rs`, `std/map.osk`
- `crates/oska-run/src/value.rs`, `data.rs`, `memory.rs`, `panics.rs`
- `crates/oska-native/src/layout.rs`, `lower/lists.rs`
- `crates/oska-syntax/src/ast.rs`, `parser.rs`
- `/mnt/project-files/gaps-specs/spec-1-core-library.md`
- `/mnt/project-files/math-spec/map-spec-draft.md`

External (accessed 2026-11-27):
- https://doc.rust-lang.org/std/vec/struct.Vec.html
- https://doc.rust-lang.org/std/collections/struct.VecDeque.html
- https://doc.rust-lang.org/std/collections/struct.BTreeMap.html
- https://doc.rust-lang.org/std/collections/struct.BTreeSet.html
- https://doc.rust-lang.org/reference/types/array.html
- https://go.dev/ref/spec#Slice_types
- https://doc.rust-lang.org/std/primitive.slice.html (sort text not obtained)
