# Oska runtime

`oskac run <file.osk> [--format=json] [-- args...]` checks one file and, if the check reports no diagnostic, runs it in the interpreter (`oska-run`). The interpreter executes the checker's **checked program** (ADR 0007); the rules it follows are in the "Runtime" section of [`design.md`](design.md). This page describes how a run ends and the codes the runtime reports. Checker codes are in [`checker.md`](checker.md).

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | `main` returned `Ok` or `()`. |
| 1 | The checker refused the program, or `main` returned `Err`: an expected failure. |
| 2 | The program uses something the interpreter cannot run yet (`E0380`). Nothing ran. |
| 101 | The program panicked: it has a bug. |

## Refusals

Before a program starts, the runtime checks that it can run all of it, reached or not, so a program never fails halfway through because of a gap in the interpreter. A refusal is printed like a checker diagnostic: to stderr, as one JSON object per line with `--format=json`.

| Code | Refusal |
| --- | --- |
| `E0380` | The program uses something the interpreter cannot run yet: a construct the checked program cannot express, a call of a prelude function with no runtime implementation, or a capability `main` declares that has no runtime. Spans the construct or call, or `main`'s body for a capability. |

## Panics

A **panic** stops the program where a bug happened. It has a stable `P`-code, a message, and the span of the expression that panicked. Without `--format=json`, it is printed to stderr like a diagnostic, `file.osk: error[P0004]: division by zero: ... (bytes 61..67)`. With `--format=json`, it is printed to stderr as one diagnostic object, in the same shape as the checker's (see [`ast.md`](ast.md)):

```json
{"code":"P0004","message":"division by zero: `10 / 0`","span":{"start":61,"end":70}}
```

Output written before the panic stays written.

Integers of every type, `Int` and the sized `I8` to `U64`, are checked: arithmetic never wraps.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow: the result of `+`, `-`, `*`, `/`, or unary `-` does not fit its type, as with a `U8` `0 - 1` or an `I8` `-128 / -1`. Spans the operation. |
| `P0002` | Index out of bounds: `List.at` was given an index below 0 or not below the list's length. Spans the call. |
| `P0003` | `todo()` reached. Spans the call. |
| `P0004` | Division by zero: the right operand of `/` or `%` is zero. Spans the operation. |
