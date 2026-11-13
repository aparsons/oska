# CLI reference

`oskac` is the Oska compiler and runner. This page is checked against `oskac --help` when the documentation is tested: every command and flag in the help text appears here, and nothing else does.

```help
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check [path] [--format=json] [--release]
                                           Type-check a source file, or a
                                           package directory (the current one
                                           with no path), and report each
                                           function's authority
    oskac run [path] [--format=json] [--interpret] [-- args...]
                                           Check a source file or package and,
                                           if it has no diagnostics, run it:
                                           natively on aarch64 Linux, on the
                                           interpreter elsewhere or with
                                           --interpret
    oskac test [path] [--format=json] [--integration] [--interpret]
                                           Run the test blocks in a source file
                                           or package, natively or interpreted
                                           as `run` does
    oskac build [path] [-o out] [--format=json]
                                           Check a source file or package and,
                                           if it has no diagnostics, compile it
                                           to an aarch64 Linux executable
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
```

`lex` and `parse` take exactly one source file, and `explain` a code. `check`, `run`, `test` and `build` take a source file, a package directory, or nothing, which means the package in the current directory. A package is a directory with an `oska.toml` (`[package]` with a `name` and a `version`) and its modules under `src/`; every `.osk` file there is a module, `main` is allowed only in `src/main.osk`, and a package without that file is a library, which `run` and `build` refuse. A directory without `oska.toml` is an error (`E0413`). A file inside a package is handled alone, with a hint to run the command on the package. A command line is parsed like this:

- A flag may come anywhere before `--`. Anything else is a positional argument.
- Everything after `--` belongs to the program being run, and only `oskac run` uses it.
- Any `--format=` value other than `json` means the default, human-readable output.
- An unknown flag is treated as a positional argument, so the command is not recognized. `oskac` then prints the usage text to standard error and exits with 2. Running `oskac` with no arguments does the same.
- A file that cannot be read prints `error: cannot read <path>: <reason>` to standard error and exits with 1, even with `--format=json`.

## Flags

