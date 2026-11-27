# What should be in the Oska prelude, and should Map or Entry be?

Date of access: 2026-11-27. Repo facts cite a path and line; outside facts cite a link. "Inference:" marks my own conclusion. Not read: Swift's standard library docs (page needs JavaScript), Elm's default-import list (page returned no text), Zig's std (the language reference says only that `std` is brought in by `@import`). Gleam's tour does not use the word "prelude".

## Short answer

Keep `Map` in the prelude. Take `Entry` out of the bare prelude names, or rename it, and stop treating a prelude name as an `E0202` error for user types before 1.0. Move the authority types nothing needs in `main`'s signature (`Budget` and its four helper types, `Sched`, `Spawn`, `Approver` and its three, `TestContext`, `map_entry_limit`, `map_probe_limit`) to `std.*` modules. Add what an agent expects (`Ord`, `Option` and `List` methods, a `Duration`), under a versioned prelude with a test that every name is documented.

## 1. What is in the prelude today

- The prelude is `crates/oska-check/src/prelude.osk` (1291 lines). Its header says it holds the built-in capabilities, `Option`, `Result`, `List`, the `String` methods, the types tests and concurrency use, and the core traits (`crates/oska-check/src/prelude.osk:2-5`). The primitives (`Int`, `I8` to `U64`, `Float`, `Bool`, `String`) and `()` are built into the checker (`crates/oska-check/src/prelude.rs:11-15`). `Untrusted<T>` is a trust flag, not a declared type (`docs/adr/0006-trust-is-a-flag-on-text.md:5`).
- Declared names, from the `pub` items of `prelude.osk`:
  - Capabilities (lines 38-595): `Stdin Stdout Stderr Fs FsRead FsWrite Dir DirRead DirWrite FileRead FileWrite Net NetConnect HostConnect Connection NetListen LoopbackListen Env Clock Random Budget Sched Spawn`.
  - Secrets: `Secret` (line 350).
  - Budget helpers: `Reservation Allowance Spending OverBudget` (lines 547-578).
  - Approval: `Approver Approval ApprovalRequest ApprovalError` (lines 604-663).
  - Tests: `TestContext` (line 687).
  - Core data: `Option Result List Bytes Range Path Args IoError` (lines 731-997).
  - Concurrency: `Scope Task` (lines 1178-1187).
  - Functions: `dbg todo map_entry_limit map_probe_limit` (lines 1197-1210).
  - Traits: `Eq Hash Display From` (lines 1215-1288).
- Two more names come from `std.map`: `Map` and `Entry` (`crates/oska-check/src/prelude.rs:203-209`).
- Display is implemented only for `String`, `Int`, `Float`, `IoError`, `OverBudget` and `ApprovalError` (`prelude.osk:580, 665, 999, 1050, 1278, 1282`). `Bool` has none.
- Inference: about 55 type, trait and function names are in every program's namespace, and most are capability types.

### What each costs

