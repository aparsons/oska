# Debugging: `dbg`, panics, and `--format=json`

Oska keeps debugging explicit. There are three tools: `dbg` to look at a value, panic reports to see where a bug happened, and `--format=json` to give every message to a tool.

## `dbg`

`dbg(expr)` prints the value of `expr` to standard error and returns it, so you can wrap any expression without changing the program. It writes `[dbg]` followed by the value.

```oska
struct Point { x: Int, y: Int }

fn main(io: Stdout) -> Result<(), IoError> {
    let p = dbg(Point(x: 1, y: 2))
    let n = dbg(p.x + 40)
    io.println("n is {n}")?
    Ok(())
}
```

```console cmd=run stream=stderr
[dbg] Point(1, 2)
[dbg] 41
```

```console cmd=run
n is 41
```

Standard output and standard error are separate, so `dbg` never mixes into the output of a program. A `Secret` is printed as `<redacted>`.

`dbg` is one of two deliberate exceptions to "effects need capabilities" (the other is a panic). Because it has an effect without a capability, it must not ship: checking with `--release` rejects every `dbg(...)` with `E0216`, and every `todo()` with `E0293`. The `E0216` diagnostic carries a fix that keeps the argument and removes the call.

```console cmd=check release stream=stderr
main.osk:4:13: error[E0216]: `dbg(...)` is not allowed in a release build (bytes 90..112)
    fix: remove `dbg`, keeping its argument
main.osk:5:13: error[E0216]: `dbg(...)` is not allowed in a release build (bytes 125..138)
    fix: remove `dbg`, keeping its argument
```

Make `oskac check --release` part of CI so a stray `dbg` fails the build. Inside a test, `dbg` output is prefixed with the test's name, as in `[my test] [dbg] 4`.

## `todo()`

`todo()` fits any type and stops the program when it is reached, with `P0003`. It marks code you have not written yet, and it is the body the fixes use for arms they add to a non-exhaustive `match`. `--release` rejects it.

## Panics

A **panic** is a runtime failure that stops the current task and cannot be caught. It means the program has a bug. A returned `Err` is different: it is an expected failure.

```oska
fn ratio(total: Int, parts: Int) -> Int {
    total / parts
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println("computing")?
    let r = ratio(10, 0)
    io.println("ratio is {r}")?
    Ok(())
}
```

```console cmd=run stream=stderr
main.osk:2:5: error[P0004]: division by zero: `10 / 0` (bytes 46..59)
    at ratio (main.osk:2:5)
    at main (main.osk:7:13)
```

Output written before the panic stays written, so `computing` was printed. A panic has a stable `P`-code, a message, and the byte span of the expression that panicked. The process exits with code 101.

