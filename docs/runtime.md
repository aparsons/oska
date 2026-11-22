# Oska runtime

`oskac run <file.osk> [--format=json] [-- args...]` checks one file and, if the check reports no diagnostic, runs it in the interpreter (`oska-run`). `oskac test <file.osk> [--format=json] [--integration]` runs its `test` blocks instead (see [Tests](#tests)). The interpreter executes the checker's **checked program** (ADR 0007); the rules it follows are in the "Runtime" section of [`design.md`](design.md). This page describes how a run ends and the codes the runtime reports. Checker codes are in [`checker.md`](checker.md).

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | `main` returned `Ok` or `()`. |
| 1 | The checker refused the program, or `main` returned `Err`: an expected failure. |
| 2 | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| 101 | The program panicked: it has a bug. Also a bug in `oskac` itself, which prints `oskac: internal error: <message>` and asks you to report it. |

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

`oskac build [path] [-o out] [--target <platform>|all] [--format=json]` checks a source file or package, as `oskac check` does, and compiles it with the native backend (`oska-native`) to a static executable for a target platform, on any host. `--target` names the platform as release assets name `oskac` builds: `linux-aarch64`, `linux-x86_64`, `macos-aarch64`, `macos-x86_64` or `windows-x86_64` (a Linux asset adds `-gnu` or `-musl` for its build, the glibc build or the musl build; the target names do not, and both builds write the same bytes). Every platform has a backend. A macOS executable is a Mach-O that imports only from `libSystem`, ad-hoc code-signed by `oskac` itself. A Windows executable is a PE32+ console program that imports only from `kernel32.dll`, `ntdll.dll` and `bcryptprimitives.dll`, loads at a randomized address, and never has writable code (ADR 0020). It runs the whole runtime the other platforms run, with file access confined race-free beneath an open handle, as on Linux and macOS. Without `--target`, the build is for the host's platform when it has a backend, else for `linux-aarch64`. It writes the file's stem, or the package directory's name, with `.exe` for Windows, in the current directory unless `-o` names the output. `--target all` writes one executable for each platform, named `<name>-<platform>` (with `.exe` for Windows), into the directory `-o` names, or the current one. It prints nothing on success, or `{"output":"<path>","diagnostics":[]}` with `--format=json` (`{"outputs":["<path>",...],"diagnostics":[]}` for `--target all`). A refused program prints its diagnostics as `oskac check` does, or `{"output":null,"diagnostics":[...]}`. The exit code is 0 on success, 1 for the checker's diagnostics or an output that cannot be written, and 2 for a program the backend cannot compile yet (`E0371`, `E0390`) or could compile only by making a branch or an address depend on a secret (`E0398`, a bug in the backend) or a `--target` it cannot build for (`E0393`, `E0394`). The same program built by the same `oskac` for the same platform gives the same bytes on every host.

`oskac release [path] --version <X.Y.Z> --key <ssh-key> [-o dir] [--format=json]` builds the same executables as `--target all` into one new directory (`<name>-v<X.Y.Z>` unless `-o` names it), writes a `SHA256SUMS` whose first line is `# <name>-release v<X.Y.Z>`, and signs it with `ssh-keygen -Y sign` in the namespace `<name>-release`, writing `SHA256SUMS.sig`: the format of `oskac`'s own releases. A missing or malformed version is refused with `E0395` (exit code 2) and a missing key, one that is not an SSH signing key, or a key `ssh-keygen` cannot sign with, with `E0396` (exit code 1), and a name that cannot name a release with `E0397`, before anything is written; the signature is checked with `ssh-keygen -Y check-novalidate` before the release counts as made, and a failed release removes its directory. See [`oskac release`](guide/cli-reference.md#oskac-release) and [Releasing an application](guide/releasing.md).

The native backend is new and compiles only part of the language so far: `Int` and the sized integer types, `Bool`, text literals and interpolation, the `String` methods (`len`, `byte_len`, `slice`, `repeat`, `trim`, `replace`, `contains`, `starts_with`, `ends_with`, `to_lowercase`, `parse_int`, `split`, `lines`, `chars`, `join`), `String` and `Path` equality, lists of any item type with `List.new`, `push`, `len`, `count`, `get`, `at`, `==`, `map` and `for`, `Path.new` and `Path.from`, `assume_trusted`, `let` and `var`, `if`, `while`, ranges (as values too) and `for` over them, `break` and `continue`, calls of the program's functions (with any number of parameters), recursion included, structs and enums (`Option` and `Result` among them) with field reads and updates, mutating methods, `match` with every pattern form, generic functions and types, each compiled once for every list of type arguments it is used with, closures (capturing by value), `cap fn` values and named functions and methods used as values, `scope`, `spawn` and `join` (tasks run one at a time in spawn order, as in the interpreter), `std.html`, and the process capabilities `main` may declare: `Stdout`, `Stderr`, `Stdin`, `Args`, `Env` (with `Env.secret` and `Secret`), `Clock`, `Random` and `Budget`, `Approver` and `Approval`, with `dbg` (of every value but a directory or file capability, whose interpreter text holds an OS handle) and `todo`, and the filesystem capabilities: `Fs`, `FsRead` and `FsWrite` and every directory and file capability attenuated from them, confined exactly as the interpreter confines them, with the same `IoError` messages and `list_dir` order. A native program's standard output is buffered: it is written when a line goes to stderr or is read from stdin, at exit and on a panic, so the two streams stay in order on a terminal (a terminal gets each line as it is printed). A write of the buffer at exit that fails is the program's `IoError`, as the interpreter's unbuffered write is: its message goes to stderr, as in `no space left on device`, and the program exits with 1. A pipe whose reader has gone is quiet, with 0, as on the interpreter. A fault signal (`SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE`, `SIGTRAP`, `SIGABRT`) writes what the buffer holds, then a report naming the signal on stderr, as in `error: the native program faulted with signal 11 (SIGSEGV)`, and still kills the process, so its exit status and `E0392` under `oskac run` are the same; the handler runs on a stack of its own, so a fault from overflowing the stack is reported too. Letting go of a value takes a bounded native stack however deeply it nests (a chain of five million links included). `Random` is the interpreter's generator, seeded from the operating system for each run, and `Args.get(0)` is the path the executable was run as (`argv[0]`, such as `./hello`); under `oskac run` it is the source file's path or the package directory, as on the interpreter. A root opened from trusted text is named in messages by its canonical path, resolved as the interpreter's `fs::canonicalize` (the C library's `realpath`, glibc's or musl's) resolves it, with no help from `/proc`, and shown as the interpreter shows it: a byte sequence that is not UTF-8 becomes U+FFFD. Lists, structs and enums keep their value semantics: changing a copy never changes the original. A program that uses anything else is refused before any file is written, never compiled wrongly. A program that imports a `std` module with no runtime is refused with `E0371`, as `oskac run` refuses it.

A native executable panics as `oskac run` does: the same report on stderr, `path:line:col: error[P....]: message (bytes a..b)` and its frames under it, with the path as `oskac build` was given it, and exit code 101. Its arithmetic and list indexing are checked exactly as the interpreter's are (`P0001`, `P0004`, `P0002`), and calls nest at most as deep as in the interpreter before `P0021`. It has no `--format=json` report.

A program's `test` blocks compile into a separate **test executable**, which runs one test per process: the one its argument names. A plain test runs on the runtime's test host, which behaves as the interpreter's (see [Tests](#tests)): an empty in-memory filesystem with the same confinement and messages, a clock at the Unix epoch, `Random` from the same fixed seed, recorded stdout and a `[name] ` prefix on each stderr line. An `#[integration]` test runs on the real OS. A program's own executable has no test host at all. `oskac test` reports native test runs exactly as it reports the interpreter's.

On every supported platform, Linux, macOS and Windows, `oskac run` and `oskac test` build natively into a private temporary directory and run the executable; `--interpret` selects the interpreter. The parity gate: a program the native backend refuses (`E0390`) is refused by `run` and `test` too, with the diagnostic `oskac build` prints and exit code 2, and nothing runs; `--interpret` still runs it. `oskac run --format=json` reports a native panic as the interpreter's run does, as one JSON object on stderr. See [platform support](guide/cli-reference.md#platform-support).

| Code | Refusal |
| --- | --- |
| `E0390` | The program uses something the native backend cannot compile yet. The message names the construct, as in ``` `oskac build` cannot compile `dbg` of a `DirRead` yet ```, and says when it is in the runtime, `std.rt`, rather than in the program. Spans the construct, or for the runtime the code of the program that reached it. `oskac run` and `oskac test` refuse it as `oskac build` does, with exit code 2 (the parity gate); `oskac run --interpret` runs it. |
| `E0391` | The executable could not be written: the output's directory does not exist, permission is denied, or the path is a directory. Nothing was written. The fix is another path with `-o`. For `oskac run` and `oskac test` on Linux and macOS, the executable could not be written to, or started from, the temporary directory (one mounted `noexec`, for example): set `TMPDIR` to another directory, or use `--interpret`. The path is a property of the command line, not of a program, so it has no example. |
| `E0392` | A native test process ended in a way no test can make it end: a signal killed it (named, as in `was killed by signal 11 (SIGSEGV)`), it exited with a code other than a pass's or a panic's, or its panic report was garbled. Or the program `oskac run` ran natively died of a signal only a fault raises (`SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE`, `SIGTRAP`, `SIGABRT`); `oskac run` then exits with 128 plus the signal's number. It is a fault in the native backend, not in the test or program, as a crash of `oskac run --interpret` would be one in the interpreter; a test is reported failed with it. The fix is to run with `--interpret` and report the fault. No program is meant to reach it, so it has no example. |
| `E0393` | `oskac build --target` names no supported platform, as in `--target linux-arm64`. The message lists the names it takes: `linux-aarch64`, `linux-x86_64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`, or `all`. It is a usage error (exit code 2), refused before the program is read, and nothing is written. The fix is one of the names. The platform is a property of the command line, not of a program, so it has no example. |
| `E0394` | `oskac build --target` names a supported platform that has no native backend yet. The message names the release that adds it and the platforms it builds for now. Since `windows-x86_64` got its backend (v0.6.0), every supported platform has one, so nothing reports it; the code stays reserved for a platform added later. Exit code 2, refused before the program is read, and nothing is written. The platform is a property of the command line, not of a program, so it has no example. |
| `E0395` | `oskac release` has no `--version`, or one that is not `X.Y.Z` or `X.Y.Z-<pre>`, as in `--version 1.2`. It is a usage error (exit code 2), refused before the program is read, and nothing is written. The fix is a version such as `--version 1.2.0`. The version is a property of the command line, not of a program, so it has no example. |
| `E0396` | `oskac release` has no `--key`, or the file it names cannot be read or is not a private key file, in OpenSSH's format or PEM's (a public key is not one), or `ssh-keygen` could not sign with it or is not installed, or the signature it wrote does not check; the message then quotes what `ssh-keygen` said. Exit code 1. A key is checked before anything is built, and a failed signature removes the release's directory, so nothing unsigned is left. The fix is a key `ssh-keygen -t ed25519 -f <file>` makes. The key is a property of the command line, not of a program, so it has no example. |
| `E0397` | `oskac release` on a program whose name, the file's stem or the package directory's name, cannot name a release: a release's name goes into its file names, the first line of `SHA256SUMS` and the signature's namespace, so it is only ASCII letters, digits, `.`, `-` and `_`, starting with a letter or digit. Exit code 1, before anything is built. The fix is to rename the file or the directory. The name is where the program is kept, not part of the program, so it has no example. |
| `E0398` | An internal error: the native backend's own code for the program would make a conditional branch or a memory address depend on a secret, and the build stops before anything is written. After optimizing, the backend follows every secret through the code it is about to select: the value read out of a `Secret<U8>`, `Secret<U32>`, `Secret<U64>` or `Secret<Bool>`, each byte the runtime reads from a `Secret<Bytes>`, and every value computed from one, into the functions it is passed to and out of the ones that return it. None may be the condition of a branch, an operand of checked arithmetic (which branches on overflow), the address of a load, store or call, or an argument of an indirect call (ADR 0012). The checker's rules (`E0353` to `E0356`) keep a program from asking for any of these, so the error is a bug in `oskac`, never in the program: report it. A secret is not followed through memory: a value stored and loaded back is public to the check, which marks secrets again where they are read out of their `Secret`. Exit code 2, as for `E0390`. No program is meant to reach it, so it has no example. |

### Native build examples

The refusal (`E0390`) has a program that is refused with exactly that code and the same program fixed. `E0391` has no example: the output path is part of the command line, not of a program. `E0392` has no example: it is a fault in the native backend, which no program is meant to reach. `E0393` has no example: the target platform is part of the command line, not of a program. `E0394` has no example: as for `E0393`, the target platform is part of the command line. `E0395` has no example: the release's version is part of the command line. `E0396` has no example: the signing key is part of the command line. `E0397` has no example: the release's name comes from where the program is kept. `E0398` has no example: it is a bug in the native backend, which no program is meant to reach.

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
{"code":"P0004","message":"division by zero: `10 / 0`","path":"file.osk","span":{"start":61,"end":70},"line":3,"column":9,"end_line":3,"end_column":18,"severity":"error","url":"https://oskalang.dev/codes/P0004","related":[],"suggestions":[],"frames":[{"function":"main","path":"file.osk","span":{"start":61,"end":70},"line":3,"column":9}],"frames_omitted":0}
```

Output written before the panic stays written.

A panic carries its call stack, `frames`, innermost first: the function running at the panic, at the panic's own span, then each running call, as the function it was made from at the call's span. Every call of a function, method or closure written in Oska is a frame (a prelude function's is not); `main`, or a test's body, named `test "name"`, is the outermost; a closure is named `<closure in f>` after the function it is written in, and a task's call is made from the function that spawned it. A broken `requires` clause (`P0006`) is the caller's: its frames start at the call. At most 20 frames are listed, and `frames_omitted` counts the rest. Native code and the interpreter report the same frames. The one exception is `P0021` when the interpreter's own host stack runs out before calls nest 50,000 deep, which can happen in a debug build of `oskac`: then its `frames_omitted` counts fewer frames left out. Without `--format=json` they are printed under the panic's line, one `    at function (path:line:column)` each, then `    ... n more frames`. Native code finds them in a call stack: each call it counts (`P0021`) stores its frame's place there, and the runtime walks it at a panic. Its `--max-memory` panic (`P0040`) spans `main`'s body and lists that one frame alone, and a `P0039` that `oskac` reports itself has no frames.

Integers of every type, `Int` and the sized `I8` to `U64`, are checked: arithmetic never wraps. Wrapping is a method a program calls by name: `wrapping_add`, `wrapping_sub` and `wrapping_mul` on `U8`, `U32` and `U64` keep the low bits of the result, and `wrapping_to_u8`, `wrapping_to_u16` and `wrapping_to_u32` keep a wider unsigned value's low bits. A checked conversion, `to_int`, `to_u8`, `to_u16`, `to_u32` or `to_u64` between `Int`, `U8`, `U16`, `U32` and `U64`, panics (`P0001`) when the value does not fit its target.

| Code | Panic |
| --- | --- |
| `P0001` | Integer overflow: the result of `+`, `-`, `*`, `/`, or unary `-` does not fit its type, as with a `U8` `0 - 1` or an `I8` `-128 / -1`, or a conversion `to_<type>()` was given a value its target cannot hold, as with `300.to_u8()` or `(-1).to_u64()`; the message names the conversion, the value and the target's range. Spans the operation or call. |
| `P0002` | Index out of bounds: `List.at`, `Bytes.at` or `Bytes.set` was given an index below 0 or not below the length. Spans the call. |
| `P0003` | `todo()` reached. Spans the call. |
| `P0004` | Division by zero: the right operand of `/` or `%` is zero. Spans the operation. |
| `P0005` | `spawn` on a finished scope: the handle's `scope` block has ended, and work never runs outside a scope. Spans the call. This is a second line of defence behind the checker (`E0292`), which already stops a handle from outliving its block: no checked program can reach this code, so it has no example. |
| `P0006` | Precondition violated: a `requires` clause of the function called is false, so the caller broke the contract. The message names the function and quotes the clause's condition. Spans the call. Every call evaluates the function's `requires` clauses, in every build, in the order written: after the arguments are evaluated and before the body runs. A clause sees the parameters and `self`. A panic while a clause is evaluated, such as an overflow, is reported as it is, not as `P0006`. |
| `P0007` | Postcondition violated: an `ensures` clause of the function that returned is false, so the function broke its promise. The message names the function, quotes the clause's condition, and names the line and column of the return that broke it, counting columns in characters from 1. Spans the `ensures` clause. Every return evaluates the function's `ensures` clauses, in every build, in the order written, with `r` bound to the value being returned. That covers falling off the end of the body (the return is its last expression, or its closing brace), an early `return`, and an early exit through `?`, where `r` is the `Err` or `None` being returned. A clause sees the parameters and `self`; a method that changes `var self` shows its clauses the receiver it ends with. A contract that calls a function with contracts checks those too, and a panic while a clause is evaluated, such as an overflow, is reported as it is, not as `P0007`. |
| `P0008` | Shift or rotation amount out of range: the amount of `<<` or `>>`, or of `rotl` or `rotr`, is below 0 or not below the width of the integer type shifted (8 for `U8`, 32 for `U32`, 64 for `U64`). The amount is never taken modulo the width, so the hardware's differing shift masking never shows: both backends check it first, on every target. The message names the operator, the amount and the range. Spans the operation or call. |
| `P0010` | `t.assert(cond)` was given `false`. Fails the test (see [Tests](#tests)). Spans the call. |
| `P0011` | `t.assert_eq(actual, expected)` was given two values that differ through `Eq`. The message shows both, as `dbg` would. Spans the call. |
| `P0012` | `t.assert_ne(actual, other)` was given two equal values. Spans the call. |
| `P0013` | `t.fail(message)` was called. The message is `message`. Spans the call. |
| `P0014` | `t.assert_snapshot(value)` found no stored snapshot for the value, or one that differs from it. The test runner reports it after the test has run (see [Snapshots](#snapshots)): the message names the snapshot's file and `oskac test --update`, then shows the difference, a line each, `- ` for the stored snapshot and `+ ` for the value. It is also given when a snapshot cannot be read or written, as when its path goes through a symlink. Spans the call. |
| `P0020` | `Env.secret` asked for a variable the environment does not set. Its signature has no room for a missing secret, and running on with a stand-in such as an empty one could let an empty password through. Spans the call. |
| `P0021` | Calls nest too deep: runaway recursion. The interpreter runs on a thread of its own with a 256 MiB stack, and a call panics instead of starting when calls already nest 50,000 deep, or when so little of that stack is left that the next call could overflow it. The process never crashes on deep recursion: `oskac run` exits with 101, and under `oskac test` only the test that recursed fails. Spans the call that would have gone too deep. |
| `P0035` | `String.slice` was given indices out of bounds: a start or end below 0 or past the text's length in characters, or a start past the end. Spans the call. Indices count characters (Unicode code points), not bytes. |
| `P0036` | `String.repeat` was given a negative count. Spans the call. |
| `P0037` | `String.repeat` would build a text over 256 MiB, so the interpreter panics instead of running out of memory. Spans the call. |
| `P0038` | Step limit reached: the program was run with `oskac run --max-steps=<n>`, and has taken `n` steps. A **step** is one call of a function, method or closure written in Oska (a trait method, one the runtime calls such as a `Display` implementation in an interpolation, and a closure that `List.map` calls included), or one loop back-edge: a `while` or `for` loop going back for its next iteration, once for each iteration that ends without `break` ([ADR 0021](adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)). `main` is not called by the program, so it is no step, and neither is a call into the runtime, such as `println`, `List.map` itself or a `std` function the runtime implements. Every such call counts, whoever makes it: an `==` through a type's own `Eq`, a `?` that converts its error with `From`, a call in a `requires` or `ensures` clause, a spawned task, and the `Display` of the error `main` returns, which the runtime calls after `main` has returned (so the limit can stop the program there, before the error is printed, spanning `main`'s body). A `scope` block is no call. The step past the limit is not taken: the program panics instead. The message names the limit and its value. Spans the call, or the whole loop for a back-edge. Without the flag a program has no step limit. Native code counts the same steps as the interpreter, so it stops at the same step. See [Run limits](#run-limits). |
| `P0039` | Time limit reached: the program was run with `oskac run --timeout=<ms>`, and has run for that many milliseconds of wall-clock time, counted from when it starts (after the check and the build). Waiting counts: a program that waits for a line of standard input stops at the deadline too. It stops within 100 ms of the deadline (the **tolerance**) on a machine that is not overloaded. On the interpreter the program panics at the step it has reached (it looks at the clock every 1024 steps, and while it waits), spanning that call or loop, or the read it waits in. Natively, `oskac` stops its own child process, that process only, and prints the report itself, with no place in the source (`error[P0039]: ...`, or a JSON object with no `path`), and exits with 101, never with the signal's `128 + n`; a line the stopped program was writing is not reported as its panic, and what it had printed to standard output but not yet written out (native standard output is buffered when it is not a terminal) is lost. A timeout is not deterministic: where the program stops differs from run to run. The message names the limit and its value. Without the flag a program has no time limit. See [Run limits](#run-limits). |
| `P0040` | Memory limit reached: the program was run with `oskac run --max-memory=<bytes>`, and the value it was about to make would take the bytes of its values (texts, lists, structs, enum values and closures), net of those it let go of, past that many. The value is not made. It is a resource bound, like a timeout: each backend counts in its own way, so the two may stop at different points. The message names the limit and its value. On the interpreter it spans the expression or call that makes the value; natively it spans `main`'s body, since the runtime's allocator does not know which code asked. Without the flag a program has no memory limit. See [Run limits](#run-limits) for what each backend counts. |
| `P0041` | Output limit reached: the program was run with `oskac run --max-output=<bytes>`, and this write would take what it wrote to standard output and standard error together past that many bytes. The write that would pass the limit writes nothing, so no byte past it reaches either stream. The message names the limit and its value. Every write the program makes counts: `println` on `Stdout` and `Stderr`, `dbg`'s line, and the text of the error `main` returns. The panic report itself does not, but a report that quotes text the program made, as `P0020` quotes the secret's name, quotes no more of it than the limit has left, cut where a character ends and followed by `...`. Spans the call that writes, or `main`'s body for its error. Without the flag a program has no output limit. See [Run limits](#run-limits). |
| `P0042` | A `Bytes` range out of bounds: `slice(start, end)` with `start` below 0, `end` past the length, or `start` past `end`; an endian read (`read_u16_be` and the rest) whose bytes do not all lie inside the buffer; `Bytes.zeroed` with a negative length; or `ct_select` between two `Secret<Bytes>` of different lengths. The message names the range or offset and the length, or both lengths. Spans the call. An index out of bounds in `at` or `set` is `P0002`, as in `List.at`. |
| `P0043` | A `Bytes` would grow past 256 MiB: `push`, `append` or an endian push whose result would be longer, or `Bytes.zeroed` with a longer length. The runtime panics instead of running out of memory, and the buffer is not changed. The message names the length it would have had. Spans the call. Not yet checked where a whole buffer is made at once from outside: `String.to_bytes` and the capabilities' `read_bytes` (`DirRead`, `FileRead`) give a buffer of any length without this panic, and a file no memory holds is an `IoError` ("out of memory"). |

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

#### `P0008`

```oska error=P0008
fn main() {
    let x: U32 = 1
    let n: U32 = 32
    let y = x << n
}
```

```oska fixed=P0008
fn main() {
    let x: U32 = 1
    let n: U32 = 31
    let y = x << n
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

#### `P0014`

```oska error=P0014 package tests
==> oska.toml <==
[package]
name = "demo"
version = "0.1.0"
==> src/lib.osk <==
pub fn evens(n: Int) -> List<Int> {
    var out: List<Int> = []
    for i in 0..n {
        out.push(i * 2)
    }
    out
}

test "shows evens" {
    t.assert_snapshot(evens(3))
}
```

```oska fixed=P0014 package tests
==> oska.toml <==
[package]
name = "demo"
version = "0.1.0"
==> src/lib.osk <==
pub fn evens(n: Int) -> List<Int> {
    var out: List<Int> = []
    for i in 0..n {
        out.push(i * 2)
    }
    out
}

test "shows evens" {
    t.assert_snapshot(evens(3))
}
==> snapshots/lib/shows evens.snap <==
[0, 2, 4]
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

#### `P0038`

```oska error=P0038 max-steps=1000
fn main() {
    var i = 0
    while i >= 0 {
        i = i + 1
    }
}
```

```oska fixed=P0038 max-steps=1000
fn main() {
    var i = 0
    while i < 100 {
        i = i + 1
    }
}
```

#### `P0039`

```oska error=P0039 timeout=50
fn main() {
    while true {}
}
```

```oska fixed=P0039 timeout=60000
fn main() {
    var i = 0
    while i < 100 {
        i = i + 1
    }
}
```

#### `P0040`

```oska error=P0040 max-memory=1048576
fn main() {
    var items: List<String> = []
    while true {
        items.push("one more line")
    }
}
```

```oska fixed=P0040 max-memory=1048576
fn main() {
    var items: List<String> = []
    var i = 0
    while i < 100 {
        items.push("one more line")
        i = i + 1
    }
}
```

#### `P0041`

```oska error=P0041 max-output=16
fn main(io: Stdout) {
    while true {
        let _ = io.println("again")
    }
}
```

```oska fixed=P0041 max-output=16
fn main(io: Stdout) {
    var i = 0
    while i < 2 {
        let _ = io.println("again")
        i = i + 1
    }
}
```

#### `P0042`

```oska error=P0042
fn main() {
    let header = Bytes.zeroed(3)
    let length = header.read_u32_be(0)
}
```

```oska fixed=P0042
fn main() {
    let header = Bytes.zeroed(4)
    let length = header.read_u32_be(0)
}
```

#### `P0043`

```oska error=P0043
fn main() {
    let huge = Bytes.zeroed(300000000)
}
```

```oska fixed=P0043
fn main() {
    let small = Bytes.zeroed(3000)
}
```

## Run limits

`oskac run` takes **run limits**, which an embedder (a program that runs code a model wrote) puts on one run. Each stops the program when it is reached with a panic of its own and exit code 101, reported as any panic is, natively and on the interpreter alike.

| Flag | Limit | Panic |
| --- | --- | --- |
| `--max-steps=<n>` | The steps the program takes: calls of functions, methods and closures written in Oska, and loop back-edges (see `P0038`). | `P0038` |
| `--timeout=<ms>` | The wall-clock milliseconds the program runs, waiting included, within a tolerance of 100 ms (see `P0039`). Natively, `oskac` stops the program's process and reports the limit itself. | `P0039` |
| `--max-output=<bytes>` | The bytes the program writes to standard output and standard error together. | `P0041` |
| `--max-memory=<bytes>` | The bytes of the program's values, net of those it let go of. The interpreter counts the bytes it reckons for each value. Native code counts the blocks that `std.rt`'s allocator gives out. See below. | `P0040` |

- A limit is a whole number above 0. A value of 0, a negative number, or anything that is not a number is a usage error (exit code 2), never "no limit". So is a limit given twice, and a limit on any command but `run`.
- A run with no limit flag behaves and performs exactly as before: the native executable is the one `oskac build` writes, byte for byte. A limit is opt-in.
- A step limit stops a program at the same step on every run and every platform ([ADR 0021](adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)), with the same output.
- Natively, each limit is built into the executable `oskac run` makes for that run (the build cache keeps it apart from the one without, and `oskac build` takes no limit). The output limit is counted in `std.rt` behind the system calls that exist ([ADR 0013](adr/0013-system-calls-only-in-std-rt.md)). The step limit is a 64-bit counter in the runtime's writable data that the generated code counts down at each step, in the backend's shared code before register allocation, so every instruction set counts alike; it makes no system call. The two backends stop at the same write, and at the same step, with the same output.
- `--max-memory` is a resource bound, not a parity promise: each backend counts in its own way, so the two may stop at different points, and the report's span differs (see `P0040`). Each checks before it allocates, with the bytes it is about to take, so a value that would pass the limit is never made.
  - **The interpreter** counts each text, list, struct or enum value the program builds, each closure, and each struct the prelude makes for it (a `Range`, `Secret`, `Path` or `IoError`, and `std.html` markup): the bytes the interpreter allocates for it (its text, its list's capacity, its fields and names, and its own record of it), and a list that grows in place counts its new capacity. A value counts until nothing holds it. The small `Option` and `Result` values the prelude's functions return are not counted, though what they hold is, and neither is the interpreter's own working memory. It finds the values let go of when a value would pass the limit; if that frees less than an eighth of the limit, the program stops there too, since it holds nearly all of its limit and keeps allocating.
  - **Native code** counts each block `std.rt`'s allocator gives out once the program starts, for its values and the runtime's own, header included (`std.rt`'s `counted_alloc` and `counted_free`). A block counts until it is freed; a block allocated before the program started is not counted, and gives nothing back when it is freed.
  - Where the platform lets a test measure it, a test shows that the run stops before the process holds much more than the limit: an address-space cap on Linux, the peak resident memory on macOS. Windows has no such measure in the tests; there they check only that the run stops with `P0040`.

## Errors and traits

- **`main` returning `Err(e)`.** The runtime prints `e.to_string()`, the `Display` text of `e`, and a newline to stderr, and exits with code 1.
- **`IoError` wording.** On both backends and every platform, the message of an `IoError` that comes from the operating system is Oska's own wording for the kind of failure, never the system's text or its error number, so the same failure prints the same bytes everywhere ([ADR 0014](adr/0014-every-supported-platform-builds-and-runs-natively.md)). A path the failure concerns comes first, as in `` `data.txt`: not found ``. One table in `oska-sys` (`IO_KINDS`) holds the wordings by kind, and one table per operating system (`LINUX_ERRORS`, `MACOS_ERRORS`, `WINDOWS_ERRORS`) gives the kind of each system error code: the raw `errno` on Linux and macOS, the Win32 error code on Windows. The wording is keyed by that code, never by Rust's `io::ErrorKind`, whose mapping from a code changes between Rust versions, so it does not depend on the compiler that built the toolchain. The interpreter reads these tables directly, `std.rt` carries Linux's and macOS's by error number, and a test checks them for every number. A code in no table says `I/O error`. The confinement refusals (a symlink, a hard link, an escaping or absolute path, a name that is not one component) keep their own wording, listed under [Capabilities](#capabilities).

  | Kind (`IO_KINDS`) | Wording | Linux and macOS `errno` | Windows code |
  | --- | --- | --- | --- |
  | `NotFound` | `not found` | `ENOENT` | `ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`, `ERROR_INVALID_DRIVE`, `ERROR_BAD_NETPATH`, `ERROR_BAD_NET_NAME` |
  | `PermissionDenied` | `permission denied` | `EACCES`, `EPERM` | `ERROR_ACCESS_DENIED` |
  | `AlreadyExists` | `already exists` | `EEXIST` | `ERROR_FILE_EXISTS`, `ERROR_ALREADY_EXISTS` |
  | `NotADirectory` | `not a directory` | `ENOTDIR` | `ERROR_DIRECTORY` |
  | `IsADirectory` | `is a directory` | `EISDIR` | `ERROR_DIRECTORY_NOT_SUPPORTED` |
  | `DirectoryNotEmpty` | `directory not empty` | `ENOTEMPTY` | `ERROR_DIR_NOT_EMPTY` |
  | `StorageFull` | `no space left on device` | `ENOSPC` | `ERROR_DISK_FULL`, `ERROR_HANDLE_DISK_FULL` |
  | `BrokenPipe` | `broken pipe` | `EPIPE` | `ERROR_BROKEN_PIPE`, `ERROR_NO_DATA` |
  | `InvalidData` | `invalid data` | none | none |
  | `InvalidInput` | `invalid input` | `EINVAL` | `ERROR_INVALID_PARAMETER` |
  | `InvalidFilename` | `invalid file name` | `ENAMETOOLONG` | `ERROR_INVALID_NAME`, `ERROR_BAD_PATHNAME`, `ERROR_FILENAME_EXCED_RANGE` |
  | `ReadOnlyFilesystem` | `read-only file system` | `EROFS` | `ERROR_WRITE_PROTECT` |
  | `FileTooLarge` | `file too large` | `EFBIG` | `ERROR_FILE_TOO_LARGE` |
  | `ResourceBusy` | `resource busy` | `EBUSY` | `ERROR_BUSY` |
  | `QuotaExceeded` | `quota exceeded` | `EDQUOT` | `ERROR_DISK_QUOTA_EXCEEDED` |
  | `CrossesDevices` | `cannot move across devices` | `EXDEV` | `ERROR_NOT_SAME_DEVICE` |
  | `TooManyLinks` | `too many links` | `EMLINK` | `ERROR_TOO_MANY_LINKS` |
  | `NotSeekable` | `not seekable` | `ESPIPE` | `ERROR_SEEK_ON_DEVICE` |
  | `OutOfMemory` | `out of memory` | `ENOMEM` | `ERROR_NOT_ENOUGH_MEMORY`, `ERROR_OUTOFMEMORY` |
  | `Unsupported` | `not supported` | `ENOSYS`, `ENOTSUP`, `EOPNOTSUPP` | `ERROR_CALL_NOT_IMPLEMENTED` |
  | `Interrupted` | `interrupted` | `EINTR` | none |
  | `WouldBlock` | `would block` | `EAGAIN` | none |
  | `TimedOut` | `timed out` | `ETIMEDOUT` | `ERROR_SEM_TIMEOUT`, `WAIT_TIMEOUT`, `ERROR_TIMEOUT` |
  | any other | `I/O error` | any other | any other |

  `invalid data` is not reachable from a system error number: the runtime raises it itself, for bytes that are not text (`stream did not contain valid UTF-8`, which names its own cause).
- **`?`** returns `None` or `Err(e)` from the enclosing function or closure. When the function's error type differs, `e` is converted through the `From` impl the checker chose.
- **Trait calls.** A trait method called on a value of a known type reaches that type's impl directly. One called on a value of a type parameter is dispatched on the value's **runtime type tag**, the name of its type without arguments (`Point`, `List`, `String`), to the program's impl for that type or else the prelude's. `Option`, `Result`, and `List` compare their items through `Eq` for the items' type.
- Every integer carries its own type as its tag: `Int`, or a sized type from `I8` to `U64`. A literal takes the type the checker gave it, arithmetic keeps its operands' type, and a range's items keep its bounds' type, so a `U8` reaches the impl for `U8` and an `Int` the one for `Int`.

## Capabilities

`main` receives exactly the capabilities its parameters declare, and an `Args` parameter receives the program's arguments: `args.get(0)` is the program path, and the arguments after `--` follow. A capability is a runtime value that carries its attenuation state, so attenuation narrows authority at runtime as well as in the types.

- `Stdin`, `Stdout`, and `Stderr` reach the standard streams. A failed write is an `IoError`, except one that finds standard output's reader gone (a closed pipe, as in `oskac run app.osk | head -1`): the program ends there, quietly, with exit code 0, on both backends. `Env` reads environment variables; `Env.secret` wraps one in a `Secret`, which stays a wrapper at runtime, so `dbg` prints it as `<redacted>`.
- A `Budget`'s counters live in the runtime, so every copy of a budget draws from the same ones; `main`'s is unlimited, and a test's `t.budget` has 100000 tokens and 100 calls. `limit(tokens, calls)` makes a child clamped to what its parent has left (a negative amount counts as 0). `reserve(tokens)` holds the tokens and one call on the budget and every budget above it, or returns `OverBudget` when any of them has less left, or has no tokens left at all; the `Reservation`'s first `debit(input, output, cache_creation, cache_read)` spends the call and the sum of those tokens and returns the reservation, its first `release()` returns it unspent, and any later one changes nothing. `remaining()` is the least any budget in the chain has left, and `spent()` what the budget and those limited from it spent. A reply may use more tokens than were reserved, since input tokens are known only after it, so a budget can end over its limit by at most the last reply's excess; with no tokens left it then refuses every reservation, even one of no tokens. Counters stop at the largest `Int` rather than overflow, and an unlimited budget's reserved count at 0 rather than below. Natively, the counters of a budget that `limit` makes are never freed (64 bytes each) and count under `--max-memory`, where the interpreter's do not. `dbg` shows `<Budget>` and `<reservation>`.
- `Approver.terminal(stdin, stderr)` makes an approver whose `confirm(summary, details)` writes `approval needed: ` and the summary, then each detail indented by two spaces, both shown as `dbg` shows text, and `approve? [y/N]`, a line each, to stderr, then reads a line from stdin: `y` or `yes`, trimmed and in any case, gives `Some(Approval)`, and anything else, end of input, or a failed write or read gives `None`. Under `--max-output` the question counts as output. An `Approval`'s record of whether it was used lives in the runtime, so every copy shares it: the first `consume(summary, details)` that matches succeeds, every later one is `ApprovalError.Used` (and `matches` is false once it is used), and one that does not match is `ApprovalError.Mismatch` and uses nothing. In a test every approver is `t.approver` (a test has no `Stdin` to make another), which writes nothing: it records each question's summary and details for `t.approvals_asked()` and answers from `t.answer_approval`, refusing when the script is empty. A program may build a `TestContext` by hand, but its approvers are terminal ones: there `t.answer_approval` does nothing and `t.approvals_asked()` is empty, on both backends. `dbg` shows `<approval>`.
- `Clock.now_millis()` is the time in milliseconds since the Unix epoch. `Random.next_int()` comes from a built-in pseudo-random generator (SplitMix64, not cryptographically secure), seeded from the clock and the process.
- `Fs`, `FsRead`, and `FsWrite` grant no file access themselves: they have no method that reads, lists, writes, or creates (a call of one is `E0273`), only attenuations. Code that needs the whole filesystem says `restrict_to("/")`. `restrict_to` attenuates one to a **directory capability**, `Dir`, `DirRead`, or `DirWrite`, which carries a root and the same access. `read_only` and `write_only` keep the root and narrow the access, and a read or write the access does not allow is refused. `restrict_to_file` on `FsRead` and `FsWrite`, or on a `DirRead` or `DirWrite`, attenuates to a **file capability**, `FileRead` or `FileWrite`, for exactly one file: `read_to_string()` and `write(text)` take no path, and no capability both reads and writes a file. Only attenuation makes a directory or file capability: `main` declaring one is refused by the checker (`E0217`).
- `write(path, text)` creates the file or replaces what it holds. `create_dir(path)` creates one directory, whose parent must exist. A directory already there is kept, with what it holds, so a program can rebuild into it; a file there is an error. `list_dir(path)` gives the names of a directory's entries, sorted by their bytes; a symlink is listed by its own name, and not followed.
- **Confinement.** `restrict_to(dir)` opens `dir` as the root, which must be a directory, and the directory capability holds it from then on. On `Fs`, `FsRead`, and `FsWrite`, `dir` is trusted text, any path whose last component is not a symlink; earlier components are followed, since the root is trusted text. On a directory capability, `dir` is a `Path`, which may come from untrusted text, walked beneath the current root as below, so a narrowing never widens. Attenuation never fails: a root that is missing, a symlink, not a directory, or outside the current root gives a directory capability that reaches nothing, and every operation on it fails with an `IoError` naming the root and the reason. A file passed to `restrict_to` is such a root: it is not a directory.
- **File capabilities.** `restrict_to_file(file)` opens the file's parent directory as `restrict_to` opens a root (on a directory capability, by walking the `Path` beneath the current root), and holds that handle together with the file's name. The file itself is not opened then, so a `FileWrite` can create it. Each use opens the name beneath the parent's handle without following a symlink, so a file replaced between uses can never lead outside: a symlink in its place is refused, and a parent swapped for a symlink is not met, because the capability holds the original. A parent that is missing, or beneath a symlink, makes every operation fail with an `IoError` naming the file; so does a name that is not a regular file.
- **Every operation walks beneath the root.** `read_to_string`, `write`, `create_dir`, `list_dir`, and narrowing with `restrict_to` resolve `path` like this, and a refusal is an `Err(IoError)`:
  1. An empty path is refused, and so is an absolute one, even one inside the root.
  2. `.` is dropped, and `..` drops the name before it, on the path as written. A `..` that would climb above the root is refused; `a/../b` is `b`, even when `a` is a symlink.
  3. `write` and `create_dir` need a name of their own: the path as written must end in a plain name, not in a separator, `.`, or `..`. `dang/` and `sub/.` are refused, even though path libraries treat them as `dang` and `sub`.
  4. The runtime holds an OS handle for the root, opened when the filesystem was restricted and released with the last copy of it. Each component is opened relative to the one before, starting from that handle, and none is followed if it is a symlink, the last included (the `oska-sys` crate, [ADR 0009](adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)). On Windows each name is opened with `NtCreateFile` beneath the handle of the one before, opening a reparse point itself rather than following it ([ADR 0014](adr/0014-every-supported-platform-builds-and-runs-natively.md)). A read opens its file beneath its parent's handle. A write opens or creates its file there, and empties it only once it is known to be a regular file, so a write never goes through a symlink and creates nothing outside the root. `create_dir` makes its directory beneath its parent's handle, and `list_dir` lists through the directory's own handle. There is no separate check to race: a symlink at any component, planted before the operation or swapped in during it, makes it fail. A native executable (`oskac build`) walks the same way with its own system calls (`openat` with `O_NOFOLLOW`, in the toolchain's runtime, `std.rt`), with the same refusals and messages.
  5. **Every symlink beneath the root is refused**, dangling or not, whether it points inside the root or outside it. This is a change: before #94 and #95, a symlink that resolved inside the root was followed. On Windows every reparse point is refused the same way, with the same message (`` `name` is a symlink, and confined access never follows one ``): a symlink, a junction, a volume mount point, or any other.
  6. **A hard-linked file is refused**. A hard link is the same file under a second name, so one planted beneath the root, such as in an upload directory, could expose or overwrite a file outside it. A read or a write that opens an existing regular file with more than one link fails with an `IoError` (`` `name` has more than one hard link, and confined access never opens one ``), checked on the open handle before anything is read or the file is emptied. A write that creates a new file is unaffected.
  6. A file that is not a regular one, such as a FIFO or a device, is refused rather than read or written.

  **Windows names.** Every name walked beneath the root is also refused on Windows when it is a reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, with or without an extension, as in `nul.txt`), or when it holds a `:`, which opens an alternate data stream. The walk opens each name beneath its parent's handle, where a device name would reach a plain file, but Windows' path functions, which other programs open files with, may take that name for the device, so such a file could be written and never read back or removed by path. The refusal keeps the names a program may use the same as before the walk opened names beneath a handle.

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
  - `t.budget` is a `Budget` of 100000 tokens and 100 calls, the same in every test.
  - `t.approver` is an `Approver` that answers from the test's script: `t.answer_approval(true)` or `t.answer_approval(false)` queues its next answer, and a question with no answer left is refused.
  - `t.stdout` records what it prints. `t.printed()` returns every line printed so far in the test, one per `println`, without its line break.
- **Assertions.** `t.assert`, `t.assert_eq`, `t.assert_ne`, and `t.fail` fail the test by panicking, with `P0010` to `P0013` and the span of the call. A failed assertion is a panic like any other: it stops the test, and its diagnostic is the test's failure. A panic from a bug, such as `P0004`, fails the test the same way.
- **Snapshots.** `t.assert_snapshot(value)` compares `value`, rendered as `dbg` renders it, with its snapshot: see [Snapshots](#snapshots).
- **Isolation.** A panic stops only the test it happened in, runaway recursion (`P0021`) included. Each test runs in a new interpreter, on a new host, on a thread of its own.
- **Integration tests.** A test marked `#[integration]` is skipped unless `--integration` is given. Then it runs on the real OS: `t.fs` is an `Fs` over the real filesystem (attenuate it like any `Fs`), `t.clock` the real time, and `t.random` seeded from the clock. `t.budget` and `t.approver` are the same as in any test: `t.approver` still answers from its script and never asks a person. Its `t.stdout` is still recorded, not written to the terminal, so `t.printed()` works and the report is the only thing on stdout.

### Snapshots

`t.assert_snapshot(value)` renders `value` as `dbg` renders it (so a secret is `<redacted>`) and compares it with one file per assertion, `snapshots/<test-file>/<test-name>.snap`. In a package, `snapshots/` is in the package's directory, and `<test-file>` is the test's file under `src/` without `.osk`, such as `lib` or `util/text`; for a single file, `snapshots/` is beside it, and `<test-file>` is its name without `.osk`. A file holds the rendered value and a line break, and a stored `\r\n` reads as `\n`.

- **Names.** A test that runs one snapshot assertion has `<test-name>.snap`. A test that runs more numbers them in run order, the order the assertions run in: `<test-name>-1.snap`, `<test-name>-2.snap`, and so on. For a straight-line test, run order is source order. An assertion in a function the test calls, or in a loop, gets a number each time it runs. A test runs on stand-ins, so the order is the same on every run and on both backends. Each character a file name cannot hold on some platform (`/ \ : * ? " < > |` and control characters) becomes `_`, and so does a `.` or space at either end of the name, so every snapshot stays in its directory; a name Windows keeps for a device (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, in any case and with any extension) gets a `_` before it. Two tests whose snapshots would get the same file, such as `a/b` and `a_b`, or `x-1` and a test `x` with two assertions, both fail with `P0014`, naming each other, and neither's snapshots are compared or written: rename one of them.
- **Symlinks.** The test runner follows no symlink beneath the package's directory or the file's: when `snapshots/`, a directory in it, or a snapshot file is a symlink, it neither reads nor writes through it, and the test fails with `P0014`. A snapshot is written to a new file in its directory, which then replaces the old one, so a symlink put there in the meantime is replaced, not followed.
- **Comparing.** The test does not read or write a snapshot: it has only its stand-in capabilities. Each backend records what each assertion was given; after the test, the test runner compares it with the file. A missing or different snapshot fails the test with `P0014`, which shows the difference and names `oskac test --update`; its frames are the call stack at the assertion, as a panic there would list them. Since the comparison comes after the test, the test runs on past the assertion. A test that panics fails with its panic, whatever its snapshots: they are neither compared nor written, since the test did not run to its end.
- **Updating.** `oskac test --update` writes each missing or different snapshot instead of failing, and lists what it wrote, each `new` or `changed`: under `snapshots written:` before the summary, which ends `, n snapshots written`, or in the JSON report's `"snapshots"`. It never deletes a snapshot, so one a renamed or removed test left behind stays until you delete it. Review the written snapshots as a diff before committing them.

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

With `--format=json`, one JSON object on stdout. Each test has its name, in a package its `module` (such as `"module":"util.text"`, right after `name`), its status (`passed`, `failed`, or `skipped`), its `duration_ms`, an integer, the whole milliseconds the test took (on the interpreter its run on a fresh interpreter; natively its process from start to exit, process start included; 0 for a skipped test), and its failure, a diagnostic object or `null`, whose `path` is relative to the package. With `--filter`, `"filtered"` after `skipped` counts the tests the filter left out, which are neither run nor listed. With `--update`, `"snapshots"` after those lists the snapshots it wrote, each `{"path":"snapshots/lib/shows evens.snap","status":"new"}`, its path from the package or the file's directory, and its status `new` or `changed`. A refused file has no tests and its diagnostics in `diagnostics`:

```json
{"tests":[{"name":"adds","status":"passed","duration_ms":0,"failure":null},{"name":"divides","status":"failed","duration_ms":3,"failure":{"code":"P0011","message":"...","path":"math.osk","span":{"start":120,"end":141},"line":12,"column":5,"end_line":12,"end_column":26,"severity":"error","url":"https://oskalang.dev/codes/P0011","related":[],"suggestions":[],"frames":[{"function":"test \"divides\"","path":"math.osk","span":{"start":120,"end":141},"line":12,"column":5}],"frames_omitted":0}}],"passed":1,"failed":1,"skipped":0,"diagnostics":[]}
```

| Exit code | Meaning |
| --- | --- |
| 0 | No test failed. Skipped tests do not count. |
| 1 | A test failed, or the checker refused the file. |
| 2 | The file uses something the interpreter cannot run yet (`E0371`, `E0380`). No test ran. |
