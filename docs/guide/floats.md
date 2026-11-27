# Floats and std.math

`Float` is IEEE 754 binary64, and Oska fixes every result of every `Float` operation down to the bit. The interpreter and the native backend give the same bits for the same program on every platform. This page shows the pinned rules, the conversions, the text forms, and the `std.math` functions. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints. The reasons are in [ADR 0027](../adr/0027-float-results-are-pinned-by-the-spec-on-every-platform.md), and the Lean model that the interpreter is checked against is in `formal/lean/OskaFormal/Float/`.

## One worked example

```oska
use std.math

fn main(io: Stdout) -> Result<(), IoError> {
    // Two roundings, then one: `fma` is a separate, named operation.
    let a = 0.1
    let b = 10.0
    let c = -1.0
    let split = a * b + c
    let fused = math.fma(a, b, c)
    io.println("a * b + c = {split}")?
    io.println("fma(a, b, c) = {fused}")?

    // One canonical NaN, whatever made it.
    let nan = 0.0 / 0.0
    let other = Float.from_bits(9218868437227405313)
    let nan_bits = nan.to_bits().to_int()
    let other_bits = other.to_bits().to_int()
    io.println("{nan_bits} {other_bits}")?
    if nan == nan { io.println("NaN equals itself")? } else { io.println("NaN is not equal to itself")? }

    // Conversions say what they do.
    let big = 9007199254740993
    let rounded = big.to_float_rounded()
    let cut = (-2.9).to_int()
    io.println("{rounded} {cut}")?

    // Text in and out.
    match "0.1".parse_float() {
        Some(x) => {
            let sum = x + 0.2
            let shown = x.fmt(3)
            io.println("{sum} {shown}")?
        }
        None => io.println("not a number")?
    }

    // The fast tier: plain names, within 1 ulp, the same bits everywhere.
    let s = math.sin(1.0)
    let p = math.powf(2.0, 0.5)
    let n = math.pow(2, 10)
    io.println("{s} {p} {n}")?
    Ok(())
}
```

```console cmd=run
a * b + c = 0.0
fma(a, b, c) = 5.551115123125783e-17
9221120237041090560 9221120237041090560
NaN is not equal to itself
9007199254740992.0 -2
0.30000000000000004 0.100
0.8414709848078965 1.414213562373095 1024
```

The sections below explain each line.

## The pinned rules

- **Round to nearest, ties to even, and nothing else.** There is no rounding-mode state, no sticky flag and no fast-math switch. `+`, `-`, `*`, `/` and `x.sqrt()` are correctly rounded, and both backends use the hardware instruction for each.
- **One canonical NaN.** Every operation that makes a NaN makes `0x7ff8000000000000`, and `Float.from_bits` turns any NaN pattern into it, so `to_bits()` never shows a payload. Two NaNs are never equal under `==`, and `<` and `>` are false for them (the example prints `NaN is not equal to itself`). `total_cmp` is a total order, NaN last, and is the way to sort floats. Negative zero equals zero under `==`, and `to_bits()` tells them apart.
- **No contraction.** The compiler never turns `a * b + c` into one fused operation. `math.fma(a, b, c)` is the explicit one that rounds once. In the example `0.1 * 10.0` rounds to exactly `1.0`, so the two operators give `0.0`, and the fused operation gives the 2^-54 that the product lost.
- **No `%` on a `Float`** (`E0223`), and a literal that rounds to infinity, such as `1e999`, is `E0222`.

## Conversions

There is no implicit `Int` to `Float` conversion, because a large `Int` would round without a word. Mixing them is `E0237`:

```oska error=E0237
fn main(io: Stdout) -> Result<(), IoError> {
    let count = 3
    let total = 1.5 + count
    io.println("{total}")?
    Ok(())
}
```

Write `1.5 + 3.0`, or convert with a method that says what it does:

- `n.to_float()` is exact, and panics (`P0047`) for an `Int` beyond 2^53, which it would round.
- `n.to_float_rounded()` rounds to nearest, ties to even. In the example `9007199254740993` becomes `9007199254740992.0`.
- `x.to_int()` truncates toward zero (`-2.9` gives `-2`), and panics (`P0044`) on NaN, an infinity, or a value that `Int` cannot hold. Check first when a value can be out of range.

## Text in and out

`x.to_string()`, `"{x}"` and `dbg` print the shortest digits that parse back to the same `Float`: `1.5`, `100.0`, `1e16`, `NaN`, `inf`. `x.fmt(digits)` prints 0 to 340 fixed decimals of the exact value, a tie going to the even digit (`P0048` for any other count).

