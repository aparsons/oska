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
| `E0370` | `main` declares a capability that has no runtime yet: `Net`, `NetConnect`, `NetListen`, `Spawn`, or `Sched`. Spans `main`'s body. (A `main` that declares a directory or file capability is already refused by the checker, `E0217`.) |
| `E0371` | The program imports a `std` module that has no runtime yet: `std.proc` or `std.sql`, used or not. Spans the module path of the `use`. |
| `E0380` | The program uses something the interpreter cannot run yet: a construct the checked program cannot express, a call of a prelude function with no runtime implementation, or a parameter of `main` that is neither a capability nor `Args`. The message names the construct, as in ``the interpreter cannot run a `From`-bound conversion yet``; others are a constructor used as a function value, a call of a generic trait's method, and a `?` or `return` in a contract outside any closure (the checker rejects that first, with `E0282`, so this is a second line of defence). Spans the construct or call, or `main`'s body for a parameter. |

### Refusal examples

Each refusal has a program that is refused with exactly that code and the same program fixed. The examples run on the in-memory host, so they depend on nothing outside the program.

#### `E0370`

```oska error=E0370
fn main(net: Net) -> Result<(), IoError> {
    Ok(())
}
```

```oska fixed=E0370
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hello")
}
```

#### `E0371`

```oska error=E0371
use std.sql

fn main() {}
```

```oska fixed=E0371
use std.html

fn main() {}
```

#### `E0380`

```oska error=E0380
fn main() {
    let make = Some
    let x: Option<Int> = make(1)
}
```

```oska fixed=E0380
fn main() {
    let x: Option<Int> = Some(1)
}
```

## Native builds

`oskac build [path] [-o out] [--format=json]` checks a source file or package, as `oskac check` does, and compiles it with the native backend (`oska-native`) to a static executable for aarch64 Linux, on any host. It writes the file's stem, or the package directory's name, in the current directory unless `-o` names the output. It prints nothing on success, or `{"output":"<path>","diagnostics":[]}` with `--format=json`. A refused program prints its diagnostics as `oskac check` does, or `{"output":null,"diagnostics":[...]}`. The exit code is 0 on success, 1 for the checker's diagnostics or an output that cannot be written, and 2 for a program the backend cannot compile yet (`E0371`, `E0390`).

The native backend is new and compiles only part of the language so far: `Int` and the sized integer types, `Bool`, text literals and interpolation, the `String` methods (`len`, `byte_len`, `slice`, `repeat`, `trim`, `replace`, `contains`, `starts_with`, `ends_with`, `to_lowercase`, `parse_int`, `split`, `lines`, `chars`, `join`), `String` and `Path` equality, lists of any item type with `List.new`, `push`, `len`, `count`, `get`, `at`, `==`, `map` and `for`, `Path.new` and `Path.from`, `assume_trusted`, `let` and `var`, `if`, `while`, ranges (as values too) and `for` over them, `break` and `continue`, calls of the program's functions (with any number of parameters), recursion included, structs and enums (`Option` and `Result` among them) with field reads and updates, mutating methods, `match` with every pattern form, generic functions and types, each compiled once for every list of type arguments it is used with, closures (capturing by value), `cap fn` values and named functions and methods used as values, `scope`, `spawn` and `join` (tasks run one at a time in spawn order, as in the interpreter), `std.html`, and the process capabilities `main` may declare: `Stdout`, `Stderr`, `Stdin`, `Args`, `Env` (with `Env.secret` and `Secret`), `Clock` and `Random`, with `dbg` (of every value but a directory or file capability, whose interpreter text holds an OS handle) and `todo`, and the filesystem capabilities: `Fs`, `FsRead` and `FsWrite` and every directory and file capability attenuated from them, confined exactly as the interpreter confines them, with the same `IoError` messages and `list_dir` order. A native program's standard output is buffered: it is written when a line goes to stderr or is read from stdin, at exit and on a panic, so the two streams stay in order on a terminal (a terminal gets each line as it is printed); a process killed by a signal (`E0392` under `oskac run`) loses what was still in the buffer, as a C program does. Letting go of a value takes a bounded native stack however deeply it nests (a chain of five million links included). `Random` is the interpreter's generator, seeded from the operating system for each run, and `Args.get(0)` is the path the executable was run as (`argv[0]`, such as `./hello`); under `oskac run` it is the source file's path or the package directory, as on the interpreter. A root opened from trusted text is named in messages by its canonical path, resolved as the interpreter's `fs::canonicalize` (glibc's `realpath`) resolves it, with no help from `/proc`, and shown as the interpreter shows it: a byte sequence that is not UTF-8 becomes U+FFFD. Lists, structs and enums keep their value semantics: changing a copy never changes the original. A program that uses anything else is refused before any file is written, never compiled wrongly. A program that imports a `std` module with no runtime is refused with `E0371`, as `oskac run` refuses it.