| Flag | Used by | Effect |
| --- | --- | --- |
| `--format=json` | every command | Machine-readable output. See [output formats](#output-formats). |
| `--release` | `check` | Reject `dbg(...)` (`E0216`) and `todo()` (`E0293`). The other commands accept the flag and ignore it. |
| `--integration` | `test` | Also run the tests marked `#[integration]`, on the real operating system. |
| `--interpret` | `run`, `test` | Run on the interpreter, on aarch64 Linux too, where the program is otherwise compiled and run natively. See [platform support](#platform-support). The other commands accept the flag and ignore it. |

## `oskac lex`

`oskac lex <file.osk>` prints the tokens of a source file, one per line, as `start..end` byte offsets and the token. It is a debugging aid, and the exact text of a token line is not a stable interface. With `--format=json`, a lexical error is printed as a diagnostic object on standard output. Exit code 0 on success, 1 on a lexical error.

## `oskac parse`

`oskac parse <file.osk>` prints the syntax tree as indented text. With `--format=json` it prints the documented AST export, described in the [AST reference](../ast.md). A parse error is printed as one diagnostic object on standard output with `--format=json`, or as text on standard error. Exit code 0 on success, 1 on a parse error.

## `oskac check`

`oskac check [path] [--format=json] [--release]` parses and type-checks one file, or every module of a package, against the prelude and reports each function's authority. Exit code 0 when there are no diagnostics, otherwise 1.

Human-readable output:

- Diagnostics go to standard error, one per line, as `file.osk:1:8: error[E0210]: message (bytes 7..13)`: the file, then the line and column where the span starts. A diagnostic with a fix is followed by an indented `fix: ...` line.
- The authority report goes to standard output: one line per function and method, each followed by its attenuations (indented), then one line per audit site.

```oska
fn double(n: Int) -> Int {
    n * 2
}

fn vouch(name: Untrusted<String>, io: Stdout) -> Result<(), IoError> {
    io.println(name.assume_trusted())
}
```

```console cmd=check
fn double: pure
fn vouch: Stdout (io)
main.osk:6:16: audit: assume_trusted (bytes 126..147)
```

The line `fn vouch: Stdout (io)` names each capability and the parameter that supplies it. A `cap fn` parameter appears as `cap fn (name)`. A parse error is reported alone, with no authority lines. Terms are in the [glossary](../../GLOSSARY.md). The checker's behavior and its full code table are in the [checker notes](../checker.md).

## `oskac run`

`oskac run <file.osk> [--format=json] [--interpret] [-- args...]` checks the file and, when it has no diagnostics, runs `main` on the real operating system: compiled natively on aarch64 Linux, on the interpreter elsewhere or with `--interpret` (see [platform support](#platform-support)). `args.get(0)` in the program is the file's path, or the package directory as given, on either backend, and the arguments after `--` follow it unchanged.

- The program's own output goes to standard output and standard error as it runs.
- A refused program's diagnostics, or the panic that stopped the program, go to standard error after that output: as `check` prints diagnostics, or as one JSON object per line with `--format=json`.
- The exit code is the program's.

| Exit code | Meaning |
| --- | --- |
| `0` | `main` returned `Ok` or `()`, or the reader of the program's standard output went away (a closed pipe, as in `oskac run app.osk \| head -1`): the program ends quietly at the write that finds out. On standard error, or with any other failure, such as a full disk, the write is an `IoError`. `oskac`'s own output ends the same way. |
| `1` | The checker refused the program, or `main` returned `Err`. The runtime prints `e.to_string()` to standard error. |
| `2` | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| `101` | The program panicked. |
| `128 + n` | A native program was killed by signal `n`, as a shell reports it, such as `143` for `SIGTERM`. A signal only a fault raises, such as `SIGSEGV`, is also reported as `E0392`. A closed pipe is not a signal on either backend: see exit code `0`. |

See [debugging](debugging.md) for panics and the [runtime notes](../runtime.md) for every runtime code.

## `oskac test`

`oskac test <file.osk> [--format=json] [--integration] [--interpret]` checks the file and runs its `test` blocks in source order: natively on aarch64 Linux, one process per test, or on the interpreter, as `run` chooses. The report is the same either way. The report goes to standard output: one line per test, the failures, and a summary, or one JSON object with `--format=json`. A refused file's diagnostics go to standard error, or into the JSON report's `diagnostics`. See [testing](testing.md).

| Exit code | Meaning |
| --- | --- |
| `0` | No test failed. Skipped tests do not count. |
| `1` | A test failed, or the checker refused the file. |
| `2` | The file uses something the interpreter cannot run yet. No test ran. |

## `oskac build`

`oskac build [path] [-o out] [--format=json]` checks a file or package as `check` does and, when it has no diagnostics, compiles it to a static executable for aarch64 Linux. Building works on every host and writes the same bytes on each. The executable is the file's stem, or the package directory's name, in the current directory; `-o out` names it instead.

- Success prints nothing, or `{"output":"<path>","diagnostics":[]}` with `--format=json`.
- A refused program's diagnostics go to standard error as `check` prints them, or to standard output as `{"output":null,"diagnostics":[...]}` with `--format=json`.
- The native backend compiles only part of the language so far. Anything else is refused with `E0390` before a file is written, never compiled wrongly; `oskac run` still runs the program, on the interpreter.
- The executable needs nothing else to run: no `oskac`, no C library. Run directly, it reports a panic as `oskac run` does (human-readable only), and `args.get(0)` is the path it was run as, such as `./hello`, where `oskac run` reports the source file's path.

| Exit code | Meaning |
| --- | --- |
| `0` | The executable was written. |
| `1` | The checker refused the program, or the executable could not be written (`E0391`). |
| `2` | The program uses something the native backend cannot compile yet (`E0371`, `E0390`). |

See the [runtime notes](../runtime.md#native-builds) for the codes.

## Platform support

| Host | `oskac run`, `oskac test` | `oskac build` |
| --- | --- | --- |
| aarch64 Linux | Native: compiled, then run | aarch64 Linux executable |
| Any other (x86_64 Linux, macOS, Windows) | Interpreter | aarch64 Linux executable |

On aarch64 Linux, `run` and `test` compile the program as `build` does and run the executable, which is 45 to 85 times faster on program logic, such as loops, lists and calls (see the [benchmark](../design.md#native-backend-benchmark)). `--interpret` runs the interpreter instead, to compare the backends when a native result looks wrong. Every other host keeps the interpreter until it has a backend of its own. Either way the output, the reports and the exit codes are the same: the two backends are held to it by the differential suite.

- The executable is written to a new private directory under the temporary directory (`TMPDIR`, else `/tmp`): a random name, created with mode `0700`, never reused, so no other user can replace or read it. It is removed as soon as the program has started, or for `test` once every test has run.
- A program the native backend cannot compile yet (`E0390`) runs on the interpreter, after one line on stderr that says so and why, such as `app.osk:3:13: note: running on the interpreter: ... (E0390)` (with `--format=json`, one JSON object with `"severity":"note"`). It is a checked program that the interpreter runs, so the output is the same, only slower. `--interpret` prints no note. A program the interpreter cannot run either, such as one that imports a module with no runtime (`E0371`), gets no note: both refuse it, and the interpreter's refusal is reported.
- If the executable cannot be written or started, for example because the temporary directory is mounted `noexec`, the command fails with `E0391`. Set `TMPDIR` to another directory or use `--interpret`.

## `oskac --version` and `oskac --help`

`oskac --version` prints `oskac` and the version on one line, such as `oskac 0.3.0`, and `oskac --help` prints the text at the top of this page. Both also answer to the short forms `-V` and `-h`. With `--format=json`, `--version` prints a JSON object instead: `{"name":"oskac","version":"0.3.0"}`.

## Output formats

### Diagnostics

Every message about a program has one shape, whichever command produced it:

```json
{"code":"E0210","message":"...","path":"app.osk","span":{"start":7,"end":13},"line":1,"column":8,"end_line":1,"end_column":14,"severity":"error","url":"https://oskalang.dev/codes/E0210","fix":{"message":"declare it `cap struct`","edits":[{"span":{"start":0,"end":0},"replacement":"cap "}],"applicability":"machine"},"related":[],"suggestions":[]}
```

- `code` is stable. Tools should match on it, never on `message`. Codes starting `E00` are lexical, `E01` syntax, `E02` and `E03` the checker, and `E037` and `E038` runtime refusals. `P` codes are panics.
- `path` is the file the diagnostic is in: package-relative in a package, as given on the command line for a single file.
- `span` is a range of UTF-8 byte offsets, `start` included and `end` excluded.
- `line` and `column` are where `span` starts, and `end_line` and `end_column` where it ends, all 1-based and counting characters, as the human output's `path:line:column` does. They describe `span` and never replace it. A diagnostic about the run rather than a place in a file (`E0391`, `E0392`) has no `path`, and none of these four.
- `severity` is `error`, or `note` for a message that refuses nothing, such as the native backend handing a program to the interpreter.
- `url` is the code's page on the documentation site.
- `fix` is present when the checker offers a repair. Each edit replaces the bytes of `span` with `replacement`, in the diagnostic's file, or in the file the fix's own `path` names when it has one. Apply the edits of one fix from the end of the file to the start, so earlier offsets stay valid. A fix can be absent, and when it is, the repair needs judgment.
- `applicability` says how safe a fix is to apply without reading it: `machine` when it is certainly right, `maybe` when it is probably what was meant but a guess, such as the closest name to an unknown one, and `placeholders` when the program checks afterwards but code is left to write, such as `match` arms whose body is `todo()`. Each code's fix has one applicability, which `oskac explain` and the code's page give. The human output marks the uncertain ones as `fix (maybe):` and `fix (placeholders):`.
- `related` lists other places the diagnostic is about, each with a `message`, `path`, `span`, `line` and `column`, such as a name's first definition, the trait's declaration of a method, or the module an import looked in. It is filled only where the checker knows the place, and is often empty.
- `suggestions` is where repairs that are plausible but uncertain go, in the shape of a `fix` with the applicability `maybe`. No tool applies them automatically. It is empty for every code today.

The format only grows. No field is renamed, retyped or given a new meaning, a diagnostic that has no `fix` never gains one, and a new uncertain repair goes into `suggestions`, never into `fix`. A consumer must accept values of `severity` and `applicability` that are not listed here, and fields it does not know.

### `check --format=json`

One object on standard output, produced even when there are diagnostics:

```json
{
  "diagnostics": [ ... ],
  "functions": [
    {"name":"vouch","authority":["Stdout"],"via":[{"param":"io","capabilities":["Stdout"]}],"pure":false}
  ],
  "audit": [
    {"kind":"assume_trusted","span":{"start":126,"end":147}}
  ],
  "attenuations": [
    {"function":"main","method":"restrict_to","span":{"start":88,"end":112},"source":"FsRead","result":"DirRead","root":"docs"}
  ]
}
```

- `diagnostics`, in source order.
- `functions`: one entry per function and method, in source order. `name` is the function's name, `Type.method` for a method of an inherent `impl`, or `<Type as Trait>.method` for one of `impl Trait for Type`, with a generic trait's arguments, as in `<AppError as From<IoError>>.from`; in a package `pkg.` and the module path come first, as in `pkg.util.text.shout` or `pkg.main.main`, and a method is named by its type's module. No two entries share a name. `"pub":true` marks a function declared `pub`. `authority` lists the built-in capabilities it can reach, in order of first appearance. `via` lists each capability-bearing parameter and what it supplies, with a method's receiver as `self`. `pure` is `true` when no parameter, the receiver included, is capability-bearing.
- `audit`: every audit site, in source order. `kind` is `assume_trusted` for a trust assertion or `expose` for a secret exposure, and `span` covers the whole call.
- `attenuations`: every call that narrows a capability, in source order. `function` is the function it is in, `method` is `restrict_to`, `restrict_to_file`, `read_only` or `write_only`, `source` and `result` are the capability kinds before and after, and `root` is the string the capability is restricted to, or `null` when the method takes no root or the root is computed at runtime. `span` covers the whole call.

A parse error is reported alone, with empty `functions`, `audit` and `attenuations`.

### `test --format=json`

```json
{"tests":[{"name":"adds","status":"passed","failure":null}],"passed":1,"failed":0,"skipped":0,"diagnostics":[]}
```

`status` is `passed`, `failed`, or `skipped`. `failure` is a diagnostic object or `null`.

### `run --format=json`

The program's output is unchanged. Each diagnostic or panic is one JSON object per line on standard error.

## Diagnostic codes

Every code has an entry with its meaning, a program that fails with it, and the same program fixed. The entries are in the [syntax and checker notes](../checker.md#syntax-diagnostic-codes) (`E00xx` to `E03xx`) and the [runtime notes](../runtime.md) (`E037x`, `E038x`, and the `P` codes), and `oskac explain` prints them offline. The documentation site has a page for each at `/codes/E0300`, an index of them at `/codes/`, and all of them as data in `/codes.json`. For symptoms and their fixes, see [troubleshooting](troubleshooting.md).

## `oskac explain`

`oskac explain <code> [--format=json]` prints the catalog entry for one diagnostic code: the same entry as that code's page on the documentation site, but offline. A code is written as in a diagnostic, such as `E0300` or `P0004`, in any case.

```sh
oskac explain E0300
oskac explain E0300 --format=json
```

The human-readable entry has the code and its meaning, whether the checker offers a machine-applicable fix for it and what the fix does, a program that fails with the code, and the same program fixed. A code that no program can reach, such as `P0005`, has no example.

With `--format=json` it prints one JSON object on one line, the same object that `/codes.json` holds for the code:

| Field | Value |
| --- | --- |
| `code` | The code, such as `"E0300"`. |
| `message` | What the code means, as Markdown. |
| `machine_fix` | `true` when a diagnostic with this code can carry a machine-applicable fix. |
| `machine_fix_description` | What that fix does, or `null`. |
| `fix_applicability` | How safe that fix is to apply, as a diagnostic's `fix` gives it: `"machine"`, `"maybe"` or `"placeholders"`, or `null`. |
| `example` | A program that fails with exactly this code, or `null`. |
| `fix` | The same program fixed, or `null`. |

Unlike the other commands, `explain` takes a code and not a file. A code that does not exist is an error with exit code 2. The message suggests the closest code when one is near, as in ``error: unknown diagnostic code `<code>`; did you mean `<closest>`?`` on standard error. With `--format=json` the error is one object on standard output instead: `{"error":"unknown code","code":"<code>","suggestion":"<closest>"}`, with `null` for a suggestion when nothing is near.
