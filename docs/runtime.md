# Oska runtime

`oskac run <file.osk> [--format=json] [-- args...]` checks one file and, if the check reports no diagnostic, runs it in the interpreter (`oska-run`). `oskac test <file.osk> [--format=json] [--integration]` runs its `test` blocks instead (see [Tests](#tests)). The interpreter executes the checker's **checked program** (ADR 0007); the rules it follows are in the "Runtime" section of [`design.md`](design.md). This page describes how a run ends and the codes the runtime reports. Checker codes are in [`checker.md`](checker.md).

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
| `E0380` | The program uses something the interpreter cannot run yet: a construct the checked program cannot express, a call of a prelude function with no runtime implementation, a parameter of `main` that is neither a capability nor `Args`, or a trait call on a type parameter whose trait has its own impl for a sized integer type (see [Errors and traits](#errors-and-traits)). Spans the construct or call, or `main`'s body for a parameter. |

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
| `P0005` | `spawn` on a finished scope: the handle's `scope` block has ended. The checker (`E0292`) already stops a handle from outliving its block, so this is a second line of defence; work never runs outside a scope. Spans the call. |
| `P0010` | `t.assert(cond)` was given `false`. Fails the test (see [Tests](#tests)). Spans the call. |
| `P0011` | `t.assert_eq(actual, expected)` was given two values that differ through `Eq`. The message shows both, as `dbg` would. Spans the call. |
| `P0012` | `t.assert_ne(actual, other)` was given two equal values. Spans the call. |
| `P0013` | `t.fail(message)` was called. The message is `message`. Spans the call. |
| `P0020` | `Env.secret` asked for a variable the environment does not set. Its signature has no room for a missing secret, and running on with a stand-in such as an empty one could let an empty password through. Spans the call. |
| `P0021` | Calls nest too deep: runaway recursion. The interpreter runs on a thread of its own with a 256 MiB stack, and a call panics instead of starting when calls already nest 50,000 deep, or when so little of that stack is left that the next call could overflow it. The process never crashes on deep recursion: `oskac run` exits with 101, and under `oskac test` only the test that recursed fails. Spans the call that would have gone too deep. |

## Errors and traits

- **`main` returning `Err(e)`.** The runtime prints `e.to_string()`, the `Display` text of `e`, and a newline to stderr, and exits with code 1.
- **`?`** returns `None` or `Err(e)` from the enclosing function or closure. When the function's error type differs, `e` is converted through the `From` impl the checker chose.
- **Trait calls.** A trait method called on a value of a known type reaches that type's impl directly. One called on a value of a type parameter is dispatched on the value's **runtime type tag**, the name of its type without arguments (`Point`, `List`, `String`), to the program's impl for that type or else the prelude's. `Option`, `Result`, and `List` compare their items through `Eq` for the items' type.
- Every integer has the tag `Int`, whatever its type. Dispatch could not tell a `U8`'s impl of a trait from an `Int`'s, so a program that calls a trait's method on a type parameter, while that trait has an impl for a sized integer type (`I8` to `U64`), is refused with `E0380`.

## Capabilities

`main` receives exactly the capabilities its parameters declare, and an `Args` parameter receives the program's arguments: `args.get(0)` is the program path, and the arguments after `--` follow. A capability is a runtime value that carries its attenuation state, so attenuation narrows authority at runtime as well as in the types.

- `Stdin`, `Stdout`, and `Stderr` reach the standard streams. `Env` reads environment variables; `Env.secret` wraps one in a `Secret`, which stays a wrapper at runtime, so `dbg` prints it as `<redacted>`.
- `Clock.now_millis()` is the time in milliseconds since the Unix epoch. `Random.next_int()` comes from a built-in pseudo-random generator (SplitMix64, not cryptographically secure), seeded from the clock and the process.
- `Fs`, `FsRead`, and `FsWrite` carry a root and an access. `read_only` and `write_only` keep the root and narrow the access, and a read or write the access does not allow is refused.
- **Confinement.** `restrict_to(dir)` on an unrestricted filesystem makes `dir`, canonicalized, the root; on a restricted one, `dir` is resolved under the current root like any path, so a restriction never widens. A directory that does not exist or escapes leaves a filesystem that reaches nothing. Through a restricted filesystem, every path is resolved like this, and a refusal is an `Err(IoError)`:
  1. An empty path is refused, and so is an absolute one, even one inside the root.
  2. The path is joined to the root and canonicalized: `..` is resolved and every symlink followed, in every component.
  3. The result must be the root or lie under it, compared component by component.
  4. A write names its file differently, because the file may not exist yet. The path as written must end in a plain name, not in a separator, `.`, or `..`: `dang/` and `dang/.` are refused, even though path libraries treat them as `dang`. The parent directory must canonicalize to inside the root, and the exact path to be opened, that directory joined with the name, must not be a symlink, dangling or not. A write never follows a symlink in its last component, even one that points inside the root.
  5. The file is then opened without following a symlink in its last component (`O_NOFOLLOW` on Linux and macOS; elsewhere the checks above stand alone), and the opened file is checked again, by its device and inode, to be the file its path now canonicalizes to, under the root. Only then is it read, or emptied and written: a file is not truncated by the open itself.

  **Residual risk.** Checking a path and opening it are still separate steps, so the defence above narrows a race but does not close it. A concurrent process that swaps an *intermediate* directory for a symlink at the right moment can still direct the open outside the root. The re-check then refuses to read or write what was opened, but a write may already have created an empty file there. Closing the race needs the whole path to be resolved beneath a directory handle, as Linux `openat2` with `RESOLVE_BENEATH` does, and `std` does not offer that. The interpreter does not defend against an attacker racing it on the same filesystem.

## Scopes and tasks

`scope |s| { ... }` runs tasks one at a time, in the order they were spawned, so the same program gives the same result on every run.

- `s.spawn(f)` queues `f` and returns a task. Nothing runs yet, so a task never starts in the middle of another task or of the block that spawned it. `f` sees the values it captured when it was spawned.
- `t.join()` runs the queued tasks of the scope, oldest first, up to and including `t`, and returns `t`'s result. Joining a later task first still runs the earlier ones first. Joining a task that has already run returns its result again.
- A scope does not exit until all its tasks have finished: when the block ends, by its value or by a `return`, the tasks still queued run in spawn order. A panic ends the program, so queued tasks do not run.
- A scope handle is valid only while its block runs. When the block ends the scope is closed, and `spawn` on its handle panics with `P0005`. A task can still be joined after its scope ends, because it has already run.
- A panic in a task stops the program at the task, with the panic's own code and span.

## Tests

`oskac test file.osk` checks the file, refuses it as `oskac run` would (by the checker's diagnostics, or by `E0371` or `E0380`, for its functions and its tests), and then runs every `test` block in source order. A refused file runs no test.

- **Stand-ins.** Each test gets a fresh `t: TestContext` on a fresh in-memory host, so nothing one test does is seen by another, and the same file gives the same report on every run:
  - `t.fs` is an empty in-memory filesystem, with its working directory at `/`.
  - `t.clock` is fixed at the Unix epoch: `now_millis()` is `0`.
  - `t.random` starts from the same fixed seed in every test.
  - `t.stdout` records what it prints. `t.printed()` returns every line printed so far in the test, one per `println`, without its line break.
- **Assertions.** `t.assert`, `t.assert_eq`, `t.assert_ne`, and `t.fail` fail the test by panicking, with `P0010` to `P0013` and the span of the call. A failed assertion is a panic like any other: it stops the test, and its diagnostic is the test's failure. A panic from a bug, such as `P0004`, fails the test the same way.
- **Isolation.** A panic stops only the test it happened in, runaway recursion (`P0021`) included. Each test runs in a new interpreter, on a new host, on a thread of its own.
- **Integration tests.** A test marked `#[integration]` is skipped unless `--integration` is given. Then it runs on the real OS: `t.fs` is the real filesystem, unrestricted, `t.clock` the real time, and `t.random` seeded from the clock. Its `t.stdout` is still recorded, not written to the terminal, so `t.printed()` works and the report is the only thing on stdout.

### Report and exit codes

Without `--format=json`, one line per test, then the failures, then a summary:

```
test adds ... ok
test divides ... FAILED
test reads the real config ... skipped (#[integration]; run with --integration)

failures:
math.osk: error[P0011]: assertion failed: `actual == expected`: actual 2, expected 3 (bytes 120..141)

3 tests: 1 passed, 1 failed, 1 skipped
```

With `--format=json`, one JSON object on stdout. Each test has its name, its status (`passed`, `failed`, or `skipped`), and its failure, a diagnostic object or `null`. A refused file has no tests and its diagnostics in `diagnostics`:

```json
{"tests":[{"name":"adds","status":"passed","failure":null},{"name":"divides","status":"failed","failure":{"code":"P0011","message":"...","span":{"start":120,"end":141}}}],"passed":1,"failed":1,"skipped":0,"diagnostics":[]}
```

| Exit code | Meaning |
| --- | --- |
| 0 | No test failed. Skipped tests do not count. |
| 1 | A test failed, or the checker refused the file. |
| 2 | The file uses something the interpreter cannot run yet (`E0371`, `E0380`). No test ran. |
