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
```

Output written before the panic stays written, so `computing` was printed. A panic has a stable `P`-code, a message, and the byte span of the expression that panicked. The process exits with code 101.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow. All integer types are checked, and arithmetic never wraps. |
| `P0002` | `List.at` with an index below 0 or not below the length. Use `get` when the index may be out of bounds. |
| `P0003` | `todo()` reached. |
| `P0004` | Division or remainder by zero. |
| `P0005` | `spawn` on a scope that has already finished. |
| `P0006` | A `requires` clause of the function called is false: fix the call, which broke the contract. |
| `P0007` | An `ensures` clause of the function that returned is false: fix the function body, which broke its promise. The message names the return. |
| `P0010` to `P0013` | A failed test assertion. See [testing](testing.md). |
| `P0020` | `Env.secret` asked for a variable that is not set. |
| `P0021` | Calls nest too deep: runaway recursion. The program panics instead of overflowing the stack. |

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

For a panic, `oskac run --format=json` prints one diagnostic object per line to standard error:

```console cmd=run stream=stderr json
{"code":"P0004","message":"division by zero: `10 / 0`","path":"main.osk","span":{"start":46,"end":59}}
```

`oskac check --format=json` prints a single object with `diagnostics`, `functions`, and `audit` to standard output; see the [CLI reference](cli-reference.md#output-formats). The `span` offsets count UTF-8 bytes from the start of the file, not characters or lines. To show the source a diagnostic points at, slice the file's bytes from `start` up to, but excluding, `end`.

## When the problem is syntax

A parse error is reported alone, because the rest of the file cannot be trusted. `oskac lex` prints the tokens with their spans, and `oskac parse` prints the syntax tree, as JSON with `--format=json`. These are the right tools when a message about a line break or a missing comma does not make sense: the syntax decides where a statement ends by looking at the last token of a line (a name, a literal, `)`, `]`, `}`, `?`, and a few keywords end one), so a line that ends with an operator continues on the next.

## A debugging routine

1. Run `oskac check file.osk --format=json` and read the `diagnostics`. Fix the first one: later ones often follow from it.
2. If it checks, run it. For a panic, take the `span` and look at that expression.
3. Wrap suspicious expressions in `dbg(...)`.
4. Write a `test` that reproduces the bug, so it stays fixed. See [testing](testing.md).
5. Remove the `dbg` calls. `oskac check --release` confirms none are left.
