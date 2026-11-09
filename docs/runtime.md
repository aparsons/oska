# Oska runtime

`oskac run <file.osk> [--format=json] [-- args...]` checks one file and, if the check reports no diagnostic, runs it in the interpreter (`oska-run`). The interpreter executes the checker's **checked program** (ADR 0007); the rules it follows are in the "Runtime" section of [`design.md`](design.md). This page describes how a run ends and the codes the runtime reports. Checker codes (`E02xx`, `E03xx`) are in [`checker.md`](checker.md).

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | `main` returned `Ok` or `()`. |
| 1 | The program was refused before it started, or `main` returned `Err`: an expected failure. |
| 101 | The program panicked: it has a bug. |

## Panics

A **panic** stops the program where a bug happened. It has a stable `P`-code, a message, and the span of the expression that panicked. Without `--format=json`, it is printed to stderr like a diagnostic, `file.osk: error[P0001]: ... (bytes 40..45)`. With `--format=json`, it is printed to stderr as one diagnostic object, in the same shape as the checker's (see [`ast.md`](ast.md)):

```json
{"code":"P0004","message":"division by zero","span":{"start":61,"end":66}}
```

Integers of every type, `Int` and the sized `I8`...`U64`, are checked: arithmetic never wraps.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow: the result of `+`, `-`, `*`, `/`, or unary `-` does not fit its type, as with a `U8` `0 - 1`. Spans the operation. |
| `P0003` | `todo()` reached. Spans the call. |
| `P0004` | Division by zero: the right operand of `/` or `%` is zero. Spans the operation. |