- **Name collisions.** A user type or function that redeclares a prelude name gets `E0202` (`docs/checker.md:758`; `crates/oska-check/src/coherence.rs:3-4`). A user `impl` block on a prelude type is `E0291` (`crates/oska-check/src/coherence.rs:9-10`). Inference: the risky bare names are `Entry`, `Map`, `Path`, `Range`, `Task`, `Scope`, `Args`, `Env`, `Clock`, `Random`, `Budget`, `Connection`, `Approval`, `Allowance`, `Spending`, `Secret`. Domain code often wants these (a cache `Entry`, a game `Map`, a billing `Budget`, a config `Env`).
- **Compile time.** The prelude parses once (`crates/oska-check/src/prelude.rs:289`, `OnceLock`). `Map` is Oska source (431 lines) joined to a program only when its text writes `Map` or `Entry` (`prelude.rs:203-209`). The test is a plain text search that "errs toward yes": a comment or string that says `Map` pulls it in (`prelude.rs:224-235`). Inference: the cost is small and bounded, but the trigger is imprecise.
- **Interpreter and native parity.** `Map` is Oska code run by both backends, and only the two limit panics are prelude functions (`docs/adr/0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md:38`). Body-less prelude functions are implemented by the runtime and so need parity tests; `Map` is covered by the differential suite (ADR 0028, line 30).
- **Authority.** A capability is a value; "Only `main` receives the built-in ones... and no code can construct one" (`prelude.osk:8-12`). The GLOSSARY says Oska has no ambient authority (`GLOSSARY.md:11-12`). So capability types in the prelude grant nothing. Inference: the cost is namespace and cognitive load, not authority. One exception to check: `Approver.terminal(stdin, stderr)` is a public constructor (`prelude.osk:612`), but it needs two capabilities to call.
- **Trust.** Every `String` method follows trust (`prelude.osk:887-960`); `Map` keeps the trust flag of its keys (ADR 0028, line 14). A hash is a plain `Int` with no trust flag, accepted on 2026-11-26 (ADR 0028, line 42).
- **Dead names.** `Sched` and `Spawn` have no runtime: `E0370` (`prelude.osk:586-595`; `gh api repos/aparsons/oska/issues/661`, open, says `oskac run` refuses `Spawn`).

## 2. Map and Entry

### What the repo decided

- Map and Entry went into the prelude on 2026-11-26; a user `Map` or `Entry` is `E0202` (ADR 0028, line 38).
- The hash is a fixed, unseeded FNV-1a. A seeded or random hash was rejected because it breaks the same-output rule (ADR 0028, lines 19, 25, 42). The defence is a probe limit of 64, a rebuild under the next fixed hash, then panic `P0045`, plus an entry limit that panics `P0044` (ADR 0028, lines 11-12, 40-41).
- ADR 0028 says the flood defence "is the weakest part of this design" and is decided by an attack test (line 32).

### How other languages place their map