A native executable panics as `oskac run` does: the same report on stderr, `path:line:col: error[P....]: message (bytes a..b)`, with the path as `oskac build` was given it, and exit code 101. Its arithmetic and list indexing are checked exactly as the interpreter's are (`P0001`, `P0004`, `P0002`), and calls nest at most as deep as in the interpreter before `P0021`. It has no `--format=json` report.

A program's `test` blocks compile into a separate **test executable**, which runs one test per process: the one its argument names. A plain test runs on the runtime's test host, which behaves as the interpreter's (see [Tests](#tests)): an empty in-memory filesystem with the same confinement and messages, a clock at the Unix epoch, `Random` from the same fixed seed, recorded stdout and a `[name] ` prefix on each stderr line. An `#[integration]` test runs on the real OS. A program's own executable has no test host at all. `oskac test` reports native test runs exactly as it reports the interpreter's.

On aarch64 Linux, `oskac run` and `oskac test` build natively into a private temporary directory and run the executable; `--interpret` selects the interpreter, which every other host uses. A program the native backend refuses (`E0390`) runs on the interpreter after a one-line note on stderr: `path:line:col: note: running on the interpreter: <why> (E0390)`. `oskac run --format=json` reports a native panic as the interpreter's run does, as one JSON object on stderr. See [platform support](guide/cli-reference.md#platform-support).

| Code | Refusal |
| --- | --- |
| `E0390` | The program uses something the native backend cannot compile yet. The message names the construct, as in ``` `oskac build` cannot compile `dbg` of a `DirRead` yet ```, and says when it is in the runtime, `std.rt`, rather than in the program. Spans the construct, or for the runtime the code of the program that reached it. `oskac run` runs the program in the interpreter meanwhile. |
| `E0391` | The executable could not be written: the output's directory does not exist, permission is denied, or the path is a directory. Nothing was written. The fix is another path with `-o`. For `oskac run` and `oskac test` on aarch64 Linux, the executable could not be written to, or started from, the temporary directory (one mounted `noexec`, for example): set `TMPDIR` to another directory, or use `--interpret`. The path is a property of the command line, not of a program, so it has no example. |
| `E0392` | A native test process ended in a way no test can make it end: a signal killed it (named, as in `was killed by signal 11 (SIGSEGV)`), it exited with a code other than a pass's or a panic's, or its panic report was garbled. Or the program `oskac run` ran natively died of a signal only a fault raises (`SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE`, `SIGTRAP`, `SIGABRT`); `oskac run` then exits with 128 plus the signal's number. It is a fault in the native backend, not in the test or program, as a crash of `oskac run --interpret` would be one in the interpreter; a test is reported failed with it. The fix is to run with `--interpret` and report the fault. No program is meant to reach it, so it has no example. |

### Native build examples

The refusal (`E0390`) has a program that is refused with exactly that code and the same program fixed. `E0391` has no example: the output path is part of the command line, not of a program. `E0392` has no example: it is a fault in the native backend, which no program is meant to reach.

#### `E0390`

```oska error=E0390
fn main(fs: FsRead) -> Result<(), IoError> {
    let here = dbg(fs.restrict_to("."))
    Ok(())
}
```

```oska fixed=E0390
fn main(fs: FsRead) -> Result<(), IoError> {
    let here = fs.restrict_to(".")
    Ok(())
}
```

## Panics

