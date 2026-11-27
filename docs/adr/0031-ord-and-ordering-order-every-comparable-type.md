# `Ord` and `Ordering` order every comparable type, and unwrap stays absent

## Decision

Oska gets one ordering story, and the core library of `List`, `Option`, `Result` and `String` (spec #660) is built on it.

- **`Ord` and `Ordering`.** The prelude declares `pub trait Ord: Eq` with `fn cmp(self, other: Self) -> Ordering`, and `pub enum Ordering { Less, Equal, Greater }`. A variant is built and matched in call style, like every variant ([ADR 0004](0004-call-style-constructors.md)): `Ordering.Less`. `<`, `<=`, `>` and `>=` work on every `Ord` type, as `==` works on every `Eq` type. For the integer types the checker and both backends keep today's direct comparison; for any other type the operator is `cmp` and a test of the result.
- **Who is `Ord`.** The integer types (`Int`, `I8` to `I64`, `U8` to `U64`), `Bool` (`false` before `true`), `String`, `Option<T: Ord>` (`None` before `Some`) and `List<T: Ord>` (lexicographic, a prefix before a longer list). A user type is `Ord` only when it writes `impl Ord for`. There is no derive.
- **Text compares by code point.** `String` order is the order of Unicode code points, which is the same as the order of the UTF-8 bytes. It does not depend on a locale, a collation table, case folding or normalization, so a sorted list prints the same on all five platforms and both backends ([ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)). `""` is before every other text, and a prefix is before a longer text.
- **`Float` is not `Ord`.** `NaN` is unordered, so no total `cmp` exists for `Float`. The checker refuses `Ord` for `Float` with a message that points to `total_cmp` (#590), which returns this `Ordering`. The pinned results of `Float` ([ADR 0027](0027-float-results-are-pinned-by-the-spec-on-every-platform.md)) are unchanged.
- **`Secret<T>` is not `Ord`.** A comparison that stops at the first differing byte leaks through timing. The checker refuses `<`, `<=`, `>`, `>=` and `cmp` on a `Secret<T>` and says why; `constant_time_eq` stays the way to test equality ([ADR 0006](0006-trust-is-a-flag-on-text.md), #209).
- **Positions are trusted.** A method that returns a position or a count, such as `find` (an `Option<Int>`), `len` or `contains`, returns a trusted value, and so does `cmp` and every comparison operator. The other text methods follow trust ([ADR 0006](0006-trust-is-a-flag-on-text.md), [ADR 0029](0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md)): `untrusted.to_uppercase()` is untrusted. `slice` and every index check its bounds on every call, so a position derived from untrusted text can steer a later `slice` only to a place inside the text, or to a panic, never outside it.
- **`unwrap` and `expect` stay absent.** A call of either is refused with a diagnostic that names the Oska way: `?`, `unwrap_or` and `match`. They are not added with a panic code. A reader of a program sees every point where it can fail, and a panic is for a bug in the program, not for a missing value.
- **Sorting is a stable merge sort.** `sorted`, `sorted_by` and `sort` take O(n log n) time and one extra list of memory. A comparison that is not a total order cannot corrupt memory: a merge sort never reads outside the list whatever `cmp` says. `sorted` builds a new list, so a panicking comparison leaves the list as it was. The steps it counts are part of [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md): the same input counts the same steps on every backend.

### Where the code lives

Checked on the code at the integration tip of spec #660:

- `prelude.osk` is declarations only. Every function in it is body-less, and the runtime implements it: the checker has no body to check for a prelude function, the interpreter finds it by name in `intrinsics.rs`, and each backend compiles it by name. A body written there would not be run by either backend, so the prelude **cannot** hold Oska bodies.
- A module in `std` can hold Oska bodies (`written_in_oska`, as `std.json`, `std.rt` and `std.map` do). The backends run it as one more module of the program, so the checker, the interpreter, the native backend and the differential suite cover it as user code, and every loop back-edge and closure call is a step ([ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)). `PRELUDE_STD` makes its names visible with no `use` ([ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md)).
- A `std` module cannot add a method to `List` or `Option` with an `impl`: `List`, `Option` and `Result` are prelude types, and an inherent `impl` for one is refused (`E0291`). `Map` got its methods because the module that declares `Map` is the module that implements it.

The choice: the **bodies are Oska, in a new `std.core` module** (`written_in_oska`, joined to a program only when its text uses one of the methods, as `std.map` is). `sorted`, `sorted_by`, `filter`, `fold`, `any`, `all`, `find`, `contains`, `reverse`, `concat` and the `Option` and `Result` methods are free functions there. Their signatures are also declared body-less in `prelude.osk`, so the checker, `oskac doc` and the API reference list them as methods. The checker resolves a call of such a method to the `std.core` function, so one implementation serves both backends. The `String` methods (`to_uppercase`, `trim_start`, `trim_end`, `find`, `split_once`, `strip_prefix`, `strip_suffix`, `is_empty`) and the `cmp` of the primitive types stay runtime built-ins next to `to_lowercase`, because they read the text's bytes.

**Fallback.** If resolving a prelude method to a `std.core` function proves too large for one ticket, the same methods are body-less prelude built-ins in `oska-run` and `oska-native`, each under the differential suite with the same output and the same step count. The first ticket that implements a list method records which of the two it used.

## Why

An agent writing Oska reaches for `sorted`, `map`, `filter` and `<` on text and finds none of them. Each miss costs a compile-and-fix turn, and a hand-written loop is longer and harder to review. One `Ord` rule gives every sort, every comparison and a later sorted map the same answer.

Code point order is the only text order that is the same everywhere without a table that changes with a Unicode version. It matches the byte order that `Bytes` and `slice` already use. A locale-aware order would make a sorted list differ between machines, and the differential suite could not hold both backends to one output.

Leaving out `Float` and `Secret<T>` follows from what each gives up. A `Float` order that places `NaN` somewhere is a choice that belongs to `total_cmp`, where the name says so. A secret order is a side channel.

`unwrap` and `expect` turn a missing value into a crash at a place the type does not show. `?`, `unwrap_or` and `match` already cover each use and keep the failure in the signature.

Writing the methods in Oska means one implementation, and a loop in it counts its steps with no extra rule. Writing each in two backends is the cost of every built-in, and the parity gate ([ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)) multiplies it by five platforms.

## Considered options

- **Locale-aware or case-folded text order.** Rejected: it differs between machines and Unicode versions.
- **A total order for `Float` with `NaN` last.** Rejected for `Ord`: it hides a choice. It belongs in `total_cmp` (#590), which returns `Ordering`.
- **`unwrap` and `expect` with a panic code, and a release-build warning.** Rejected: the shortcut makes real programs crash. Revisit only if Adam asks.
- **Bodies in `prelude.osk`.** Rejected: neither backend runs them (see Where the code lives).
- **Built-ins for every method.** Kept as the fallback only: it costs two backends and five platforms for each method.
- **An untrusted position from `find` on untrusted text.** Rejected: an `Untrusted<Int>` would make every `slice` and `len` of parsing code unusable, and `slice` already checks its bounds.

## Consequences

The checker gains `Ord`, `Ordering`, the operator rule for non-integer types, the `E0223` rule for a type with no `Ord`, and the refusals for `Float`, `Secret<T>`, `unwrap` and `expect`. `GLOSSARY.md` defines **Ord** and **Ordering**. The differential suite runs each new method on every backend and compares output and step counts under `--max-steps`, with a seeded random-list sort that checks stability. This builds on [ADR 0004](0004-call-style-constructors.md), [ADR 0006](0006-trust-is-a-flag-on-text.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md), [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md), [ADR 0027](0027-float-results-are-pinned-by-the-spec-on-every-platform.md), [ADR 0028](0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md) and [ADR 0029](0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md), and contradicts none of them. A `Map` key still needs `Eq` and `Hash`, not `Ord` (#591).