- Rust: `HashMap` is not in the prelude. The prelude is "kept as small as possible" and holds items "used in almost every single Rust program" ([std::prelude](https://doc.rust-lang.org/std/prelude/index.html)). It does hold `Vec`, `String`, `Option`, `Result`. The Rust map page imports it by `use` in its examples and uses a randomly seeded hasher "selected to provide resistance against HashDoS" ([HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html)). I did not find a statement on the page that `HashMap` is outside the prelude; that follows from the prelude's list.
- Go: `map` is a language construct, and `builtin` lists the predeclared names (`make`, `delete`, `len`, `clear`, `append`...) ([builtin](https://pkg.go.dev/builtin)).
- Python: `dict` is a built-in, in a list of 73 entries that also contains `eval`, `exec`, `open`, `compile` and `input`. The docs warn that `eval` on untrusted input "will lead to security vulnerabilities" and that replacing `__builtins__` "is not a security mechanism" ([builtins](https://docs.python.org/3/library/functions.html)). Python seeds `str` and `bytes` hashes randomly by default against "a denial-of-service caused by carefully chosen inputs" with O(n^2) dict construction ([cmdline](https://docs.python.org/3/using/cmdline.html#envvar-PYTHONHASHSEED)).
- Gleam: `Int`, `String`, `List`, `Bool`, `Result` need no import, but `Dict` is in `gleam/dict` and `Option` in `gleam/option`, both imported ([Gleam tour](https://tour.gleam.run/everything/)).
- Zig: nothing is implicit; `const std = @import("std");` ([Zig docs](https://ziglang.org/documentation/master/#import)).
- HashDoS is a known class: CWE-407 cites CVEs of "CPU consumption via inputs that cause many hash table collisions" ([CWE-407](https://cwe.mitre.org/data/definitions/407.html)).

### Tradeoffs

- **For keeping `Map` in.**
  - Discoverability: an agent writes `Map.new()` first and fixes later. #660's problem statement is that each missed method "costs a compile-and-fix turn" (`gh api repos/aparsons/oska/issues/660`, open).
  - Token cost: one `use std.map` line saved per program, minor.
  - Python, Go and Gleam's built-in set all give key-value tables a short path (Gleam via a one-line import).
  - Inference: of the dictionary-like types in the languages read, only Rust and Zig leave the map out, and Rust still has `Vec`.
- **For taking it out.** Rust's per-edition list shows that adding a trait later is the breaking case, while adding a type is not, because "manually imported items take priority over prelude items" ([2021 prelude](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html)). Oska chose the opposite for types: a user type of the same name is `E0202`. Inference: so in Oska adding a type name to the prelude is breaking, and removing it later is not. The names are asymmetric: leaving a name out can be reversed without breakage; putting a name in cannot.
- **`Entry` is the weak name.** Inference: `Entry` is a common domain word and an agent rarely writes it, since `entries()` returns it and `Map.of` takes a list of it (`crates/oska-check/src/std/map.osk:67, 111, 186`). It costs a name collision for little discoverability.
- **HashDoS.** A fixed unseeded hash is attackable by design. Rust and Python randomize ([HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html); [PYTHONHASHSEED](https://docs.python.org/3/using/cmdline.html#envvar-PYTHONHASHSEED)). Oska bounds the damage instead: at most 64 probes per lookup, then a panic (ADR 0028, lines 11, 40). Inference: that turns a CPU-exhaustion attack (CWE-407) into a clean crash, which is an availability bug, not a hang. It is safe for step-limited runs (ADR 0021) and deterministic, but a server that parses attacker-chosen keys can be made to panic on purpose. Putting `Map` in the prelude does not change the hash, but it raises how often untrusted keys reach it. Inference: `std.json` and `Value.as_map()` (ADR 0028, line 44) are the main entry point, so the flood test over JSON keys matters more than the prelude question.

### Recommendation (Map and Entry)

1. Inference: keep `Map` in the prelude.
2. Inference: before 1.0, change the rule for prelude type names so a user declaration shadows them (as Rust's imports win over the prelude), or keep `E0202` but rename `Entry` to a longer name such as `MapEntry`, or make it reachable only as `Map.Entry`. Either avoids an unremovable collision.
3. Inference: document the HashDoS limit in the `Map` API text (a program with attacker-chosen keys can panic with `P0045`) and run the 100,000-key test ADR 0028 asks for (line 32), including keys that share a 64-bit FNV-1a hash.

## 3. What else should leave the prelude

Inference, ranked by value:

1. `map_entry_limit` and `map_probe_limit` (`prelude.osk:1207-1210`). They are internal ("For `std.map` only") but take two free-function names from every user. Make them intrinsics only `std.map` can name.
2. `Budget`, `Reservation`, `Allowance`, `Spending`, `OverBudget` (`prelude.osk:523-578`). They are for `std.llm`, which does not exist in the module table (`crates/oska-check/src/prelude.rs:47-201`). Move to `std.llm` or `std.budget`.
3. `Sched` and `Spawn` (`prelude.osk:586-595`). Both have no runtime. `std.proc` already takes `Spawn`. Move `Spawn` to `std.proc`.
4. `TestContext` (`prelude.osk:687`). Tests receive `t` as a built-in value, so the type name need not be bare.
5. `Approver`, `Approval`, `ApprovalRequest`, `ApprovalError` (`prelude.osk:604-663`), to a `std.approval` module. Unlike the others they hold no authority a program lacks, but they are used by tool-calling code only.
6. Keep `Stdin Stdout Stderr Fs FsRead FsWrite Dir* File* Net* Env Clock Random Secret Option Result List Bytes Path IoError` and the four traits. Inference: they are what an agent writes in `main`'s signature, as `prelude_names_resolve_without_imports` shows (`crates/oska-check/tests/names_and_authority.rs:162-171`). They hold no ambient authority, because only `main` receives them (`prelude.osk:8-12`).
7. Compare: Python's `open`, `eval`, `exec` and `input` are always available and are ambient authority, and Python itself says restricting builtins is not a security mechanism ([builtins](https://docs.python.org/3/library/functions.html)). Deno gives no file, network or environment access unless granted ([Deno security](https://docs.deno.com/runtime/fundamentals/security/)); WASI programs "can be provided with capabilities" and have no built-in I/O ([WASI](https://github.com/WebAssembly/WASI/blob/main/docs/Capabilities.md)). Oska's prelude already matches the capability stance; the work is trimming names, not authority.

## 4. What is missing

Inference, ranked:

1. `Ord` and `Ordering`, and `<` on text. #660 lists `[3,1,2].sorted()`, `"a" < "b"` (`E0223`) and `Some(1).map(...)` as failing today, and says `Option` has only `ok_or`, `is_some`, `unwrap_or`, and `List` only `new`, `push`, `len`, `count`, `get`, `at`, `map` (`gh api repos/aparsons/oska/issues/660`; confirmed by `prelude.osk:736-781`). This is the largest gap for agents. #660 also wants no `Ord` for `Secret` and none for `Float` (it points to `total_cmp`; `prelude.osk:1036`).
2. `Option`, `Result`, `List` and `String` methods in #660 (`map`, `and_then`, `filter`, `fold`, `sorted`, `contains`, `to_uppercase`, `split_once`). Read the exact list in the issue.
3. `Display` for `Bool`, which has none today (`prelude.osk:1272-1285`).
4. A time type. `Clock` returns only `now_millis(): Int` (`prelude.osk:498-501`). Add `Duration` and a `Timestamp` with checked arithmetic, kept in `std.time`, not bare.
5. A common error convention. `IoError` is the only error type besides two special ones (`prelude.osk:997`). `From<T>` exists (`prelude.osk:1288`), so a `std.error` can build on it. Inference: keep it out of the bare prelude until `?` conversion rules settle.
6. Process, SQL and extra file operations are specified, not built: #661 (`gh api repos/aparsons/oska/issues/661`, open). They stay in `std.*` (`std.proc`, `std.sql`), which the prelude should not absorb, because they carry authority (`Spawn`, `NetConnect`).
7. `Float` math is `std.math`, an import (`crates/oska-check/src/prelude.rs:189-194`); #590 is closed (`gh api repos/aparsons/oska/issues/590`). Keep it so; only the methods of `Float` itself are bare (`prelude.osk:1014-1042`).
8. Classification names. ADR 0029 adds a `Declassify` capability and four labels (ADR 0029, lines 5, 10). Inference: only `Declassify` as a capability type needs a bare name, like `Secret`.
9. `print`, `log` and `assert` rules. Output goes only through `Stdout` and `Stderr` capabilities (`prelude.osk:46-63`); `t.assert` is on the test context (`prelude.osk:699`); `dbg` is "an error under `--release`" (`prelude.osk:1196`). Go's `print` and `println` exist as built-ins ([builtin](https://pkg.go.dev/builtin)). Inference: Oska is right to have no bare `print`, because it would be ambient authority; keep it so and say so in the prelude header.
10. `Trusted` text and `Bytes`. Not missing: trust is a flag on `String` and `Bytes` (`prelude.osk:14-18`; ADR 0006, line 5), and `Bytes` is already in the prelude (`prelude.osk:804`).

## 5. Principles for an AI-first, secure-by-default prelude

- **No ambient authority.** Already true; add a test that no bare name is a function that does I/O (`docs/adr/0002-capabilities-are-the-effect-system.md`; `GLOSSARY.md:11-12`). Sources: [Deno](https://docs.deno.com/runtime/fundamentals/security/), [WASI](https://github.com/WebAssembly/WASI/blob/main/docs/Capabilities.md), and Python's counterexample ([builtins](https://docs.python.org/3/library/functions.html)).
- **Small, and used in almost every program.** This is Rust's stated rule ([std::prelude](https://doc.rust-lang.org/std/prelude/index.html)). Inference: Oska should state the same rule in the `prelude.osk` header and apply it to the list in section 3.
- **Versioned.** Rust gates additions by edition because a new trait can break callers ([2021 prelude](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html)). Oska's manifest has `name` and `version` but no language-edition field (`crates/oska-check/src/manifest.rs:15, 42`; I saw no edition field there). Inference: add a `prelude` or `edition` key before the prelude is frozen, because `E0202` makes every added name breaking.
- **Predictable names.** #660's purpose is that agents reach for names from Python, Rust and TypeScript (`gh api repos/aparsons/oska/issues/660`). Inference: use the common name (`sorted`, `filter`, `to_uppercase`), and make a known-but-missing name such as `unwrap` fail with a message naming the Oska way, which #660 plans.
- **Every name documented and tested.** The comment above each `pub` item becomes the API reference (`prelude.osk:30-32`). Inference: add a test that fails if a bare name has no comment, no example and no entry in the differential suite, and keep `prelude_names_resolve_without_imports` complete (`crates/oska-check/tests/names_and_authority.rs:162`).
- **Same result everywhere.** Prelude behaviour is pinned to be identical on the interpreter and native code (ADR 0014, ADR 0027, ADR 0028 line 19). Inference: any new bare name needs a differential test before it ships.
- **No hidden surprises on untrusted input.** Parse functions refuse special values by default (`prelude.osk:943-956`); `Map` limits entries and probes. Inference: a bare name should have a documented limit and a stable panic code wherever input size or key choice is attacker-controlled.

## 6. Ranked recommendations

1. Keep `Map` in the prelude (Inference; see section 2).
2. Fix the `E0202` rule for prelude type names or rename `Entry` before 1.0 (Inference, from the asymmetry in [Rust's edition guide](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html)).
3. Ship #660 (`Ord`, `Ordering`, `Option`, `Result`, `List`, `String` methods): sourced as the largest agent-facing gap (`gh api repos/aparsons/oska/issues/660`).
4. Hide `map_entry_limit` and `map_probe_limit` (Inference).
5. Move `Budget` and its helpers, `Sched`, `Spawn`, `TestContext` and the approval types to `std.*` (Inference).
6. Add a language or prelude edition field to the manifest (Inference, from Rust).
7. Document and test the HashDoS limit; run the 100,000-key attack test (sourced: ADR 0028 line 32; CWE-407; Inference on the wording).
8. Add `Display for Bool`, a `Duration` in `std.time`, and a `std.error` (Inference).
9. Test that every bare name is documented, covered by the differential suite and holds no authority (Inference).
10. Make the `Map` auto-join trigger exact (a resolved name, not a text match), so a comment does not add 431 lines (Inference; `prelude.rs:224-235`).

## Sources

- [Rust std::prelude](https://doc.rust-lang.org/std/prelude/index.html)
- [Rust 2021 prelude changes](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html)
- [Rust HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html)
- [Go builtin](https://pkg.go.dev/builtin)
- [Python built-in functions](https://docs.python.org/3/library/functions.html)
- [Python PYTHONHASHSEED and -R](https://docs.python.org/3/using/cmdline.html#envvar-PYTHONHASHSEED)
- [Gleam language tour](https://tour.gleam.run/everything/)
- [Zig documentation](https://ziglang.org/documentation/master/#import)
- [Deno security](https://docs.deno.com/runtime/fundamentals/security/)
- [WASI capabilities](https://github.com/WebAssembly/WASI/blob/main/docs/Capabilities.md)
- [CWE-407](https://cwe.mitre.org/data/definitions/407.html)
- Repo: `crates/oska-check/src/prelude.osk`, `crates/oska-check/src/prelude.rs`, `crates/oska-check/src/std/map.osk`, `crates/oska-check/src/coherence.rs`, `crates/oska-check/src/manifest.rs`, `crates/oska-check/tests/names_and_authority.rs`, `docs/checker.md`, `GLOSSARY.md`, `docs/adr/0001`, `0006`, `0008`, `0025`, `0028`, `0029`; issues #660, #661, #590 (read with `gh api`).