A **panic** stops the program where a bug happened. It has a stable `P`-code, a message, and the span of the expression that panicked. Without `--format=json`, it is printed to stderr like a diagnostic, `file.osk:3:9: error[P0004]: division by zero: ... (bytes 61..67)`, starting with its file, line and column. With `--format=json`, it is printed to stderr as one diagnostic object, in the same shape as the checker's (see [`ast.md`](ast.md)), with a `path`: the file the panic is in. In a package that is the module's own file, and `span` is an offset into it (an item of the package is named as the panicking module's code names it, as `util.text.shout` or `shout`):

```json
{"code":"P0004","message":"division by zero: `10 / 0`","path":"file.osk","span":{"start":61,"end":70}}
```

Output written before the panic stays written.

Integers of every type, `Int` and the sized `I8` to `U64`, are checked: arithmetic never wraps.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow: the result of `+`, `-`, `*`, `/`, or unary `-` does not fit its type, as with a `U8` `0 - 1` or an `I8` `-128 / -1`. Spans the operation. |
| `P0002` | Index out of bounds: `List.at` was given an index below 0 or not below the list's length. Spans the call. |
| `P0003` | `todo()` reached. Spans the call. |
| `P0004` | Division by zero: the right operand of `/` or `%` is zero. Spans the operation. |
| `P0005` | `spawn` on a finished scope: the handle's `scope` block has ended, and work never runs outside a scope. Spans the call. This is a second line of defence behind the checker (`E0292`), which already stops a handle from outliving its block: no checked program can reach this code, so it has no example. |
| `P0006` | Precondition violated: a `requires` clause of the function called is false, so the caller broke the contract. The message names the function and quotes the clause's condition. Spans the call. Every call evaluates the function's `requires` clauses, in every build, in the order written: after the arguments are evaluated and before the body runs. A clause sees the parameters and `self`. A panic while a clause is evaluated, such as an overflow, is reported as it is, not as `P0006`. |
| `P0007` | Postcondition violated: an `ensures` clause of the function that returned is false, so the function broke its promise. The message names the function, quotes the clause's condition, and names the line and column of the return that broke it, counting columns in characters from 1. Spans the `ensures` clause. Every return evaluates the function's `ensures` clauses, in every build, in the order written, with `r` bound to the value being returned. That covers falling off the end of the body (the return is its last expression, or its closing brace), an early `return`, and an early exit through `?`, where `r` is the `Err` or `None` being returned. A clause sees the parameters and `self`; a method that changes `var self` shows its clauses the receiver it ends with. A contract that calls a function with contracts checks those too, and a panic while a clause is evaluated, such as an overflow, is reported as it is, not as `P0007`. |
| `P0010` | `t.assert(cond)` was given `false`. Fails the test (see [Tests](#tests)). Spans the call. |
| `P0011` | `t.assert_eq(actual, expected)` was given two values that differ through `Eq`. The message shows both, as `dbg` would. Spans the call. |
| `P0012` | `t.assert_ne(actual, other)` was given two equal values. Spans the call. |
| `P0013` | `t.fail(message)` was called. The message is `message`. Spans the call. |
| `P0020` | `Env.secret` asked for a variable the environment does not set. Its signature has no room for a missing secret, and running on with a stand-in such as an empty one could let an empty password through. Spans the call. |
| `P0021` | Calls nest too deep: runaway recursion. The interpreter runs on a thread of its own with a 256 MiB stack, and a call panics instead of starting when calls already nest 50,000 deep, or when so little of that stack is left that the next call could overflow it. The process never crashes on deep recursion: `oskac run` exits with 101, and under `oskac test` only the test that recursed fails. Spans the call that would have gone too deep. |
| `P0035` | `String.slice` was given indices out of bounds: a start or end below 0 or past the text's length in characters, or a start past the end. Spans the call. Indices count characters (Unicode code points), not bytes. |
| `P0036` | `String.repeat` was given a negative count. Spans the call. |
| `P0037` | `String.repeat` would build a text over 256 MiB, so the interpreter panics instead of running out of memory. Spans the call. |

### Panic examples

Each panic has a program that stops with exactly that code and the same program fixed. The `P001x` examples are tests, because only a `test` block can fail an assertion, and run under `oskac test`. `P0005` has no example: the checker (`E0292`) stops a program from keeping a scope handle past its block, so no checked program can reach it.

#### `P0001`

```oska error=P0001
fn main() {
    let x: U8 = 255
    let y = x + 1
}
```

```oska fixed=P0001
fn main() {
    let x: Int = 255
    let y = x + 1
}
```

#### `P0002`

```oska error=P0002
fn main() {
    let xs: List<Int> = List.new()
    let first = xs.at(0)
}
```

```oska fixed=P0002
fn main() {
    let xs: List<Int> = List.new()
    let first = xs.get(0)
}
```

#### `P0003`

```oska error=P0003
fn main() {
    todo()
}
```

```oska fixed=P0003
fn main() {}
```

#### `P0004`

```oska error=P0004
fn main() {
    let zero = 0
    let n = 10 / zero
}
```

```oska fixed=P0004
fn main() {
    let two = 2
    let n = 10 / two
}
```

#### `P0006`

```oska error=P0006
fn double(x: Int) -> Int
    requires x > 0
{
    x * 2
}

fn main() {
    let y = double(-1)
}
```

```oska fixed=P0006
fn double(x: Int) -> Int
    requires x > 0
{
    x * 2
}

fn main() {
    let y = double(1)
}
```

#### `P0007`

```oska error=P0007
fn absolute(x: Int) -> Int
    ensures |r| r >= 0
{
    x
}

fn main() {
    let y = absolute(-1)
}
```

```oska fixed=P0007
fn absolute(x: Int) -> Int
    ensures |r| r >= 0
{
    if x < 0 {
        0 - x
    } else {
        x
    }
}

fn main() {
    let y = absolute(-1)
}
```

#### `P0010`

```oska error=P0010 tests
test "is positive" {
    t.assert(1 < 0)
}
```

```oska fixed=P0010 tests
test "is positive" {
    t.assert(0 < 1)
}
```

#### `P0011`

```oska error=P0011 tests
test "adds" {
    t.assert_eq(1 + 1, 3)
}
```

```oska fixed=P0011 tests
test "adds" {
    t.assert_eq(1 + 1, 2)
}
```

#### `P0012`

```oska error=P0012 tests
test "differs" {
    t.assert_ne(2, 2)
}
```

```oska fixed=P0012 tests
test "differs" {
    t.assert_ne(2, 3)
}
```

#### `P0013`

```oska error=P0013 tests
test "not written yet" {
    t.fail("write this test")
}
```

```oska fixed=P0013 tests
test "written" {
    t.assert(true)
}
```

#### `P0020`

```oska error=P0020
fn main(env: Env) {
    let key = env.secret("API_KEY")
}
```

```oska fixed=P0020
fn main(env: Env) {
    let key = env.get("API_KEY")
}
```

#### `P0021`

```oska error=P0021
fn down(n: Int) -> Int {
    1 + down(n - 1)
}

fn main() {
    let depth = down(10)
}
```

```oska fixed=P0021
fn down(n: Int) -> Int {
    if n == 0 { 0 } else { 1 + down(n - 1) }
}

fn main() {
    let depth = down(10)
}
```

#### `P0035`

```oska error=P0035
fn main() {
    let part = "abc".slice(0, 10)
}
```

```oska fixed=P0035
fn main() {
    let part = "abc".slice(0, 2)
}
```

#### `P0036`

```oska error=P0036
fn main() {
    let line = "ab".repeat(-1)
}
```

```oska fixed=P0036
fn main() {
    let line = "ab".repeat(3)
}
```

#### `P0037`

```oska error=P0037
fn main() {
    let huge = "ab".repeat(300000000)
}
```

```oska fixed=P0037
fn main() {
    let small = "ab".repeat(3000)
}
```

## Errors and traits

- **`main` returning `Err(e)`.** The runtime prints `e.to_string()`, the `Display` text of `e`, and a newline to stderr, and exits with code 1.
- **`?`** returns `None` or `Err(e)` from the enclosing function or closure. When the function's error type differs, `e` is converted through the `From` impl the checker chose.
- **Trait calls.** A trait method called on a value of a known type reaches that type's impl directly. One called on a value of a type parameter is dispatched on the value's **runtime type tag**, the name of its type without arguments (`Point`, `List`, `String`), to the program's impl for that type or else the prelude's. `Option`, `Result`, and `List` compare their items through `Eq` for the items' type.
- Every integer carries its own type as its tag: `Int`, or a sized type from `I8` to `U64`. A literal takes the type the checker gave it, arithmetic keeps its operands' type, and a range's items keep its bounds' type, so a `U8` reaches the impl for `U8` and an `Int` the one for `Int`.

## Capabilities

`main` receives exactly the capabilities its parameters declare, and an `Args` parameter receives the program's arguments: `args.get(0)` is the program path, and the arguments after `--` follow. A capability is a runtime value that carries its attenuation state, so attenuation narrows authority at runtime as well as in the types.

- `Stdin`, `Stdout`, and `Stderr` reach the standard streams. A failed write is an `IoError`, except one that finds standard output's reader gone (a closed pipe, as in `oskac run app.osk | head -1`): the program ends there, quietly, with exit code 0, on both backends. `Env` reads environment variables; `Env.secret` wraps one in a `Secret`, which stays a wrapper at runtime, so `dbg` prints it as `<redacted>`.
- `Clock.now_millis()` is the time in milliseconds since the Unix epoch. `Random.next_int()` comes from a built-in pseudo-random generator (SplitMix64, not cryptographically secure), seeded from the clock and the process.
- `Fs`, `FsRead`, and `FsWrite` grant no file access themselves: they have no method that reads, lists, writes, or creates (a call of one is `E0273`), only attenuations. Code that needs the whole filesystem says `restrict_to("/")`. `restrict_to` attenuates one to a **directory capability**, `Dir`, `DirRead`, or `DirWrite`, which carries a root and the same access. `read_only` and `write_only` keep the root and narrow the access, and a read or write the access does not allow is refused. `restrict_to_file` on `FsRead` and `FsWrite`, or on a `DirRead` or `DirWrite`, attenuates to a **file capability**, `FileRead` or `FileWrite`, for exactly one file: `read_to_string()` and `write(text)` take no path, and no capability both reads and writes a file. Only attenuation makes a directory or file capability: `main` declaring one is refused by the checker (`E0217`).
- `write(path, text)` creates the file or replaces what it holds. `create_dir(path)` creates one directory, whose parent must exist. A directory already there is kept, with what it holds, so a program can rebuild into it; a file there is an error. `list_dir(path)` gives the names of a directory's entries, sorted by their bytes; a symlink is listed by its own name, and not followed.
- **Confinement.** `restrict_to(dir)` opens `dir` as the root, which must be a directory, and the directory capability holds it from then on. On `Fs`, `FsRead`, and `FsWrite`, `dir` is trusted text, any path whose last component is not a symlink; earlier components are followed, since the root is trusted text. On a directory capability, `dir` is a `Path`, which may come from untrusted text, walked beneath the current root as below, so a narrowing never widens. Attenuation never fails: a root that is missing, a symlink, not a directory, or outside the current root gives a directory capability that reaches nothing, and every operation on it fails with an `IoError` naming the root and the reason. A file passed to `restrict_to` is such a root: it is not a directory.
- **File capabilities.** `restrict_to_file(file)` opens the file's parent directory as `restrict_to` opens a root (on a directory capability, by walking the `Path` beneath the current root), and holds that handle together with the file's name. The file itself is not opened then, so a `FileWrite` can create it. Each use opens the name beneath the parent's handle without following a symlink, so a file replaced between uses can never lead outside: a symlink in its place is refused, and a parent swapped for a symlink is not met, because the capability holds the original. A parent that is missing, or beneath a symlink, makes every operation fail with an `IoError` naming the file; so does a name that is not a regular file.
- **Every operation walks beneath the root.** `read_to_string`, `write`, `create_dir`, `list_dir`, and narrowing with `restrict_to` resolve `path` like this, and a refusal is an `Err(IoError)`:
  1. An empty path is refused, and so is an absolute one, even one inside the root.
  2. `.` is dropped, and `..` drops the name before it, on the path as written. A `..` that would climb above the root is refused; `a/../b` is `b`, even when `a` is a symlink.
  3. `write` and `create_dir` need a name of their own: the path as written must end in a plain name, not in a separator, `.`, or `..`. `dang/` and `sub/.` are refused, even though path libraries treat them as `dang` and `sub`.
  4. On Linux and macOS, the runtime holds an OS handle for the root, opened when the filesystem was restricted and released with the last copy of it. Each component is opened relative to the one before, starting from that handle, and none is followed if it is a symlink, the last included (the `oska-sys` crate, [ADR 0009](adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)). A read opens its file beneath its parent's handle. A write opens or creates its file there, and empties it only once it is known to be a regular file, so a write never goes through a symlink and creates nothing outside the root. `create_dir` makes its directory beneath its parent's handle, and `list_dir` lists through the directory's own handle. There is no separate check to race: a symlink at any component, planted before the operation or swapped in during it, makes it fail. A native executable (`oskac build`) walks the same way with its own system calls (`openat` with `O_NOFOLLOW`, in the toolchain's runtime, `std.rt`), with the same refusals and messages.
  5. **Every symlink beneath the root is refused**, dangling or not, whether it points inside the root or outside it. This is a change: before #94 and #95, a symlink that resolved inside the root was followed.
  6. **A hard-linked file is refused** on Linux and macOS. A hard link is the same file under a second name, so one planted beneath the root, such as in an upload directory, could expose or overwrite a file outside it. A read or a write that opens an existing regular file with more than one link fails with an `IoError` (`` `name` has more than one hard link, and confined access never opens one ``), checked on the open handle before anything is read or the file is emptied. A write that creates a new file is unaffected. Windows does not check link counts.
  6. A file that is not a regular one, such as a FIFO or a device, is refused rather than read or written.

  **Windows keeps a race.** `std` has no handle-relative open on Windows, so there each component is checked on the path not to be a symlink, the path is opened, and what was opened is checked again to lie under the root. That narrows the race but does not close it: a concurrent process that swaps an *intermediate* directory for a symlink at the right moment can still direct the open outside the root. The re-check then refuses to read or write what was opened, but a write may already have created an empty file there. Every name walked beneath the root is also refused on Windows when it is a reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, with or without an extension, as in `nul.txt`), which opens the device in every directory, or when it holds a `:`, which opens an alternate data stream.

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
- **Integration tests.** A test marked `#[integration]` is skipped unless `--integration` is given. Then it runs on the real OS: `t.fs` is an `Fs` over the real filesystem (attenuate it like any `Fs`), `t.clock` the real time, and `t.random` seeded from the clock. Its `t.stdout` is still recorded, not written to the terminal, so `t.printed()` works and the report is the only thing on stdout.

### Report and exit codes

Without `--format=json`, one line per test, then the failures, then a summary:

```
test adds ... ok
test divides ... FAILED
test reads the real config ... skipped (#[integration]; run with --integration)

failures:
test divides: math.osk:12:5: error[P0011]: assertion failed: `actual == expected`: actual 2, expected 3 (bytes 120..141)

3 tests: 1 passed, 1 failed, 1 skipped
```

Each failure is named by its test. In a package, a test is named after its module and `: `, as in `test util.text: shouts ... ok`, and its failure's path starts from the current directory, as in `shop/src/util/text.osk:6:5`. Control characters in test names and failure messages are escaped (`\n`, `\r`, `\t`, `\u{..}`), so a message cannot forge a line of the report.

**Output.** What a test writes to stderr, `dbg` included, goes to the real stderr as it happens, each line prefixed with the test's name in brackets, such as `[divides] [dbg] 4`. What it prints to `t.stdout` is recorded for `t.printed()`, for integration tests too, so the report is the only thing on stdout.

With `--format=json`, one JSON object on stdout. Each test has its name, in a package its `module` (such as `"module":"util.text"`, right after `name`), its status (`passed`, `failed`, or `skipped`), and its failure, a diagnostic object or `null`, whose `path` is relative to the package. A refused file has no tests and its diagnostics in `diagnostics`:

```json
{"tests":[{"name":"adds","status":"passed","failure":null},{"name":"divides","status":"failed","failure":{"code":"P0011","message":"...","path":"math.osk","span":{"start":120,"end":141}}}],"passed":1,"failed":1,"skipped":0,"diagnostics":[]}
```

| Exit code | Meaning |
| --- | --- |
| 0 | No test failed. Skipped tests do not count. |
| 1 | A test failed, or the checker refused the file. |
| 2 | The file uses something the interpreter cannot run yet (`E0371`, `E0380`). No test ran. |
