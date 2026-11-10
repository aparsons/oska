# CLI reference

`oskac` is the Oska compiler and runner. This page is checked against `oskac --help` when the documentation is tested: every command and flag in the help text appears here, and nothing else does.

```help
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check <file.osk> [--format=json] [--release]
                                           Type-check a source file and report
                                           each function's authority
    oskac run <file.osk> [--format=json] [-- args...]
                                           Check a source file and, if it has
                                           no diagnostics, run it
    oskac test <file.osk> [--format=json] [--integration]
                                           Run the test blocks in a source file
    oskac --version                        Print the version
    oskac --help                           Print this help
```

Every command takes exactly one source file. A command line is parsed like this:

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

## `oskac lex`

`oskac lex <file.osk>` prints the tokens of a source file, one per line, as `start..end` byte offsets and the token. It is a debugging aid, and the exact text of a token line is not a stable interface. With `--format=json`, a lexical error is printed as a diagnostic object on standard output. Exit code 0 on success, 1 on a lexical error.

## `oskac parse`

`oskac parse <file.osk>` prints the syntax tree as indented text. With `--format=json` it prints the documented AST export, described in the [AST reference](../ast.md). A parse error is printed as one diagnostic object on standard output with `--format=json`, or as text on standard error. Exit code 0 on success, 1 on a parse error.

## `oskac check`

`oskac check <file.osk> [--format=json] [--release]` parses and type-checks one file against the prelude and reports each function's authority. Exit code 0 when there are no diagnostics, otherwise 1.

Human-readable output:

- Diagnostics go to standard error, one per line, as `file.osk: error[E0210]: message (bytes 7..13)`. A diagnostic with a fix is followed by an indented `fix: ...` line.
- The authority report goes to standard output: one line per function and method, then one line per audit site.

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
main.osk: audit: assume_trusted (bytes 126..147)
```

The line `fn vouch: Stdout (io)` names each capability and the parameter that supplies it. A `cap fn` parameter appears as `cap fn (name)`. A parse error is reported alone, with no authority lines. Terms are in the [glossary](../../GLOSSARY.md). The checker's behavior and its full code table are in the [checker notes](../checker.md).

## `oskac run`

`oskac run <file.osk> [--format=json] [-- args...]` checks the file and, when it has no diagnostics, runs `main` on the real operating system. `args.get(0)` in the program is the file's path, and the arguments after `--` follow it.

- The program's own output goes to standard output and standard error as it runs.
- A refused program's diagnostics, or the panic that stopped the program, go to standard error after that output: as `check` prints diagnostics, or as one JSON object per line with `--format=json`.
- The exit code is the program's.

| Exit code | Meaning |
| --- | --- |
| `0` | `main` returned `Ok` or `()`. |
| `1` | The checker refused the program, or `main` returned `Err`. The runtime prints `e.to_string()` to standard error. |
| `2` | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| `101` | The program panicked. |

See [debugging](debugging.md) for panics and the [runtime notes](../runtime.md) for every runtime code.

## `oskac test`

`oskac test <file.osk> [--format=json] [--integration]` checks the file and runs its `test` blocks in source order. The report goes to standard output: one line per test, the failures, and a summary, or one JSON object with `--format=json`. A refused file's diagnostics go to standard error, or into the JSON report's `diagnostics`. See [testing](testing.md).

| Exit code | Meaning |
| --- | --- |
| `0` | No test failed. Skipped tests do not count. |
| `1` | A test failed, or the checker refused the file. |
| `2` | The file uses something the interpreter cannot run yet. No test ran. |

## `oskac --version` and `oskac --help`

`oskac --version` prints `oskac` and the version on one line, such as `oskac 0.0.1`, and `oskac --help` prints the text at the top of this page. Both also answer to the short forms `-V` and `-h`. `--version` prints plain text whatever `--format` says.

## Output formats

### Diagnostics

Every message about a program has one shape, whichever command produced it:

```json
{"code":"E0210","message":"...","span":{"start":7,"end":13},"fix":{"message":"declare it `cap struct`","edits":[{"span":{"start":0,"end":0},"replacement":"cap "}]}}
```

- `code` is stable. Tools should match on it, never on `message`. Codes starting `E00` are lexical, `E01` syntax, `E02` and `E03` the checker, and `E037` and `E038` runtime refusals. `P` codes are panics.
- `span` is a range of UTF-8 byte offsets, `start` included and `end` excluded.
- `fix` is present when the repair is unambiguous. Each edit replaces the bytes of `span` with `replacement`. Apply the edits of one fix from the end of the file to the start, so earlier offsets stay valid. A fix can be absent, and when it is, the repair needs judgment.

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
  ]
}
```

- `diagnostics`, in source order.
- `functions`: one entry per function and method, in source order. `name` is the function's name, or `Type.method`. `authority` lists the built-in capabilities it can reach, in order of first appearance. `via` lists each capability-bearing parameter and what it supplies, with a method's receiver as `self`. `pure` is `true` when no parameter, the receiver included, is capability-bearing.
- `audit`: every audit site, in source order. `kind` is `assume_trusted` for a trust assertion or `expose` for a secret exposure, and `span` covers the whole call.

A parse error is reported alone, with empty `functions` and `audit`.

### `test --format=json`

```json
{"tests":[{"name":"adds","status":"passed","failure":null}],"passed":1,"failed":0,"skipped":0,"diagnostics":[]}
```

`status` is `passed`, `failed`, or `skipped`. `failure` is a diagnostic object or `null`.

### `run --format=json`

The program's output is unchanged. Each diagnostic or panic is one JSON object per line on standard error.

## Diagnostic codes

The tables of checker codes (`E02xx`, `E03xx`) are in the [checker notes](../checker.md#diagnostic-codes), the runtime refusals and panics are in the [runtime notes](../runtime.md), and parser codes (`E01xx`) are in the [AST reference](../ast.md). For symptoms and their fixes, see [troubleshooting](troubleshooting.md).

## Coming in v0.1.0: `oskac explain`

> **Available from v0.1.0.** This command is part of the decided design and is not in the current build. When it ships, this section and the matching entry in `oskac --help` are updated together.

```sh planned
oskac explain E0300
```

`oskac explain <code>` prints the catalog entry for one diagnostic code: the same text as that code's page on the documentation site (`/codes/E0300`), but offline.

Until it exists, the code tables linked above are the reference.
