# Float results are pinned by the spec on every platform

## Decision

Oska has one `Float` type, IEEE 754 binary64. The spec fixes every result of every `Float` operation, down to the bits. The interpreter and the native backend on each supported platform give the same bits for the same program.

**Rounding.** Round to nearest, ties to even, and nothing else. There is no rounding-mode state, no sticky flag and no fast-math switch.

**NaN.** There is one canonical NaN. Every operation that makes a NaN makes the canonical one, and `to_bits` returns its pattern. `==` and `<` follow IEEE 754, so a NaN is not equal to itself. `total_cmp` is a total order and is the way to sort floats.

**No contraction.** The compiler never turns `a * b + c` into one fused operation. `fma(a, b, c)` is a separate, explicit function that rounds once. A program chooses fusion by naming it.

**Exact operations are native.** IEEE 754 requires `+`, `-`, `*`, `/` and `sqrt` to be correctly rounded. Both code generators emit the hardware instruction for each (SSE2 on x86_64, the FP unit on aarch64). The interpreter uses the host's IEEE operation, with the same rules.

**Everything else is Oska.** `exp`, `ln`, `sin`, `cos`, `pow`, `atan2` and the other `std.math` functions are written in Oska, one fdlibm-style algorithm each. No result depends on the C library, the compiler or the processor.

**Two accuracy tiers.** Adam decided this on 2026-11-22 (comment on spec #590). `std.math` offers two tiers side by side:

- The **fast tier** is within 1 ulp. The docs state the bound per function. The plain name is the fast tier: `sin`.
- The **exact tier** is correctly rounded: always the nearest double. It is a separate function with a distinct name: `sin_exact`. A program names which promise it relies on, and an agent cannot pick the exact tier by accident.

Both tiers give the same bits on every platform and backend. The exact tier must match a high-precision reference on every input of the test set with no tolerance, including the known hard-to-round cases. Each exact function has a stated fast path, a slow path and an error proof for both, written in the docs. A later SIMD fast path must return the same bits as its scalar version, in both tiers.

**Steps.** A loop in a `std.math` function counts steps as ADR 0021 says, so `--max-steps` stops a float-heavy program at the same step on both backends. A kernel inside the runtime would need to charge steps in proportion to its work, so the library is written in Oska instead.

**Conversions and text.** There is no implicit `Int` to `Float` conversion. `to_int()` panics on NaN, infinity or a value out of range. `to_string` prints the shortest digits that parse back to the same value. `parse_float` is correctly rounded and rejects `NaN`, `inf`, a leading `+` and over-long input unless the caller opts in. The spec for each ticket gives the exact rules.

**Secrets.** `Secret<Float>` is refused by the checker, because float arithmetic is not constant-time on common hardware (subnormals).

## Why

Other languages leave float results open: Rust calls `sin` and `exp` non-deterministic, Go does not promise bit-identical results across architectures, and JavaScript engines differ by operating system. Oska promises the same output on every platform and on both backends ([ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md), [ADR 0021](0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)). A float type that broke the promise would be worse than none: an agent could not tell which answer is right, and a verifier and an executor could disagree on a numeric contract.

The native backend has no outside assembler, linker or C library ([ADR 0011](0011-oska-generates-native-code-itself.md)), so there is no libm to call. Writing the functions in Oska is the choice that fits that rule, and it is also what makes them the same on each platform. The interpreter stays the reference, and `Float` enters the Lean model before it ships ([ADR 0018](0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md)).

## Considered options

- **Call the platform libm.** Rejected: results differ by platform and version, and the native backend has no C library.
- **One tier, 1 ulp.** Rejected by Adam: some programs need the nearest double and must be able to say so.
- **One tier, correctly rounded.** Rejected: it is slower for every call, and most programs do not need it.
- **Allow fused multiply-add when the hardware has it.** Rejected: the same source would give different bits on x86_64 and aarch64.

## Consequences

`std.math` is larger and slower than a libm binding. The first ticket of the float functions measures the speed before the rest of them are written. The differential suite gains golden vectors for every function on both backends and every platform. Every exact function needs an error proof in the docs. A change to a pinned result is a breaking change and needs a new ADR.