Under the panic's line come its **frames**, the call stack that led to it, innermost first: the function that panicked, at the panic's place, then each function that called it, at the call. Here `ratio` panicked, and `main` called it on line 7, so a `P0004` deep in a helper shows which caller passed the bad value. A closure's frame is named `<closure in f>`, after the function it is written in, and a test's body `test "name"`. At most 20 frames are listed, then `    ... n more frames`, so runaway recursion (`P0021`) stays readable. `oskac test` lists the frames of a failed test too, back to the test's own line. Native code and the interpreter report the same frames. The one exception is `P0021` when the interpreter's own host stack runs out before calls nest 50,000 deep, which can happen in a debug build of `oskac`: then its `frames_omitted` counts fewer frames left out.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow, or a `to_<type>()` conversion given a value that does not fit its target, such as `300.to_u8()`. All integer types are checked, and arithmetic never wraps: call `wrapping_add`, `wrapping_sub`, `wrapping_mul` or `wrapping_to_<type>()` where wrapping is meant. |
| `P0002` | `List.at`, `Bytes.at` or `Bytes.set` with an index below 0 or not below the length. Use `get` when the index may be out of bounds. |
| `P0003` | `todo()` reached. |
| `P0004` | Division or remainder by zero. |
| `P0005` | `spawn` on a scope that has already finished. |
| `P0006` | A `requires` clause of the function called is false: fix the call, which broke the contract. |
| `P0007` | An `ensures` clause of the function that returned is false: fix the function body, which broke its promise. The message names the return. |
| `P0008` | A shift (`<<`, `>>`) or rotation (`rotl`, `rotr`) by an amount below 0 or not below the type's width, such as a `U32` shifted by 32. The amount is never taken modulo the width. |
| `P0010` to `P0014` | A failed test assertion, `P0014` a missing or different snapshot. See [testing](testing.md). |
| `P0020` | `Env.secret` asked for a variable that is not set. |
| `P0021` | Calls nest too deep: runaway recursion. The program panics instead of overflowing the stack. |
| `P0042` | A `Bytes` slice or endian read that runs past the end of the buffer, such as `read_u32_be(0)` on 3 bytes, `Bytes.zeroed` with a negative length, or `ct_select` between two `Secret<Bytes>` of different lengths. Check `len()` first when the buffer may be short. |
| `P0043` | A `Bytes` that would grow past 256 MiB. Not yet checked for `String.to_bytes` or a capability's `read_bytes`. |
| `P0044` | A `std.map` map or `std.set` set given a new entry past its entry limit (4,194,304, or the lower `Map.with_max_entries(n)` or `Set.with_max_entries(n)`). |
| `P0045` | A `std.map` map or `std.set` set whose keys or items collide under every hash it tries, as keys chosen to flood it do. |
| `P0046` | `Float.to_int()` of a NaN, an infinity or a value `Int` cannot hold. Check the value first. |
| `P0049` | `Int.to_float()` of a value beyond 2^53 or below -2^53, which a `Float` could not hold exactly. Call `to_float_rounded()` where rounding is meant. |
| `P0050` | `Float.fmt(digits)` with `digits` below 0 or above 340. |
| `P0051` | A `std.deque` deque given an item past its item limit (4,194,304, or the lower `Deque.with_max_entries(n)`). |

The full table is in the [runtime notes](../runtime.md#panics).

## Exit codes of `oskac run`

| Code | Meaning |
| --- | --- |
| `0` | `main` returned `Ok` or `()`. |
| `1` | The checker refused the program, or `main` returned `Err`. An expected failure. |
| `2` | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| `101` | The program panicked. It has a bug. |

When `main` returns `Err(e)`, the runtime prints `e.to_string()` to standard error and exits with 1.

## `--format=json`

Every command takes `--format=json`. Diagnostics then carry the same shape everywhere: a stable `code`, a `message`, a `span` of byte offsets, and a `fix` when the repair is unambiguous. Tools should match on `code`, never on `message`.

For a panic, `oskac run --format=json` prints one diagnostic object per line to standard error, with the frames in `frames`, each `{function, path, span, line, column}`, and how many were left out in `frames_omitted`:

```console cmd=run stream=stderr json
{"code":"P0004","message":"division by zero: `10 / 0`","path":"main.osk","span":{"start":46,"end":59},"line":2,"column":5,"end_line":2,"end_column":18,"severity":"error","url":"https://oskalang.dev/codes/P0004","related":[],"suggestions":[],"frames":[{"function":"ratio","path":"main.osk","span":{"start":46,"end":59},"line":2,"column":5},{"function":"main","path":"main.osk","span":{"start":149,"end":161},"line":7,"column":13}],"frames_omitted":0}
```

`oskac check --format=json` prints a single object with `diagnostics`, `functions`, and `audit` to standard output; see the [CLI reference](cli-reference.md#output-formats). The `span` offsets count UTF-8 bytes from the start of the file, not characters or lines. To show the source a diagnostic points at, slice the file's bytes from `start` up to, but excluding, `end`.

## When the problem is syntax

A parse error is reported alone, because the rest of the file cannot be trusted. `oskac lex` prints the tokens with their spans, and `oskac parse` prints the syntax tree, as JSON with `--format=json`. These are the right tools when a message about a line break or a missing comma does not make sense: the syntax decides where a statement ends by looking at the last token of a line (a name, a literal, `)`, `]`, `}`, `?`, and a few keywords end one), so a line that ends with an operator continues on the next.

## A debugging routine

1. Run `oskac check file.osk --format=json` and read the `diagnostics`. Fix the first one: later ones often follow from it.
2. If it checks, run it. For a panic, take the `span` and look at that expression, then walk up its `frames` to the caller that passed the bad value.
3. Wrap suspicious expressions in `dbg(...)`.
4. Write a `test` that reproduces the bug, so it stays fixed. See [testing](testing.md).
5. Remove the `dbg` calls. `oskac check --release` confirms none are left.