`"1.5e3".parse_float()` is correctly rounded, and gives `None` for `NaN`, `inf`, a leading `+`, a number beyond `Float`, spaces, and text over 400 bytes. `parse_float_lenient()` is the opt-in that takes `+`, `nan`, `inf` and up to 65536 bytes.

## Secrets

`Secret<Float>` is refused (`E0357`): float arithmetic is not constant-time on common hardware, because a subnormal operand is slower. Keep a secret as a secret integer or `Secret<Bytes>`.

```oska error=E0357
fn main(io: Stdout) -> Result<(), IoError> {
    let key: Secret<Float> = Secret.new(1.5)
    io.println("unreachable")?
    Ok(())
}
```

## std.math

`use std.math`, then call a function as `math.name(...)`. Integer helpers: `math.gcd`, `math.isqrt`, `math.pow`, `math.floor_div` and `math.rem_euclid`. `math.pow(base, exp)` is the `Int` function, and the `Float` one is `math.powf`: `math.pow(2.0, 0.5)` is `E0220`.

`math.abs`, `math.floor`, `math.ceil` and `math.round_half_even` are exact. These functions are the **fast tier**, and the plain name is the fast tier: each is within 1 ulp of the exact result, the same bits on every platform and backend, written in Oska (one fdlibm-style algorithm each) so that no result depends on the C library.

| Function | Result |
| --- | --- |
| `math.hypot(x, y)` | within 1 ulp |
| `math.exp(x)`, `math.ln(x)` | within 1 ulp |
| `math.sin(x)`, `math.cos(x)` | within 1 ulp, for `\|x\| <= 1647098`; a finite `x` beyond that stops with `P0006`, and an infinity gives NaN |
| `math.atan2(y, x)` | within 1 ulp |
| `math.powf(x, y)` | within 1 ulp |

The bound is on the result, not a promise that it is the nearest double: in the example `math.powf(2.0, 0.5)` is `1.414213562373095`, one ulp below the nearest double to the square root of two, which `2.0.sqrt()` gives exactly. A program that needs the nearest double says so by name, in the exact tier below. Each loop in `std.math` counts steps, so `--max-steps` stops a float-heavy program at the same step on both backends ([ADR 0021](../adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)).

### The exact tier

The exact tier returns the nearest double for every input, a tie going to the even one, under a distinct name so that an agent cannot pick it by accident: `math.hypot_exact`, `math.exp_exact`, `math.ln_exact`, `math.sin_exact`, `math.cos_exact`, `math.atan2_exact` and `math.powf_exact`. They have the special cases of the fast functions (and `sin_exact` and `cos_exact` stop with `P0006` above 1647098 in size), give the same bits on every platform and backend, and differ from the fast tier by at most the last bit: `math.exp(1.0)` is 2.7182818284590455, `math.exp_exact(1.0)` is `e`. [ADR 0027](../adr/0027-float-results-are-pinned-by-the-spec-on-every-platform.md) states the promise.

Each function has two paths. The **fast path** evaluates it in double-double arithmetic (two doubles, about 106 bits) with a proven error below 2^-71 of the result, and answers when the result and the result moved by 2^-70 round to the same double. The **slow path** runs for the one input in about 100000 that fails that test: it uses 256-bit fixed point, or for `hypot` exact integers, and rounds once. A tie, which `hypot` and `powf` can have, is found and rounded to even. Each function's error proof is in its entry of the `std.math` reference.

| Function | Exact | Fast | Ratio |
| --- | --- | --- | --- |
| `exp` | 480 ns | 46 ns | 10 |
| `ln` | 1270 ns | 51 ns | 25 |
| `sin` | 1280 ns | 98 ns | 13 |
| `cos` | 1150 ns | 114 ns | 10 |
| `hypot` | 253 ns | 19 ns | 13 |
| `atan2` | 1290 ns | 485 ns | 2.7 |
| `powf` | 2470 ns | 1940 ns | 1.3 |

Measured natively on x86_64, one million calls, the loop's own cost removed. The fast `atan2` and `powf` call the software `fma`, which the exact tier does not. A call takes 140 to 300 steps on the fast path and 2700 to 45000 on the slow path.

<!-- end exact-tier -->

## Floats and tasks

A `Float` is 64 bits copied like an `Int`. There is no rounding-mode state, so tasks share nothing through floating point, and [the concurrency notes](https://github.com/aparsons/oska/blob/main/formal/tla/parallel-tasks.md) need no model for it.
