# Oska runtime

`oskac run <file.osk> [--format=json] [-- args...]` checks one file and, if the check reports no diagnostic, runs it in the interpreter (`oska-run`). The interpreter executes the checker's **checked program** (ADR 0007); the rules it follows are in the "Runtime" section of [`design.md`](design.md). This page describes how a run ends and the codes the runtime reports. Checker codes are in [`checker.md`](checker.md).

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | `main` returned `Ok` or `()`. |
| 1 | The checker refused the program, or `main` returned `Err`: an expected failure. |
| 2 | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| 101 | The program panicked: it has a bug. |

## Refusals

Before a program starts, the runtime checks that it can run all of it, reached or not, so a program never fails halfway through because of a gap in the interpreter. A refusal is printed like a checker diagnostic: to stderr, as one JSON object per line with `--format=json`.

| Code | Refusal |
| --- | --- |
| `E0370` | `main` declares a capability that has no runtime yet: `Net`, `NetConnect`, `NetListen`, `Spawn`, or `Sched`. Spans `main`'s body. |
| `E0371` | The program imports a `std` module that has no runtime yet: `std.proc`, `std.sql`, or `std.html`, used or not. Spans the module path of the `use`. |
| `E0380` | The program uses something the interpreter cannot run yet: a construct the checked program cannot express, a call of a prelude function with no runtime implementation, or a parameter of `main` that is neither a capability nor `Args`. Spans the construct or call, or `main`'s body for a parameter. |

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
| `P0003` | `todo()` reached. Spans the call. |
| `P0004` | Division by zero: the right operand of `/` or `%` is zero. Spans the operation. |
| `P0020` | `Env.secret` asked for a variable the environment does not set. Its signature has no room for a missing secret, and running on with a stand-in such as an empty one could let an empty password through. Spans the call. |

## Capabilities

`main` receives exactly the capabilities its parameters declare, and an `Args` parameter receives the program's arguments: `args.get(0)` is the program path, and the arguments after `--` follow. A capability is a runtime value that carries its attenuation state, so attenuation narrows authority at runtime as well as in the types.

- `Stdin`, `Stdout`, and `Stderr` reach the standard streams. `Env` reads environment variables; `Env.secret` wraps one in a `Secret`, which stays a wrapper at runtime, so `dbg` prints it as `<redacted>`.
- `Clock.now_millis()` is the time in milliseconds since the Unix epoch. `Random.next_int()` comes from a built-in pseudo-random generator (SplitMix64, not cryptographically secure), seeded from the clock and the process.
- `Fs`, `FsRead`, and `FsWrite` carry a root and an access. `read_only` and `write_only` keep the root and narrow the access, and a read or write the access does not allow is refused.
- **Confinement.** `restrict_to(dir)` on an unrestricted filesystem makes `dir`, canonicalized, the root; on a restricted one, `dir` is resolved under the current root like any path, so a restriction never widens. A directory that does not exist or escapes leaves a filesystem that reaches nothing. Through a restricted filesystem, every path is resolved like this, and a refusal is an `Err(IoError)`:
  1. An empty path is refused, and so is an absolute one, even one inside the root.
  2. The path is joined to the root and canonicalized: `..` is resolved and every symlink followed, in every component.
  3. The result must be the root or lie under it, compared component by component.
  4. A write may name a file that does not exist yet. Then its parent directory must canonicalize to inside the root, its last component must be a plain name, and it must not be a dangling symlink, which the write would follow.

  Checking and then opening are two steps, so a concurrent process that swaps a directory for a symlink between them could escape. The interpreter does not defend against an attacker racing it on the same filesystem.
