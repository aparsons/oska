# CLI reference

`oskac` is the Oska compiler and runner. This page is checked against `oskac --help` when the documentation is tested: every command and flag in the help text appears here, and nothing else does.

```help
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check [path] [--format=json] [--release]
                [--baseline=<file> | --update-baseline=<file>]
                                           Type-check a source file, or a
                                           package directory (the current one
                                           with no path), and report each
                                           function's authority; compare it
                                           with a saved baseline (exit code 3
                                           when it widened), or write one
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
    oskac fix [path] [--dry-run] [--placeholders] [--format=json]
                                           Apply the machine fixes to a source
                                           file or package, checking again
                                           after each, and write each changed
                                           file once
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
```

`lex` and `parse` take exactly one source file, and `explain` a code. `check`, `run`, `test`, `build` and `fix` take a source file, a package directory, or nothing, which means the package in the current directory. A package is a directory with an `oska.toml` (`[package]` with a `name` and a `version`) and its modules under `src/`; every `.osk` file there is a module, `main` is allowed only in `src/main.osk`, and a package without that file is a library, which `run` and `build` refuse. A directory without `oska.toml` is an error (`E0413`). A file inside a package is handled alone, with a hint to run the command on the package. A command line is parsed like this:

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
| `--baseline=<file>` | `check` | Compare the program's authority with the [authority baseline](#authority-baseline) in `<file>`, and exit with 3 when it widened. Any other command refuses it as a usage error. |
| `--update-baseline=<file>` | `check` | Write the program's [authority baseline](#authority-baseline) to `<file>`, only when there are no diagnostics. Not with `--baseline`; any other command refuses it as a usage error. |
| `--integration` | `test` | Also run the tests marked `#[integration]`, on the real operating system. |
| `--interpret` | `run`, `test` | Run on the interpreter, on aarch64 Linux too, where the program is otherwise compiled and run natively. See [platform support](#platform-support). The other commands accept the flag and ignore it. |
| `--dry-run` | `fix` | Print the diff the fixes would make, and write nothing. The other commands accept the flag and ignore it. |
| `--placeholders` | `fix` | Also apply fixes whose applicability is `placeholders`, such as `E0253`'s `todo()` arms. The other commands accept the flag and ignore it. |

## `oskac lex`

`oskac lex <file.osk>` prints the tokens of a source file, one per line, as `start..end` byte offsets and the token. It is a debugging aid, and the exact text of a token line is not a stable interface. With `--format=json`, a lexical error is printed as a diagnostic object on standard output. Exit code 0 on success, 1 on a lexical error.

## `oskac parse`

`oskac parse <file.osk>` prints the syntax tree as indented text. With `--format=json` it prints the documented AST export, described in the [AST reference](../ast.md). A parse error is printed as one diagnostic object on standard output with `--format=json`, or as text on standard error. Exit code 0 on success, 1 on a parse error.

## `oskac check`

`oskac check [path] [--format=json] [--release] [--baseline=<file> | --update-baseline=<file>]` parses and type-checks one file, or every module of a package, against the prelude and reports each function's authority. Exit code 0 when there are no diagnostics, otherwise 1; with `--baseline`, 3 when the program checks but its authority widened.

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

The line `fn vouch: Stdout (io)` names each capability and the parameter that supplies it. A `cap fn` parameter appears as `cap fn (name)`, and a parameter of a bounded type parameter `T` as `generic T (name)`. A parse error is reported alone, with no authority lines. Terms are in the [glossary](../../GLOSSARY.md). The checker's behavior and its full code table are in the [checker notes](../checker.md).

### Authority baseline

An **authority baseline** is a saved record of a program's authority, meant to be committed and reviewed like a lockfile. `oskac check --update-baseline=authority.json` writes it, only when the program has no diagnostics. It holds `format` (1), the `oskac` version that wrote it, and the report without places: each function's `name`, `authority` and `pure`; audit sites counted per function and kind; and attenuations counted per function, method, kinds and root. A function's `authority` here also has `cap fn` for a `cap fn` parameter and `generic T` for a parameter of a bounded type parameter `T`. Everything is sorted, with no spans or paths, LF line endings and a final newline, so the file is the same on every platform and changes only when authority does.

`oskac check --baseline=authority.json` compares the program's report with the baseline, which may also be a saved `oskac check --format=json` report of a program with no diagnostics. Functions are matched by name, so a rename is one function removed and one added. A built-in kind covers every kind it can be attenuated into: `Fs` covers `Dir`, `DirRead` and `FileRead`, among others. The gate fails, with exit code 3, when:

- a function's authority gained an entry that none of its old entries covers (*widened*);
- a function that was pure is not (*became impure*);
- a function or method has more audit sites of a kind than before (*new audit site*);
- an added function reaches a kind that no entry of any function in the baseline covers.

Narrowing, removals, attenuation changes, an added function within the program's existing authority, and anything in a test block are reported and never fail. There is no flag that lets a widening pass: approving one means updating the baseline in the same change, so the approval is a reviewed diff. With diagnostics, `check` exits with 1 and compares nothing, since a refused program's report is incomplete. An unreadable or malformed baseline, a `format` other than 1, a saved report with diagnostics, or two functions of one name in either report is an `error:` and exit code 1.

In the human output a `baseline:` section follows the authority report, one line per change, those that fail the gate first, each marked with what it gained (`+`) or lost (`-`):

```text
baseline: authority.json
  widened fn main: +Net
  added fn notes: DirRead
```

With `--format=json`, the check object gains `authority_diff` as its last key:

```json
"authority_diff": {
  "baseline": "authority.json",
  "baseline_oskac": "0.3.0",
  "widened": [{"function":"main","gained":["Net"]}],
  "narrowed": [{"function":"notes","gained":["DirRead"],"lost":["Fs"]}],
  "became_impure": [{"function":"total"}],
  "added": [{"function":"fetch","authority":["NetConnect"],"pure":false,"new_kinds":["NetConnect"]}],
  "removed": [{"function":"load","authority":["DirRead"],"pure":false}],
  "new_audit_sites": [{"function":"shout","kind":"assume_trusted","count":1}],
  "removed_audit_sites": [],
  "new_attenuations": [{"function":"main","method":"read_only","source":"Dir","result":"DirRead","root":null,"count":1}],
  "removed_attenuations": [],
  "fails": true
}
```

`baseline_oskac` is the version that wrote the baseline, or `null` for a saved report; when it differs from the running `oskac`, the human output says so, since a new release can change what the report records. `added` lists, in `new_kinds`, the entries no baseline function covers. Each `count` is how many sites were added or removed.

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

## `oskac fix`

`oskac fix [path] [--dry-run] [--placeholders] [--format=json]` applies the fixes that are safe to apply automatically to a file or package, as `check` takes them. It checks the program, applies the fix of the first diagnostic, in the order `check` reports them, whose applicability is `machine`, checks again, and repeats until no such fix is left. With `--placeholders` it applies `placeholders` fixes too, such as `match` arms whose body is `todo()`, which compile but are left to fill in. It never applies a `maybe` fix or a suggestion. It stops early when a fix leaves its own diagnostic in place, which it then undoes, or after 100 rounds.

The fixes are made in memory. At the end, each changed file is written once, replacing the old file in one step. Nothing is written, and the command says which rule refused it in an `error:` line on standard error, when:

- a fix edits a file that is not one of the checked program's, the file itself or the package's modules and manifest, or its edits overlap or fall outside the file;
- a file is a symlink, or is reached through one: no file is read or written through a symlink;
- a file changed on disk while `oskac fix` ran;
- the fixed program's authority is wider than the starting program's, by the rules of the [authority baseline](#authority-baseline): a function gained a capability, a function stopped being pure, a new function reaches a capability no function did, or an audit site was added. The starting program is compared once every module parses, since a module that does not parse has no authority report.

Human-readable output: one line per applied fix on standard output, as ``app.osk:2:24: fixed E0005: remove `;` ``, then with `--dry-run` a unified diff of each file, and the diagnostics that remain on standard error as `check` prints them.

| Exit code | Meaning |
| --- | --- |
| `0` | No diagnostic remains. |
| `1` | Diagnostics remain, a file cannot be read, or writing was refused. |
| `2` | Usage error. |

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
    {"function":"vouch","kind":"assume_trusted","span":{"start":126,"end":147}}
  ],
  "attenuations": [
    {"function":"main","method":"restrict_to","span":{"start":88,"end":112},"source":"FsRead","result":"DirRead","root":"docs"}
  ]
}
```

- `diagnostics`, in source order.
- `functions`: one entry per function and method, in source order. `name` is the function's name, `Type.method` for a method of an inherent `impl`, or `<Type as Trait>.method` for one of `impl Trait for Type`, with a generic trait's arguments, as in `<AppError as From<IoError>>.from`; in a package `pkg.` and the module path come first, as in `pkg.util.text.shout` or `pkg.main.main`, and a method is named by its type's module. No two entries share a name. `"pub":true` marks a function declared `pub`. `authority` lists the built-in capabilities it can reach, in order of first appearance. `via` lists each capability-bearing parameter and what it supplies, with a method's receiver as `self`; a parameter whose type is or holds a bounded type parameter is listed too, naming it in `generics`. `pure` is `true` when `via` is empty.
- `audit`: every audit site, in source order. `function` is the function it is in (`test "<name>"` for a test block), `kind` is `assume_trusted` for a trust assertion or `expose` for a secret exposure, and `span` covers the whole call.
- `attenuations`: every call that narrows a capability, in source order. `function` is the function it is in (`test "<name>"` for a test block), `method` is `restrict_to`, `restrict_to_file`, `read_only` or `write_only`, `source` and `result` are the capability kinds before and after, and `root` is the string the capability is restricted to, or `null` when the method takes no root or the root is computed at runtime. `span` covers the whole call.

A parse error is reported alone, with empty `functions`, `audit` and `attenuations`.

### `test --format=json`

```json
{"tests":[{"name":"adds","status":"passed","failure":null}],"passed":1,"failed":0,"skipped":0,"diagnostics":[]}
```

`status` is `passed`, `failed`, or `skipped`. `failure` is a diagnostic object or `null`.

### `run --format=json`

The program's output is unchanged. Each diagnostic or panic is one JSON object per line on standard error.

### `fix --format=json`

One object on standard output:

```json
{
  "applied": [
    {"code":"E0005","message":"remove `;`","path":"src/main.osk","applicability":"machine"}
  ],
  "files": ["src/main.osk"],
  "diagnostics": [],
  "rounds": 1,
  "written": true
}
```

- `applied`: the fixes applied, in order, each with its diagnostic's `code`, the fix's `message`, the `path` of the file it edits, and its `applicability`. Empty when writing was refused.
- `files`: the files written, or that `--dry-run` would write, with paths as `diagnostics` gives them.
- `diagnostics`: what remains, in the full diagnostic shape: in the fixed program, or in the program on disk when writing was refused.
- `rounds`: how many fixes were applied and checked, a fix that was undone included.
- `written`: `true` when the files were written; `false` with `--dry-run`, or when nothing changed or writing was refused.
- `diff`, only with `--dry-run`: one `{"path","diff"}` object per file in `files`, with its unified diff.

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
