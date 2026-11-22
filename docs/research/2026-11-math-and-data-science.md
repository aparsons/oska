# Which math and data science features should Oska build, and in what order?

- **Date of access:** 2026-11-22
- **Scope:** what the main math and data libraries provide, which parts belong in a language's standard library, and what Oska should build for an AI-first, secure-by-default language. Not a spec. Language changes are frozen until #181 ([ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)), so this ranks work for after the gate.
- **Method:** 16 pages read in full or in part, from the 20-source cap. Four fetches returned only a redirect, a script shell or a robots block (see "Open questions"), and I cite nothing from them. Repo facts come from the ADRs, `docs/design.md` and the four notes this one builds on. Pages held no text addressed to an AI reader.
- **Not repeated here:** why Oska needs floats and a map for code mode ([code-mode-changes, item 15](2026-11-code-mode-changes.md)), what agents do in throwaway scripts ([agent-scripting note](2026-11-agent-scripting-python-vs-oska.md)), the design foundations ([AI-first language note](2026-11-ai-first-secure-language.md)), the gap list for replacing Rust ([replacing-Rust note](2026-11-oska-replacing-rust.md)), and how a strict parser and limits work ([JSON note](2026-11-json-parser.md)). This note reuses their method: strict by default, every limit on, stable codes.

## Short answer

1. Build a `Float` type first, with IEEE 754 binary64 results pinned on every backend: no fused multiply-add unless asked, one NaN, transcendental functions written in Oska so they cannot differ.
2. Then a deterministic map, `std.math`, `std.stats` and a seeded `Rng` that is a plain value, apart from the `Random` capability.
3. Then `std.csv` through `DirRead` and `FileRead`, with cells as `Untrusted<String>`, then `Decimal` and `BigInt`.
4. A small typed `Table` comes next. Dense matrices come last, and only with a fixed summation order.
5. Do not build Parquet, Arrow, BLAS bindings, tensors with autodiff, or plotting yet. Each one breaks determinism, the no-linker rule, or the attack surface budget.

## 1. What the libraries are, and what is used most

### Usage evidence

