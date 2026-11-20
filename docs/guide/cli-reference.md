# CLI reference

`oskac` is the Oska compiler and runner. This page is checked against `oskac --help` when the documentation is tested: every command and flag in the help text appears here, and nothing else does.

```help
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check [path | -] [--format=json] [--release] [--no-cache]
                [--baseline=<file> | --update-baseline=<file> | --grant=<file>]
                                           Type-check a source file, a package
                                           directory (the current one with no
                                           path), or one module on standard
                                           input (-), and report each
                                           function's authority; compare it
                                           with a saved baseline or an
                                           authority grant (exit code 3 when
                                           it goes past), or write a baseline
    oskac run [path | -] [--format=json] [--interpret] [--no-cache]
              [--grant=<file>] [--max-steps=<n>] [--timeout=<ms>]
              [--max-output=<bytes>] [--max-memory=<bytes>]
              [-- args...]
                                           Check a source file, package or
                                           module on standard input (-) and,
                                           if it has no diagnostics and stays
                                           within the --grant, run it:
                                           natively on aarch64 and x86_64
                                           Linux, on the interpreter elsewhere
                                           or with --interpret; stop it with
                                           P0038 after the steps --max-steps
                                           gives, with P0039 after the
                                           milliseconds --timeout gives, or
                                           with P0041 before it writes more
                                           than the bytes --max-output gives
                                           to stdout and stderr together,
                                           and with P0040 once its values
                                           take more than --max-memory bytes
    oskac test [path] [--format=json] [--integration] [--interpret] [--no-cache]
                                           Run the test blocks in a source file
                                           or package, natively or interpreted
                                           as `run` does
    oskac build [path] [-o out] [--target <platform>|all] [--format=json] [--no-cache]
                                           Check a source file or package and,
                                           if it has no diagnostics, compile it
                                           to an executable for the platform
                                           (by default the host's when it has a
                                           backend, else linux-aarch64), or for
                                           every platform with a backend into
                                           the -o directory
    oskac release [path] --version <X.Y.Z> --key <ssh-key>
                  [-o dir] [--format=json] [--no-cache]
                                           Build a source file or package for
                                           every platform into one directory
                                           (by default <name>-v<X.Y.Z>), with
                                           a SHA256SUMS signed by the SSH key
    oskac fix [path] [--dry-run] [--placeholders] [--format=json]
                                           Apply the machine fixes to a source
                                           file or package, checking again
                                           after each, and write each changed
                                           file once
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac new <name> [--lib] [--no-agent-files] [--format=json]
                                           Lay out a new package in the
                                           directory <name>: oska.toml,
                                           src/main.osk (src/lib.osk with
                                           --lib), and the agent-setup
                                           AGENTS.md and CLAUDE.md
    oskac hook claude --event=post-edit [--format=json]
                                           Check what Claude Code just edited,
                                           reading its hook input on standard
                                           input; exit 2 hands diagnostics back
    oskac hook claude --event=stop [--format=json] [targets...]
                                           Run the release check and the tests
                                           before Claude Code may finish; exit
                                           2 blocks it with what failed
    oskac cache info [--format=json]       Report the build cache, which check,
                                           run, test and build reuse unchanged
                                           results from: its folder, entries,
                                           size, limit, hits and misses
    oskac cache clean [--format=json]      Empty the build cache
    oskac self info [--format=json]        Report this installation: version,
                                           platform, executable, Oska home and
                                           install receipt
    oskac self uninstall [--format=json]   Remove this managed install, as the
                                           installer's uninstall does
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
```

`lex` and `parse` take exactly one source file, and `explain` a code. `check`, `run`, `test`, `build`, `release` and `fix` take a source file, a package directory, or nothing, which means the package in the current directory. A package is a directory with an `oska.toml` (`[package]` with a `name` and a `version`) and its modules under `src/`; every `.osk` file there is a module, `main` is allowed only in `src/main.osk`, and a package without that file is a library, which `run`, `build` and `release` refuse. A directory without `oska.toml` is an error (`E0413`). A file inside a package is handled alone, with a hint to run the command on the package. A command line is parsed like this:

- A flag may come anywhere before `--`. Anything else is a positional argument.
- Everything after `--` belongs to the program being run, and only `oskac run` uses it.
- `--target <platform>`, or `--target=<platform>`, is for `oskac build` only: any other command given it prints the usage text to standard error and exits with 2.
- `--version <X.Y.Z>` and `--key <file>` (or `--version=<X.Y.Z>` and `--key=<file>`) are for `oskac release` only: any other command given `--key` or `--version=` prints the usage text to standard error and exits with 2. `--version` alone, with no command, prints `oskac`'s own version.
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
| `--grant=<file>` | `check`, `run` | Compare the program's authority with the [authority grant](#authority-grant) in `<file>`, and exit with 3 when it goes past it; `run` then runs nothing. Not with `--baseline` or `--update-baseline`; a repeated flag, an empty file name, or the flag on any other command is a usage error (exit code 2). |
| `--integration` | `test` | Also run the tests marked `#[integration]`, on the real operating system. |
| `--event=post-edit`, `--event=stop` | `hook` | The event the hook handles. It is required. |
| `--interpret` | `run`, `test` | Run on the interpreter, on Linux and macOS too, where the program is otherwise compiled and run natively. See [platform support](#platform-support). The other commands accept the flag and ignore it. |
| `--target=<platform>` | `build` | Build for this platform, named as release assets name `oskac` builds (`linux-aarch64`, ...), or with `all` for every platform with a backend. Also written `--target <platform>`. Any other command given it is a usage error (exit code 2). See [`oskac build`](#oskac-build). |
| `--version=<X.Y.Z>` | `release` | The release's version: `X.Y.Z`, or `X.Y.Z-<pre>` such as `1.2.0-rc.1`. Also written `--version <X.Y.Z>`. See [`oskac release`](#oskac-release). |
| `--key=<file>` | `release` | The SSH private key that signs the release's `SHA256SUMS`. Also written `--key <file>`. Any other command given it is a usage error (exit code 2). |
| `--dry-run` | `fix` | Print the diff the fixes would make, and write nothing. The other commands accept the flag and ignore it. |
| `--placeholders` | `fix` | Also apply fixes whose applicability is `placeholders`, such as `E0253`'s `todo()` arms. The other commands accept the flag and ignore it. |
| `--max-steps=<n>` | `run` | Stop the program with `P0038` (exit code 101) after `<n>` steps: calls of functions, methods and closures written in Oska, and loop back-edges ([ADR 0021](../adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)). Natively and on the interpreter alike, at the same step. The value is a whole number above 0; anything else, a repeated flag, or the flag on any other command is a usage error (exit code 2). See [run limits](../runtime.md#run-limits). |
| `--timeout=<ms>` | `run` | Stop the program with `P0039` (exit code 101) after `<ms>` milliseconds of wall-clock time, also while it waits for input, within a tolerance of 100 ms. Natively, `oskac` stops its own child process, only that one, and prints the report itself, with exit code 101, never `128 + n`. The value is a whole number above 0; anything else, a repeated flag, or the flag on any other command is a usage error (exit code 2). See [run limits](../runtime.md#run-limits). |
| `--max-output=<bytes>` | `run` | Stop the program with `P0041` (exit code 101) before it writes more than `<bytes>` bytes to standard output and standard error together. The value is a whole number above 0; anything else, a repeated flag, or the flag on any other command is a usage error (exit code 2). See [run limits](../runtime.md#run-limits). |
| `--max-memory=<bytes>` | `run` | Stop the program with `P0040` (exit code 101) before its values (texts, lists, structs, enum values, closures) take more than `<bytes>` bytes, net of those it let go of. The backends may stop at different points. The value is a whole number above 0; anything else, a repeated flag, or the flag on any other command is a usage error (exit code 2). See [run limits](../runtime.md#run-limits). |
| `--lib` | `new` | Lay out a library: `src/lib.osk` and no `main`. Any other command given it is a usage error (exit code 2). |
| `--no-agent-files` | `new` | Leave out `AGENTS.md` and `CLAUDE.md`. Any other command given it is a usage error (exit code 2). |
| `--no-cache` | `check`, `run`, `test`, `build`, `release` | Neither read nor write the [build cache](#oskac-cache). Any other command refuses it as a usage error (exit code 2). |

## `oskac lex`

`oskac lex <file.osk>` prints the tokens of a source file, one per line, as `start..end` byte offsets and the token. It is a debugging aid, and the exact text of a token line is not a stable interface. With `--format=json`, a lexical error is printed as a diagnostic object on standard output. Exit code 0 on success, 1 on a lexical error.

## `oskac parse`

`oskac parse <file.osk>` prints the syntax tree as indented text. With `--format=json` it prints the documented AST export, described in the [AST reference](../ast.md). A parse error is printed as one diagnostic object on standard output with `--format=json`, or as text on standard error. Exit code 0 on success, 1 on a parse error.

## `oskac check`

`oskac check [path | -] [--format=json] [--release] [--baseline=<file> | --update-baseline=<file> | --grant=<file>]` parses and type-checks one file, every module of a package, or one module read from [standard input](#source-from-standard-input), against the prelude and reports each function's authority. Exit code 0 when there are no diagnostics, otherwise 1; with `--baseline` or `--grant`, 3 when the program checks but its authority widened or goes past the grant.

With `--format=json` the check object ends with `verdict`, which says what the exit code says, so a caller branches on one value: `ok` (exit code 0), `diagnostics` (1) or `widened` (3, every way the [authority baseline](#authority-baseline) or [authority grant](#authority-grant) comparison fails). See [output formats](#output-formats).

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

An **authority baseline** is a saved record of a program's authority, meant to be committed with the program and reviewed in any change that edits it. `oskac check --update-baseline=authority.json` writes it, only when the program has no diagnostics, as `oskac fix` writes a file: to a new file beside it that then replaces it in one step, and never through a symlink at that path. It holds `format` (1), the `oskac` version that wrote it, and the report without places: each function's `name`, `authority` and `pure`; audit sites counted per function and kind; and attenuations counted per function, method, kinds and root. A function's `authority` here also has `cap fn` for a `cap fn` parameter and `generic T` for a parameter of a bounded type parameter `T`. Everything is sorted, with no spans or paths, LF line endings and a final newline, so the file is the same on every platform and changes only when authority does.

`oskac check --baseline=authority.json` compares the program's report with the baseline, which may also be a saved `oskac check --format=json` report of a program with no diagnostics. Functions are matched by name, so a rename is one function removed and one added. A built-in kind covers every kind it can be attenuated into: `Fs` covers `Dir`, `DirRead` and `FileRead`, among others. The gate fails, with exit code 3, when:

- a function's authority gained an entry that none of its old entries covers (*widened*);
- a function that was pure is not (*became impure*);
- a function or method has more audit sites of a kind than before (*new audit site*);
- an added function reaches a kind that no entry of any function in the baseline covers.

Narrowing, removals, attenuation changes, an added function within the program's existing authority, and anything in a test block are reported and never fail. There is no flag that lets a widening pass: approving one means updating the baseline in the same change, so the approval is a reviewed diff. With diagnostics, `check` exits with 1 and compares nothing, since a refused program's report is incomplete. An unreadable or malformed baseline, a `format` other than 1, a file with neither a `format` nor the `diagnostics` of a saved report, a saved report with diagnostics, or two functions of one name in either report is an `error:` and exit code 1. So is a baseline that `--update-baseline` cannot write. Then nothing was compared, so with `--format=json` no check object is printed: standard output is empty and there is no `verdict`.

In the human output a `baseline:` section follows the authority report, one line per change, those that fail the gate first, each marked with what it gained (`+`) or lost (`-`):

```text
baseline: authority.json
  widened fn main: +Net
  added fn notes: DirRead
```

With `--format=json`, the check object gains `authority_diff`, just before `verdict`, which stays last:

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

`verdict` is `widened` exactly when `fails` is `true`.

`baseline_oskac` is the version that wrote the baseline, or `null` for a saved report; when it differs from the running `oskac`, the human output says so, since a new release can change what the report records. `added` lists, in `new_kinds`, the entries no baseline function covers. Each `count` is how many sites were added or removed.

### Authority grant

An **authority grant** ([glossary](../../GLOSSARY.md)) is the most authority an embedder allows one piece of code, such as a snippet a model wrote, to have for one run. The embedder writes it; it is not committed with the code, and `oskac` never writes or updates it. `--grant=<file>` reads it on `check` and on `run`. A grant has the authority baseline's format (format 1), so an embedder can write one by hand or from a saved `oskac check --format=json` report, and it is compared by the [authority baseline](#authority-baseline)'s rules:

- A grant that lists only `main` limits the whole program: an added function that reaches a kind no grant entry covers goes past it, as an added function does against a baseline, so the embedder need not know the snippet's other function names. In a package, functions are named by module (`pkg.main.main`), as in its report.
- A new audit site that the grant does not list, such as an `assume_trusted` on text the code did not write, goes past it, although no function gains a capability.
- A program that goes past its grant exits with 3 and `verdict` `widened`. There is no `--update-grant` and no flag that lets a program pass its grant: only the embedder changes it.
- With diagnostics, nothing is compared and the exit code is 1. An unreadable or malformed grant is an `error:` with exit code 1, as for a baseline, and nothing runs.

The output is the baseline's, with `grant` in place of `baseline`: a `grant:` section in the human output, and `"grant": "<file>"` and `grant_oskac` in place of `baseline` and `baseline_oskac` in the JSON `authority_diff`.

`oskac run --grant=<file>` checks the program against the grant before it runs anything, on either backend, and for a file, a package or a module on [standard input](#source-from-standard-input) (`oskac run - --grant=grant.json`). The program is checked once, and that check decides both the comparison and what runs. A program past its grant is not run: the exit code is 3, and standard error has the human `grant:` section followed by the `error:` line, or with `--format=json` one JSON object on one line, `{"authority_diff":{...},"verdict":"widened","error":"..."}`, with the error's text in `error`.

## `oskac run`

`oskac run <file.osk | -> [--format=json] [--interpret] [--grant=<file>] [--max-steps=<n>] [--timeout=<ms>] [--max-output=<bytes>] [--max-memory=<bytes>] [-- args...]` checks the file, or the module read from [standard input](#source-from-standard-input), and, when it has no diagnostics and stays within its [authority grant](#authority-grant) if one is given, runs `main` on the real operating system: compiled natively on Linux and macOS, on the interpreter on Windows or with `--interpret` (see [platform support](#platform-support)). `args.get(0)` in the program is the file's path, the package directory as given, or `<stdin>` for a module read from standard input, on either backend, and the arguments after `--` follow it unchanged.

- The program's own output goes to standard output and standard error as it runs.
- A refused program's diagnostics, or the panic that stopped the program, go to standard error after that output: as `check` prints diagnostics, or as one JSON object per line with `--format=json`.
- The exit code is the program's, or 3 when the program goes past its `--grant` and was not run.
- `--max-steps=<n>` stops the program, with the panic `P0038` and exit code 101, at the step past `<n>`: a call of a function, method or closure written in Oska, or a loop going back for its next iteration. It stops at the same step natively and on the interpreter. See [run limits](../runtime.md#run-limits).
- `--timeout=<ms>` stops the program, with the panic `P0039` and exit code 101, once it has run for `<ms>` milliseconds of wall-clock time, waiting for input included, within a tolerance of 100 ms. A timeout is not deterministic: where the program stops differs from run to run. See [run limits](../runtime.md#run-limits).
- `--max-output=<bytes>` stops the program, with the panic `P0041` and exit code 101, at the write that would take what it wrote to standard output and standard error together past that many bytes; that write writes nothing. See [run limits](../runtime.md#run-limits).
- `--max-memory=<bytes>` stops the program, with the panic `P0040` and exit code 101, before its values take more than that many bytes, net of those it let go of. Each backend counts in its own way, so the two may stop at different points. See [run limits](../runtime.md#run-limits).

| Exit code | Meaning |
| --- | --- |
| `0` | `main` returned `Ok` or `()`, or the reader of the program's standard output went away (a closed pipe, as in `oskac run app.osk \| head -1`): the program ends quietly at the write that finds out. On standard error, or with any other failure, such as a full disk, the write is an `IoError`. `oskac`'s own output ends the same way. |
| `1` | The checker refused the program, or `main` returned `Err`. The runtime prints `e.to_string()` to standard error. |
| `2` | The program uses something the interpreter cannot run yet (`E0370`, `E0371`, `E0380`). Nothing ran. |
| `3` | The program's authority goes past its [authority grant](#authority-grant) (`--grant`). Nothing ran. |
| `101` | The program panicked. |
| `128 + n` | A native program was killed by signal `n`, as a shell reports it, such as `143` for `SIGTERM`. A signal only a fault raises, such as `SIGSEGV`, is also reported as `E0392`. A closed pipe is not a signal on either backend: see exit code `0`. |

See [debugging](debugging.md) for panics and the [runtime notes](../runtime.md) for every runtime code.

### Source from standard input

A bare `-` as the path makes `check` and `run` read one module from standard input to its end, as an embedder pipes a snippet: `oskac check - < snippet.osk`, `oskac run - -- a b < snippet.osk`. The result is what the same bytes in a file give, with `<stdin>` as the file's path in diagnostics, panics and the JSON `path`, so the output never names a temporary file.

- An empty standard input is an empty module. Standard input that is not UTF-8 is the error a file that is not UTF-8 gives: `error: cannot read <stdin>: stream did not contain valid UTF-8`, exit code 1.
- A program run with `run -` has `<stdin>` as `args.get(0)`, with the arguments after `--` following it. Its own standard input is closed: a read gets end of file at once, on either backend, since `oskac` has read the program from it.
- The [build cache](#oskac-cache) keys the module by its bytes, as it keys a file, so a second `check -` of the same snippet is a hit with the same output.
- `-` is a path only for `check` and `run`: `lex`, `parse`, `test`, `build` and `fix` refuse it as a usage error (exit code 2).

## `oskac test`

`oskac test <file.osk> [--format=json] [--integration] [--interpret]` checks the file and runs its `test` blocks in source order: natively on Linux and macOS, one process per test, or on the interpreter, as `run` chooses. The report is the same either way. The report goes to standard output: one line per test, the failures, and a summary, or one JSON object with `--format=json`. A refused file's diagnostics go to standard error, or into the JSON report's `diagnostics`. See [testing](testing.md).

| Exit code | Meaning |
| --- | --- |
| `0` | No test failed. Skipped tests do not count. |
| `1` | A test failed, or the checker refused the file. |
| `2` | The file uses something the interpreter cannot run yet. No test ran. |

## `oskac build`

`oskac build [path] [-o out] [--target <platform>|all] [--format=json]` checks a file or package as `check` does and, when it has no diagnostics, compiles it to a static executable for a target platform. Building works on every host and writes the same bytes on each. The executable is the file's stem, or the package directory's name, in the current directory; `-o out` names it instead.

- `--target` names the platform as release assets name `oskac` builds: `linux-aarch64`, `linux-x86_64`, `macos-aarch64`, `macos-x86_64` or `windows-x86_64`. Every platform has a backend; an unknown name is refused with `E0393`, which lists the names. A Windows executable's name ends with `.exe`. Without `--target`, the build is for the host's platform when it has a backend, else for `linux-aarch64`.
- `--target all` writes one executable for each platform with a backend, named `<name>-<platform>` (such as `hello-linux-aarch64`), into the directory `-o` names, or the current one.
- Success prints nothing, or `{"output":"<path>","diagnostics":[]}` with `--format=json` (`{"outputs":["<path>",...],"diagnostics":[]}` with `--target all`).
- A refused program's diagnostics go to standard error as `check` prints them, or to standard output as `{"output":null,"diagnostics":[...]}` with `--format=json`.
- The native backend compiles only part of the language so far. Anything else is refused with `E0390` before a file is written, never compiled wrongly; `oskac run` still runs the program, on the interpreter.
- The executable needs nothing else to run: no `oskac`, and on Linux no C library (a macOS executable uses only `libSystem`, which every Mac has). Run directly, it reports a panic as `oskac run` does (human-readable only), and `args.get(0)` is the path it was run as, such as `./hello`, where `oskac run` reports the source file's path.

| Exit code | Meaning |
| --- | --- |
| `0` | The executable was written. |
| `1` | The checker refused the program, or the executable could not be written (`E0391`). |
| `2` | The program uses something the native backend cannot compile yet (`E0371`, `E0390`), or `--target` names a platform it cannot build for (`E0393`, `E0394`). |

See the [runtime notes](../runtime.md#native-builds) for the codes.

## `oskac release`

`oskac release [path] --version <X.Y.Z> --key <ssh-key> [-o dir] [--format=json]` builds a file or package for every platform into one directory and signs the result, in the format `oskac`'s own releases use. The directory, `<name>-v<X.Y.Z>` in the current directory unless `-o` names it, must not exist yet. It holds:

- one executable per platform, named as `oskac build --target all` names them: `<name>-linux-aarch64`, `<name>-linux-x86_64`, `<name>-macos-aarch64`, `<name>-macos-x86_64` and `<name>-windows-x86_64.exe`;
- `SHA256SUMS`, whose first line names the release, `# <name>-release v<X.Y.Z>`, followed by one `sha256sum` line, `<sha256>  <file>`, per executable, sorted by file name;
- `SHA256SUMS.sig`, the signature `ssh-keygen -Y sign` writes with your key, in the namespace `<name>-release`.

The first line puts the name and the version under the signature, so a signed release cannot be passed off as another one. `oskac` builds and hashes the release itself and runs your `ssh-keygen` to sign it, so a key with a passphrase asks for it as `ssh-keygen` does. The name (the file's stem or the package directory's name) must be ASCII letters, digits, `.`, `-` and `_`, starting with a letter or digit. The version, the key and the name are checked before anything is built, and every platform is built before anything is written; the signature is checked with `ssh-keygen -Y check-novalidate` once written; if writing, signing or that check fails, the directory is removed, so a failed release never leaves an unsigned one behind. How your users check a release, and how to add Authenticode or Apple notarization, is in [Releasing an application](releasing.md).

- Success prints nothing, or with `--format=json` one object: `{"directory":"<dir>","name":"<name>","version":"<X.Y.Z>","files":[{"path":"<dir>/<file>","sha256":"<hex>"},...],"checksums":"<dir>/SHA256SUMS","signature":"<dir>/SHA256SUMS.sig","namespace":"<name>-release","diagnostics":[]}`.
- A refusal prints its diagnostics as `check` does, or `{"directory":null,"diagnostics":[...]}` with `--format=json`.

| Exit code | Meaning |
| --- | --- |
| `0` | The release was written and signed. |
| `1` | The checker refused the program, the directory could not be written or already exists (`E0391`), the key is missing, is not an SSH signing key, or could not sign (`E0396`), or the program's name cannot name a release (`E0397`). |
| `2` | The version is missing or is not `X.Y.Z` (`E0395`), or the program uses something the native backend cannot compile yet (`E0371`, `E0390`). |

## `oskac fix`

`oskac fix [path] [--dry-run] [--placeholders] [--format=json]` applies the fixes that are safe to apply automatically to a file or package, as `check` takes them. It checks the program, applies the fix of the first diagnostic, in the order `check` reports them, whose applicability is `machine`, checks again, and repeats until no such fix is left. With `--placeholders` it applies `placeholders` fixes too, such as `match` arms whose body is `todo()`, which compile but are left to fill in. It never applies a `maybe` fix or a suggestion. It stops early when a fix leaves its own diagnostic in place, which it then undoes, or after 100 rounds.

The fixes are made in memory. At the end, each changed file is written once, replacing the old file in one step. Nothing is written, and the command says which rule refused it in an `error:` line on standard error, when:

- a fix edits a file that is not one of the checked program's, the file itself or the package's modules and manifest, or its edits overlap or fall outside the file;
- a file is a symlink, or is reached through one: no file is read or written through a symlink;
- a file changed on disk while `oskac fix` ran;
- the fixed program's authority is wider than the starting program's, by the rules of the [authority baseline](#authority-baseline): a function gained a capability, a function stopped being pure, a new function reaches a capability no function did, or an audit site was added. The starting program is compared once every module parses, since a module that does not parse has no authority report. So a fix applied before every module parses is never checked for widening, by this rule or any other: only the fixes after that point, and the program they leave, are compared with the starting authority.

Two short windows remain in the write. A file is re-read to see whether it changed on disk, then replaced in a later step, so an edit made in between is lost. On Windows the file is looked at by path and then renamed, so a change between those two steps is not caught either. Neither window is closed; a file you edit while `oskac fix` runs may lose the edit.

Human-readable output: one line per applied fix on standard output, as ``app.osk:2:24: fixed E0005: remove `;` ``, then with `--dry-run` a unified diff of each file, and the diagnostics that remain on standard error as `check` prints them.

| Exit code | Meaning |
| --- | --- |
| `0` | No diagnostic remains. |
| `1` | Diagnostics remain, a file cannot be read, or writing was refused. |
| `2` | Usage error. |

## `oskac hook`

`oskac hook claude --event=post-edit` is the check-on-edit hook for Claude Code, so that the hook runs the same on Linux, macOS and Windows with no shell script. Claude Code's settings call it after an edit, and it reads the hook's JSON from standard input: `tool_input.file_path`, and `cwd` to resolve a relative path.

- For a `.osk` file or an `oska.toml`, it checks what `oskac` finds for the file as `check` does, without `--release`: a module of a package is checked with its package, and any other file alone. Any other file is ignored, with no check. A file is never read through a symlink: a `.osk` file that is one is refused with `E0411`, as a symlink in a package is.
- With no diagnostic it prints nothing. With diagnostics it writes to standard error one line each, `path:line:column: error[CODE]: message`, at most 20, then `and N more`, then, when some have a `machine` fix, how many and the `oskac fix` command that applies them. Paths are written from `cwd`.
- With `--format=json` it writes one JSON object on one line instead: `event` (`"post-edit"`), `target`, `diagnostics` (at most 20, each in the shape `check --format=json` gives it, with `line`, `column`, `severity`, `url` and `fix`), `more` (how many were left out), `machine_fixes`, and `fix_command` (the `oskac fix` command, or `null` when no diagnostic has a `machine` fix).
- It reads its input and the program's files, writes no file, starts no program and opens no network connection.

`oskac hook claude --event=stop [targets...]` is the Stop gate: Claude Code's settings call it when the agent tries to finish, and the agent cannot finish while the release check or a test fails. It reads `cwd` from the hook's JSON.

- The targets are the ones named, each a path from `cwd`, such as a package directory or a `.osk` file. With none, it takes the package in `cwd` if that is one; otherwise every package, and every `.osk` file outside a package's `src/`, beneath `cwd`, skipping hidden directories and following no symlink. Name the targets in the hook's settings when a project holds `.osk` files that are broken on purpose.
- For each target it runs `oskac check --release` and, when that is clean, `oskac test` without `--integration`, on the interpreter and in-process: a test gets only stand-in capabilities, so the gate never touches the real file system, clock or network.
- With any diagnostic or failed test it writes to standard error one line each (`path:line:column: error[CODE]: message`, and `test <name>: path:line:column: error[CODE]: message` for a test), at most 20 of each per target, then `and N more`, and exits with 2. It blocks on every attempt to finish.
- A target it cannot check blocks too: a named target that is not there, a directory beneath `cwd` it cannot read, or a `.osk` file that is a symlink (`E0411`). Each is an `error:` line, every other target is still checked, and the gate exits with 2. It never lets the agent finish on what it could not check.
- With `--format=json` it writes one JSON object on one line instead: `event` (`"stop"`), `targets` (each target that blocks, with `target`, `diagnostics` in the shape `check --format=json` gives them, `more`, `failed_tests`, each a `name` and its `failure` as a diagnostic, and `more_failed_tests`), and `errors` (what could not be read).
- A test run that the interpreter refuses (`oskac test` exits with 2: the program uses something the interpreter cannot run yet) does not block. The gate exits with 0 and tells the user, in a `systemMessage` on standard output, and the agent is told nothing.
- The settings give it a `timeout` in seconds, so that a slow suite cannot stall every turn.

The published settings, `https://oskalang.dev/agent-setup/claude-settings.json`, run each command as `oskac` by name with arguments only: no script and no shell syntax, so the same line works in whichever shell Claude Code uses, on Linux, macOS and Windows. `oskac` must be on `PATH`. The old `.claude/hooks/oska-check.sh` fell back to `$OSKA_HOME/bin/oskac`; the command has no such fallback, so add the installer's `bin_dir` to `PATH` before you enable a hook. A project that saved the script earlier can keep it: the published copy is now one line that calls `oskac hook claude --event=post-edit`.

| Exit code | Meaning |
| --- | --- |
| `0` | Nothing to report. For the Stop gate, also a test run the interpreter cannot run yet, told to the user only. |
| `2` | Diagnostics, handed back to the agent: Claude Code shows standard error to it. For the Stop gate, it blocks the agent from finishing, as does a target it cannot read. |
| `1` | Malformed input, an unknown event, or, after an edit, a file that cannot be read: one `error:` line. Claude Code treats `1` as non-blocking. |

## Platform support

| Host | `oskac run`, `oskac test` | `oskac build` |
| --- | --- | --- |
| aarch64 Linux | Native: compiled, then run | aarch64 Linux executable |
| x86_64 Linux | Native: compiled, then run | x86_64 Linux executable |
| Apple silicon macOS | Native: compiled, then run | aarch64 macOS executable |
| Intel macOS | Native: compiled, then run | x86_64 macOS executable |
| Any other (Windows) | Interpreter | aarch64 Linux executable |

On Linux and macOS, `run` and `test` compile the program as `build` does, for the host, and run the executable, which is 40 to 120 times faster on program logic, such as loops, lists and calls (see the [benchmark](../design.md#native-backend-benchmark)). `--interpret` runs the interpreter instead, to compare the backends when a native result looks wrong. Windows keeps the interpreter until it has a backend of its own. Either way the output, the reports and the exit codes are the same: the two backends are held to it by the differential suite.

- The executable is written to a new private directory under the temporary directory (`TMPDIR`, else `/tmp`): a random name, created with mode `0700`, never reused, so no other user can replace or read it. It is removed as soon as the program has started, or for `test` once every test has run.
- A program the native backend cannot compile yet (`E0390`) runs on the interpreter, after one line on stderr that says so and why, such as `app.osk:3:13: note: running on the interpreter: ... (E0390)` (with `--format=json`, one JSON object with `"severity":"note"`). It is a checked program that the interpreter runs, so the output is the same, only slower. `--interpret` prints no note. A program the interpreter cannot run either, such as one that imports a module with no runtime (`E0371`), gets no note: both refuse it, and the interpreter's refusal is reported.
- If the executable cannot be written or started, for example because the temporary directory is mounted `noexec`, the command fails with `E0391`. Set `TMPDIR` to another directory or use `--interpret`.
- A macOS executable is ad-hoc signed by `oskac`: it runs on a Mac, but Gatekeeper refuses it after a download. To distribute one, sign it with your Developer ID and notarize it: see [Signing and notarizing for macOS](macos-signing.md).

## `oskac new`

`oskac new <name>` lays out a new package in the directory `<name>`, under the current directory, so that nobody writes the layout from memory:

- `oska.toml`, with `name = "<name>"` and `version = "0.1.0"`;
- `src/main.osk`, a `main` that prints a greeting, with two tests; or, with `--lib`, `src/lib.osk`, a `pub` function and its test, and no `main`;
- `AGENTS.md` and `CLAUDE.md`, the [agent-setup](agent-guide.md#files-for-agents) templates the site publishes, byte for byte. They are built into `oskac`, so `oskac new` works offline. `--no-agent-files` leaves both out.

The package passes `oskac check --release` and `oskac test` as written, and a program's `main` uses the one capability it takes, `Stdout`, so it starts with no unused authority. In the package, `oskac run` prints `Hello, world!`.

`<name>` must be a valid package name, as the manifest's `name` is: snake_case, lowercase letters and digits in words joined by single underscores, starting with a letter. The directory may not exist yet, or be empty; anything else is refused. A refusal writes nothing and exits with 1, printing `oskac new: <reason>` to standard error, or `{"ok":false,"error":"<code>","message":"..."}` with `--format=json`, where the code is `invalid_name`, `not_empty` (a directory with something in it), `exists` (a file, or a symlink, in the way) or `io` (the file system refused). Each file is created new, never written over one that is there.

On success it prints the files it wrote and exits with 0. With `--format=json` it prints `{"ok":true,"directory":"demo","files":["oska.toml","src/main.osk","AGENTS.md","CLAUDE.md"]}`, the files in the order they were written, relative to the directory. No name, or more than one, is a usage error (exit code 2).

## `oskac self`

`oskac self info [--format=json]` and `oskac self uninstall [--format=json]` ask `oskac` about its own installation and remove it, without the installer. They read the [install receipt](install.md#the-install-receipt) in the Oska home (`~/.oska`, or `OSKA_HOME`), and `oskac` makes no network connection to do it. Any other `self` command or flag is a usage error (exit code 2); with `--format=json` it prints `{"ok":false,"error":"usage","message":"..."}`.

`self info` prints the version, the platform, the canonical path of the running executable, the Oska home and the receipt. With `--format=json` it prints one object: `version`, `platform`, `executable`, `oska_home` (`null` with no home), `receipt` (the receipt as parsed, with every field it has, or `null` when there is none or it cannot be read), `receipt_error` (why it cannot be read, else `null`), `receipt_matches` and `digest_checked`. `receipt_matches` is `true` when the receipt's `version` is this `oskac`'s and it lists this executable as a `binary`, by path. `digest_checked` is always `false`: `oskac` does not check the receipt's SHA-256 digests, so it identifies its install by path and version only. It exits with 0.

`self uninstall` applies the rules of the installer's [`--uninstall`](install.md#uninstall) in this process. [The install receipt](install-receipt.md) defines them, for every kind of entry and for each of the three uninstallers. It checks every entry of the receipt before it removes anything, against the places this platform's installer writes each kind, and refuses the whole uninstall if one is elsewhere, or of a kind that installer never writes. Then:

- `binary` and `record` entries (files directly in the Oska home or its `bin` directory) are removed only when the receipt matches this `oskac`, as `self info` reports; otherwise they are skipped and listed in `kept`, and the rest of the uninstall goes ahead. As `oskac` checks no digest, it removes only what it can show unchanged otherwise: the running `oskac` itself, the `version` file while it holds the receipt's version, and the receipt. Any other `binary` or `record` is kept, for the installer's `--uninstall` to remove.
- `hook_script` and `hook_settings` entries (what `install.sh --hook` wrote into a project: the post-edit settings, or the old shell hook from an earlier installer) are removed only if they are byte for byte the published hook files this `oskac` carries; a changed file, or one from another release, is kept. Settings the installer did not create are kept, with a hint to remove the hook from them by hand.
- `profile_line` and `user_path_entry` entries (what `--add-to-path` did) are left to the installer's `--uninstall`: `path_hint` says what to remove, or to run it.
- On Windows the running `oskac.exe` cannot be deleted, so it is moved into the temporary directory and listed under `pending`, with the path it was moved to: delete that file once `oskac` has exited.

Then it removes `bin` and the Oska home if they are left empty, and it never touches `~/.config/oska`. With `--format=json` it prints `{"ok":true,"action":"uninstalled","removed":[...],"kept":[...],"pending":[...],"path_hint":null}`, where each `pending` entry is `{"path":"...","moved_to":"...","note":"..."}`. A refusal removes nothing and exits with 1, printing `{"ok":false,"error":"<code>","message":"..."}` with `--format=json`: `not_installed` (no receipt, or no Oska home) or `unsafe_path` (an entry outside the places the installer writes its kind, of a kind this platform's installer does not write, in a `.config/oska` directory, with `..`, or through a symlink, or a receipt it cannot read).

## `oskac cache`

`oskac check`, `run`, `test` and `build` keep their results in a build cache, and reuse a result whose inputs are unchanged instead of checking or compiling again. A reused result is the same as a fresh one: the same output, byte for byte, the same diagnostics and authority report, with `--format=json` too, and the same exit code. The cache is on by default and prints nothing of its own.

- **What is kept.** For `check`, what it printed and its exit code. For `run`, `test` and `build`, the executable the native backend built for each target; the check before it still runs. A refusal (`E0390`), a check with `--baseline`, `--update-baseline` or `--grant`, and a program run on the interpreter are not kept.
- **What makes a miss.** Any change to a module, the manifest, the set of modules, the path the program was named by, the target, an output flag (`--format`, `--release`), or the `oskac` executable itself: the key includes the SHA-256 of the running `oskac`, so a development build never reuses another build's results.
- **Where.** One cache per user, shared across projects and worktrees: `OSKA_CACHE_DIR` if set, else `$XDG_CACHE_HOME/oska` or `~/.cache/oska` on Linux, `~/Library/Caches/oska` on macOS, and `%LOCALAPPDATA%\oska\cache` on Windows.
- **Integrity.** Each entry is written to a temporary file and renamed into place, so concurrent runs never see half an entry, and it carries the SHA-256 of its contents, checked on every read: a damaged entry is a miss, and is rebuilt. Symbolic links in the cache are never followed. On Linux and macOS a cache folder that is a symbolic link, is owned by another user, or is writable by its group or others is refused: `oskac` prints `warning: running without the build cache: <reason>` to standard error and carries on without it.
- **Size.** At most 1 GiB, or `OSKA_CACHE_LIMIT` bytes; the least recently used entries are removed after each write.
- **Off.** `--no-cache` on one command, or `OSKA_CACHE=off` for all of them.

`oskac cache info [--format=json]` prints the cache folder, whether it is used (and why not), its entries, their size, the limit, and the hits and misses counted since it was last cleaned. With `--format=json` it prints `{"location":"...","used":true,"reason":null,"entries":3,"size":49709,"limit":1073741824,"hits":2,"misses":2}`. It makes nothing and exits with 0. `oskac cache clean [--format=json]` removes every entry and the counts, and prints `removed <n> entries (<bytes> bytes) from <folder>`, or `{"location":"...","removed":3,"bytes":49709}`; it leaves a refused folder alone and exits with 1. Any other `cache` command is a usage error (exit code 2).

## `oskac --version` and `oskac --help`

`oskac --version` prints `oskac` and the version on one line, such as `oskac 0.5.0`, and `oskac --help` prints the text at the top of this page. Both also answer to the short forms `-V` and `-h`. With `--format=json`, `--version` prints a JSON object instead: `{"name":"oskac","version":"0.5.0"}`.

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
  ],
  "verdict": "ok"
}
```

- `diagnostics`, in source order.
- `functions`: one entry per function and method, in source order. `name` is the function's name, `Type.method` for a method of an inherent `impl`, or `<Type as Trait>.method` for one of `impl Trait for Type`, with a generic trait's arguments, as in `<AppError as From<IoError>>.from`, and a generic type's arguments, as in `<Pair<Int> as Show>.show`; in a package `pkg.` and the module path come first, as in `pkg.util.text.shout` or `pkg.main.main`, and a method is named by its type's module. No two entries share a name. `"pub":true` marks a function declared `pub`. `authority` lists the built-in capabilities it can reach, in order of first appearance. `via` lists each capability-bearing parameter and what it supplies, with a method's receiver as `self`; a parameter whose type is or holds a bounded type parameter is listed too, naming it in `generics`. `pure` is `true` when `via` is empty.
- `audit`: every audit site, in source order. `function` is the function it is in, named as for an attenuation, `kind` is `assume_trusted` for a trust assertion or `expose` for a secret exposure, and `span` covers the whole call.
- `attenuations`: every call that narrows a capability, in source order. `function` is the function it is in (`test "<name>"` for a test block, with the name written as an Oska string, or `test at line <n>` when the name is empty), `method` is `restrict_to`, `restrict_to_file`, `read_only` or `write_only`, `source` and `result` are the capability kinds before and after, and `root` is the string the capability is restricted to, or `null` when the method takes no root or the root is computed at runtime. `span` covers the whole call.
- `verdict`: always the last key. `ok` with exit code 0, `diagnostics` with exit code 1, or `widened` with exit code 3 when the program checks but a comparison of its authority with `--baseline` or `--grant` fails (a function widened or became impure, a new audit site, or a new kind). With `--baseline` or `--grant`, `authority_diff` comes just before it. An `error:` that stops the check, such as an unreadable file, baseline or grant, prints no check object.

A parse error is reported alone, with empty `functions`, `audit` and `attenuations`.

### `test --format=json`

```json
{"tests":[{"name":"adds","status":"passed","failure":null}],"passed":1,"failed":0,"skipped":0,"diagnostics":[]}
```

`status` is `passed`, `failed`, or `skipped`. `failure` is a diagnostic object or `null`.

### `run --format=json`

The program's output is unchanged. Each diagnostic or panic is one JSON object per line on standard error. A program refused for going past its `--grant` prints its diff and its error the same way, as one `{"authority_diff":{...},"verdict":"widened","error":"..."}` line (see [authority grant](#authority-grant)).

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