- NumPy had 1,116,262,387 PyPI downloads in the last month, and 268,547,506 in the last week ([pypistats, numpy](https://pypistats.org/packages/numpy), read 2026-11-22).
- The Stack Overflow 2025 survey page I could read lists only Polars, at 3.8% of all respondents as a recent tag ([Stack Overflow Developer Survey 2025, technology](https://survey.stackoverflow.co/2025/technology)). The text I got held no figure for pandas, NumPy or PyTorch, so I give none. I did not reach the JetBrains Python survey.
- nalgebra has 104,289,254 downloads on crates.io ([crates.io, category mathematics](https://crates.io/api/v1/crates?category=mathematics&sort=downloads&per_page=15)). I found no download figures for ndarray, Polars, statrs, linfa or burn in my sources.

Inference: NumPy's count is mostly dependency installs, not people writing numerical code. It shows that array math is the base layer that other packages need, not how many people use it directly.

### What the libraries provide

I checked the first-party pages named in the source list. The rest of this table is general knowledge, marked "(unchecked)", so treat it as a map, not a claim.

| Library | What it provides | Checked against |
| --- | --- | --- |
| NumPy | N-dimensional arrays, vector math, linear algebra through the BLAS and LAPACK that it links to, and random generators | [NumPy random compatibility](https://numpy.org/doc/stable/reference/random/compatibility.html) names the linked LAPACK and says builds differ |
| SciPy, scikit-learn, statsmodels | Special functions, optimization, statistics, models (unchecked) | none |
| pandas, Polars | Tables with typed columns, group-by, joins, CSV and Parquet readers (unchecked) | Polars appears in the survey above |
| PyTorch, burn | Tensors with autodiff and GPU kernels (unchecked) | none |
| R, Julia | Statistics and arrays in the base language (unchecked) | none |
| ndarray, nalgebra, statrs, linfa | Rust arrays, linear algebra, distributions, classical ML (unchecked, except nalgebra) | [crates.io](https://crates.io/api/v1/crates?category=mathematics&sort=downloads&per_page=15): "a general-purpose linear algebra library with transformations and statically or dynamically sized matrices" |
| BLAS and LAPACK | A fixed interface for dense linear algebra, with many implementations (unchecked) | NumPy page: a different LAPACK build can give "completely different (but equally valid!) results" for `multivariate_normal` |
| Apache Arrow | A columnar memory layout for sharing data without copying: validity bitmaps, aligned buffers, little-endian IPC | [Arrow columnar format](https://arrow.apache.org/docs/format/Columnar.html) |
| Parquet | A columnar file format (unchecked) | none |

Inference: the features used by most people are few. Reading a file into a table, filtering, grouping, summing, mean, standard deviation, sorting, joining and printing a result. The deep parts (BLAS, autodiff, GPU) serve a smaller group.

### What other languages ship in their standard library

This is the table that decides what belongs in `std`. "Std" means in the language's own standard library or base class library.

| Language | In std | Third party | Lesson for Oska |
| --- | --- | --- | --- |
| Go | `math` with trig, exp, log, rounding, `Nextafter`, `Gamma`, `Erf`, and FMA since Go 1.14; `math/big`; `math/bits`; `math/cmplx`; `math/rand` ([pkg.go.dev/math](https://pkg.go.dev/math)) | gonum for matrices and statistics (unchecked) | Std holds scalar math and big numbers. Its docs say: "This package does not guarantee bit-identical results across architectures." |
| JavaScript | `Math` on `Number` only, plus `BigInt`, `Intl` ([MDN Math](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Math)) | d3, simple-statistics, TensorFlow.js (unchecked) | MDN: "Even the same JavaScript engine on a different OS or architecture can give different results!" |
| Java | `Math` with `addExact`, `multiplyExact`, `floorMod`, `fma`; `StrictMath`; `BigDecimal` ([Javadoc, Math](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/Math.html)) | Apache Commons Math: statistics, linear algebra, optimization, ODEs, distributions ([Commons Math](https://commons.apache.org/proper/commons-math/)); Tablesaw (unchecked) | Checked integer ops are in std and throw on overflow. Math is allowed 1 to 2.5 ulps of error, and `Math` is not bit-for-bit across implementations. `StrictMath` exists for the case where it must be. |
| C and C++ | `<cmath>` with special functions, `<numeric>`, `<complex>`, `<random>`, `<valarray>`; C++26 adds `<linalg>` (BLAS-style) and checked and saturating arithmetic ([cppreference, numerics](https://en.cppreference.com/w/cpp/numeric)) | Eigen, Boost.Math (unchecked) | No matrix type and no statistics in std. Checked and saturating arithmetic arrived only in C++26. |
| .NET | 8 to 128-bit integers, `Half`, `Decimal`, `BigInteger`, `Complex`, `Matrix3x2`, `Matrix4x4`, `Vector<T>` ([.NET numerics](https://learn.microsoft.com/en-us/dotnet/standard/numerics)) | `System.Numerics.Tensors` is a NuGet package; ML.NET (unchecked) | Std holds exact decimals and big integers. Tensors are a package. |
| Swift | Accelerate is a system framework for "large-scale mathematical computations ... optimized for high performance and low energy consumption" ([Apple, Accelerate](https://developer.apple.com/documentation/accelerate)) | none checked | The platform supplies BLAS and DSP. Oska cannot assume a platform library on five targets (ADR 0014). |
| Python | `decimal` with exact base-10 values, a context, eight rounding modes and trap flags ([decimal](https://docs.python.org/3/library/decimal.html)); `random`, `statistics`, `fractions` (unchecked) | NumPy, pandas and the rest | `Decimal(0.1)` captures the binary float, so build from text or integers. |
| Rust | `f64` with `mul_add`, `sqrt` and rounding functions specified ([std f64](https://doc.rust-lang.org/std/primitive.f64.html)) | ndarray, nalgebra, statrs, linfa, burn, Polars (names unchecked) | Std is small and precise. Numerics live in crates. |
| SQL and DuckDB | The data baseline: queries over tables | none | Not checked: both DuckDB pages I fetched were redirects. |

Inference: every language puts scalar math, big integers and (in some) exact decimals in std, and puts arrays, tables and statistics in packages. The two exceptions to the "scalar only" pattern are Go's `math/big` and .NET's `Decimal`, which are exact types, not approximate ones. Oska has no package ecosystem yet ([ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md) gives git packages), so a package-only approach would leave a gap. But a std module is a promise to keep stable, so Oska should keep std small: scalar math, exact types, CSV, and the smallest stats.

## 2. What Oska has now

- `Int` is 64-bit signed and overflow panics with `P0001`. Sized integer types convert only explicitly, with checked and wrapping forms ([design.md](../design.md)).
- No float: `let x = 1.5` fails with `E0101`. No map type ([agent-scripting note, section 2](2026-11-agent-scripting-python-vs-oska.md); [code-mode-changes, item 15](2026-11-code-mode-changes.md)).
- `Random` is a capability with `next_int()` and a built-in pseudo-random generator. Tests get a seeded `t.random` ([design.md](../design.md), [testing guide](../guide/testing.md)).
- `Secret<U8>`, `Secret<U32>`, `Secret<U64>`, `Secret<Bool>` and `Secret<Bytes>` exist, with wrapping arithmetic and `constant_time_eq` (design.md). `Bytes` and wrapping ops are the first work under the parity gate, in v0.6.0.
- `std` modules: json, sql, sha2, hmac, ed25519, html, proc, mcp, rt, and sshsig. `std.json` keeps number text, so it needs no float ([JSON note](2026-11-json-parser.md), #229).
- Files are read only through confined capabilities, `DirRead` and `FileRead` ([ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)). Text read from them is `Untrusted<String>`, and parsing it to a non-text type makes the result trusted ([ADR 0006](../adr/0006-trust-is-a-flag-on-text.md)).
- A step is one call or one loop back-edge, counted the same on both backends ([ADR 0021](../adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)).
- The runtime and crypto are written in Oska, with only `oska-sys` allowed `unsafe` and no outside linker ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md), [0011](../adr/0011-oska-generates-native-code-itself.md), [0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)).

## 3. Proposed features, ranked

Cost: S is a spec and a few days of work, M is one spec with its own tests across both backends, L is several specs. All of these are new language or `std` surface, so all wait for #181 and the parity gate.

| Rank | Feature | Value | Cost | Depends on | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `Float` (binary64) with pinned semantics | Very high: every tool result with a price or score, and JSON numbers with a fraction | M | ADR 0014 gate; a new ADR; [item 15](2026-11-code-mode-changes.md); the Lean model in `formal/` | Rules in section 4. Without it, nothing below works. |
| 2 | Deterministic map, `Map<K, V>` | Very high: JSON objects, counting, group-by | M | A new ADR; strict key types | Iterate in insertion order, the same on every backend. No hash-order leaks. |
| 3 | `std.math`: integer helpers (`gcd`, `isqrt`, `pow` checked, `floor_div`, `rem_euclid`), float functions | High | S to M | Rank 1 | Write in Oska. See the libm rule in section 4. |
| 4 | `std.stats` over `List<Float>`: sum, mean, variance, median, quantile, min and max, histogram | High: what agents compute most in data tasks (Inference) | S | Ranks 1 and 3 | One fixed summation method. Contracts reject empty and NaN input. |
| 5 | `Rng`: a seeded generator that is a plain value | High for tests, simulation and sampling | S | Rank 1; ADR 0002 | Pure and deterministic with a documented algorithm. `Random` stays the capability for entropy. |
| 6 | `std.csv` | High: the common file format for agents | M | Ranks 1 and 2; ADR 0008, 0006; limits as in the JSON note | Strict, with every limit on. Returns cells as `Untrusted<String>`. |
| 7 | `Decimal` (fixed scale, money) | Medium to high: money and reports | M | A checked big-integer core | Build from text or integers only. Rounding is explicit. |
| 8 | `BigInt` | Medium: crypto, exact counting, JSON integers above 2^53 | M | ADR 0012; Bytes milestone | Constant-time `Secret` big arithmetic is a separate, later item. |
| 9 | `Table`, a small typed table | High, but only after 2, 6, 4 | L | Ranks 2, 4, 6 | `select`, `filter`, `sort_by`, `group_by`, `sum`, `join` on a key. No lazy engine. |
| 10 | `Matrix<Float>`, dense and small | Medium | L | Ranks 1, 3; step accounting | `matmul`, `transpose`, `solve`. Fixed loop order. Written in Oska. |
| 11 | Units | Low to medium | M | Rank 1 | A package, not std. |
| 12 | `Float32`, complex numbers | Low | M | Rank 1 | Later. |
| NOT | Parquet and Arrow readers | Medium | L | none | Not first, see below. |
| NOT | BLAS or LAPACK bindings | Low here | L | none | Breaks ADR 0011 and determinism. |
| NOT | Tensors with autodiff, GPU kernels | Low here | L | none | An AI model belongs outside the language. |
| NOT | Plotting | Low | L | none | An agent can write an SVG as text. |

### What to build first, and what not to build

Build rank 1, then 2, then 3 to 5 as one batch, then 6. The first four give a real agent snippet: read a CSV, compute a mean and a group count, print. Everything else can wait for feedback from those snippets.

Do not build these yet:

- **Parquet and Arrow.** The Arrow security page says invalid data "may therefore cause invalid memory accesses" and that a crafted IPC file "acts as a decompression bomb" ([Arrow security](https://arrow.apache.org/docs/format/Security.html)). Parquet needs several decompression codecs on top. Each one is a parser for untrusted input in Oska, inside the trusted base ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md) means writing them ourselves). Inference: ship CSV and JSON Lines first, then a bounded Arrow IPC reader once limits and fuzzing are in place, then Parquet if users ask.
- **BLAS and LAPACK.** There is no external linker ([ADR 0011](../adr/0011-oska-generates-native-code-itself.md)). A linked BLAS also differs by build: NumPy's own page says a different LAPACK can give "completely different (but equally valid!) results".
- **Tensors, autodiff, GPUs.** They would need unsafe kernels, huge allocations and non-deterministic reductions. Calling an outside model is a job for `Net` and `std.mcp`.
- **Plotting.** No ecosystem benefit now. Text output an agent can read is better.
- **A big stats library** in the style of statsmodels or Commons Math (which spans optimization, ODEs and more). Leave it to packages.

## 4. What makes this AI-first and secure-by-default

### Deterministic floats

The aim is [ADR 0014](../adr/0014-every-supported-platform-builds-and-runs-natively.md)'s rule: byte-identical output on every platform and both backends. The sources show where other languages leave that open.

- **Transcendental functions differ.** Rust says the precision of `sin` and `exp` "is non-deterministic" and "varies by platform, Rust version, and can even differ within the same execution" ([Rust f64](https://doc.rust-lang.org/std/primitive.f64.html)). Go does not promise bit-identical results across architectures. JavaScript engines differ. Java allows 1 to 2.5 ulps and offers `StrictMath` for the strict case. Inference: `std.math` should implement `sin`, `exp`, `log`, `pow` and the rest in Oska, as one fixed algorithm, tested by the differential suite. That makes them identical by construction, which no libm can promise.
- **Exact operations may be native.** Rust guarantees `sqrt`, `mul_add` and rounding functions return the correctly rounded or exact result. Addition, subtraction, multiplication, division and `sqrt` are required by IEEE 754 to be correctly rounded, so both code generators can emit the hardware instruction. Inference.
- **No silent fused multiply-add.** Rust's `mul_add` page shows `a * b + c` and the fused form giving different results on the same inputs. WebAssembly has no implicit fusion: it defines `fma` as a separate operator that rounds once ([Wasm numerics](https://webassembly.github.io/spec/core/exec/numerics.html)). Inference: the compiler must never contract `a * b + c`, and `Float.fma` is explicit. The x86_64 and aarch64 backends then agree.
- **One NaN.** Rust's NaN payload and sign are arbitrary and "may change across Rust versions and target platforms". WebAssembly's deterministic profile produces only a "positive canonical NaN" ([Wasm numerics](https://webassembly.github.io/spec/core/exec/numerics.html)). Inference: Oska has one NaN value. `to_bits` returns the canonical pattern. `==` on NaN is false, as in IEEE, and a `total_cmp` gives a total order for sorting.
- **Round to nearest even only.** Wasm uses "round-to-nearest ties-to-even" and does not support directed rounding. Oska should copy that: no rounding-mode state, no sticky flags, no fast-math switch.
- **Printing and parsing are specified.** Output must be byte-identical, so `to_string` must name the algorithm (shortest digits that round-trip) and parse must be correctly rounded. Inference: these are two small specs, and both are testable against golden files.
- **Random is stream-stable by contract.** NumPy only promises the same stream "on the same build ... on the same machine", and says different CPUs "can cause differences in certain edge cases that cascade" ([NumPy random](https://numpy.org/doc/stable/reference/random/compatibility.html)). Oska can promise more: `Rng.seeded(n)` gives the same stream on every platform, in every release, because the algorithm is in the spec and the float conversion uses only exact operations. A change to the algorithm is a new named generator.
- **Reductions have one order.** `sum` adds left to right, or in one named tree, always. No parallel reduction in std.

### Resource-bounded compute

- **Kernels must charge steps.** ADR 0021 counts calls and loop back-edges. A runtime built-in such as `sort`, `matmul` or `parse_csv` is one call, so a loop inside it does no stepping, and a model-written snippet could burn minutes under `--max-steps 1000`. Inference: either write kernels in Oska (the JSON note and ADR 0012 already lean this way, and every inner loop then counts), or charge steps in proportion to the work.
- **Bound every allocation.** `Matrix` and `Table` sizes are computed with checked `Int` math, so `rows * cols` overflow gives `P0001` before any allocation. Add a per-run memory limit in the run-limits spec, and have `std.csv` take row, column, cell-length and total-size limits, on by default, as the JSON note does for `std.json`.
- **Exact types fail loudly.** `Int` overflow already panics. Float to `Int` conversion must be checked: NaN, infinity or out of range is an error, with a stable code. Java ships `addExact` and `toIntExact` for this, and C++26 only now adds `ckd_add` and friends, so Oska is already ahead on integers.

### Capability-confined data access and trust flags

- `std.csv` takes a `FileRead` or `DirRead`, never a path string, so the authority report shows which data a snippet can read ([ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)). The same rule applies to any later Arrow reader.
- Every cell is `Untrusted<String>`. `parse_float` and `parse_int` return trusted numbers ([ADR 0006](../adr/0006-trust-is-a-flag-on-text.md)), so the parse is the validation step, and it must reject `NaN`, `inf` and a leading `+` unless the caller opts in.
- A `Table` column that holds text keeps the untrusted flag, so a CSV cell cannot reach a shell command or an HTML page without a trust assertion. This matters for agents: a spreadsheet cell can hold an injected instruction.
- A Table built from text or a file can never write. Writing needs a `FileWrite`.
- No pickle-style formats. Do not ship any binary format that can run code on load.

### Secrets and constant time

- Floats are not constant-time on most hardware (subnormals can be slow), so there is no `Secret<Float>`. Inference: refuse it in the checker with a stable error.
- Secret arithmetic stays on `Secret` integers with wrapping ops. A constant-time `BigInt` for RSA-style work is a later item, written with the same rules, and ADR 0012 is the place to decide it.
- `Secret` values cannot flow into `std.stats` or print. That is already true of interpolation (design.md).

### Contracts and stable codes

- Numeric preconditions are what contracts are good at: `requires !xs.is_empty()`, `requires !x.is_nan()`, `requires n >= 0`, `ensures result >= 0.0`. `std.stats` should state them, so a prover or a runtime check reports the same stable code. Inference.
- New runtime errors need codes in the `P` series next to `P0001`. Candidates: float to int out of range, NaN where a number was required, empty input to a statistic, a table or matrix size over the limit, a CSV limit hit.
- An agent reads the code and the span, not an English message. That is the existing loop ([ai-driven-development note](2026-11-ai-driven-development.md)).

### What agents do in data and math tasks

Inference, because I found no primary measurement: in code mode an agent mostly reads tool JSON, filters and totals it, groups by a key, sorts, formats a number with fixed decimals, and sometimes reads a CSV. It almost never needs matrices. That points at ranks 1 to 6, and at number formatting (`format(x, 2)`) as a small item to add to rank 1. The agent-scripting note found 6 of 29 Python calls were JSON inspection and none needed a matrix, which fits.

## 5. Risks and open questions

- **Cost of the libm in Oska.** Writing correct `sin` and `exp` is real work. Port a documented algorithm (an fdlibm-style one) and test it against a high-precision reference. Open: does the project accept 1 ulp, or must it be correctly rounded? Java allows 1 to 2.5 ulps; correct rounding is slower and harder.
- **Speed.** A pure-Oska libm and a pinned summation order are slower than hardware libm and SIMD. Open: how much, and is it acceptable for agent snippets? Measure before deciding.
- **Map design.** Insertion order is easy to explain but costs memory. Hash maps need a fixed hash with no random seed. Open: key types, and whether Oska needs a sorted map as well.
- **Float equality.** Should `==` on `Float` be allowed at all, or only `approx_eq` and `total_cmp`? Many agent bugs come from `==`. Open.
- **`Float` and trust.** Does a float parsed from untrusted text need a flag? ADR 0006 says non-text results are trusted. NaN and infinity from untrusted text could still cause trouble downstream, so the parse must reject them by default.
- **Lean model.** `formal/` must grow floats so the interpreter stays the reference ([ADR 0018](../adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)). That is real work, and docs/agents/formal.md applies.
- **Open source gaps.** I did not read: DuckDB (both pages returned only a redirect), the glibc libm accuracy page (blocked by robots), NVD for the Parquet vulnerability CVE-2025-30065 (script shell only), the JetBrains Python survey, and primary pages for pandas, SciPy, scikit-learn, statsmodels, PyTorch, R, Julia, gonum, Eigen, Boost.Math, d3, Tablesaw, ML.NET, ndarray, statrs, linfa, burn and Parquet. Claims about them are marked "(unchecked)" or left out. The Apple Accelerate page gave only its meta description.

## First spec outline: `Float` and the numeric core

1. **Type.** `Float` is IEEE 754 binary64. Literals `1.5`, `2e-3`. No implicit `Int` to `Float` conversion.
2. **Arithmetic.** `+ - * /` and `sqrt` use round-to-nearest-even. No contraction, no fast-math, no rounding-mode state, no flags.
3. **NaN.** One canonical NaN. `==` and `<` follow IEEE. `total_cmp` orders everything. `to_bits` is canonical.
4. **Conversions.** `to_int()` is checked and panics with a new `P` code on NaN, infinity or out of range. `Int.to_float()` is checked for values above 2^53, or named `to_float_rounded()`.
5. **Text.** `to_string` prints the shortest round-trip digits. `parse_float` is correctly rounded and rejects `NaN`, `inf` and `+` by default. `fmt(x, digits)` for fixed decimals.
6. **`std.math`.** Written in Oska: `abs`, `floor`, `ceil`, `round_half_even`, `fma`, `exp`, `ln`, `sin`, `cos`, `atan2`, `pow`, `hypot`. The algorithm is named in the spec, with a stated error bound.
7. **Both backends.** Exact ops use hardware instructions on x86_64 and aarch64. The differential suite runs golden vectors, including subnormals, signed zero and the edges of every function.
8. **Limits.** Library loops count steps (ADR 0021). A kernel written in the runtime must charge steps in proportion to its work.
9. **Formal.** Add `Float` to the Lean model before it ships, and a TLA+ note if any float state is shared between tasks (ADR 0018, 0019).
10. **Then.** Map, `std.stats`, `Rng`, `std.csv`, in that order, each with its own spec and error codes.

## Sources

1. [Stack Overflow Developer Survey 2025, technology](https://survey.stackoverflow.co/2025/technology)
2. [pypistats, numpy](https://pypistats.org/packages/numpy)
3. [NumPy: random compatibility policy](https://numpy.org/doc/stable/reference/random/compatibility.html)
4. [Rust standard library: f64](https://doc.rust-lang.org/std/primitive.f64.html)
5. [Go: package math](https://pkg.go.dev/math)
6. [MDN: JavaScript Math](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Math)
7. [Java SE 21: java.lang.Math](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/Math.html)
8. [cppreference: C++ numerics library](https://en.cppreference.com/w/cpp/numeric)
9. [Apple: Accelerate](https://developer.apple.com/documentation/accelerate) (meta description only)
10. [Microsoft: numerics in .NET](https://learn.microsoft.com/en-us/dotnet/standard/numerics)
11. [Apache Arrow: columnar format](https://arrow.apache.org/docs/format/Columnar.html)
12. [Apache Arrow: security considerations](https://arrow.apache.org/docs/format/Security.html)
13. [WebAssembly core spec: numerics](https://webassembly.github.io/spec/core/exec/numerics.html)
14. [Python: decimal](https://docs.python.org/3/library/decimal.html)
15. [Apache Commons Math](https://commons.apache.org/proper/commons-math/)
16. [crates.io API: category mathematics](https://crates.io/api/v1/crates?category=mathematics&sort=downloads&per_page=15)
17. Fetched but unusable, nothing cited: [DuckDB data overview](https://duckdb.org/docs/stable/data/overview), [DuckDB CSV overview](https://duckdb.org/docs/stable/data/csv/overview.html), [NVD CVE-2025-30065](https://nvd.nist.gov/vuln/detail/CVE-2025-30065), [glibc manual, errors in math functions](https://www.gnu.org/software/libc/manual/html_node/Errors-in-Math-Functions.html)
