# Agent guide

How an AI coding agent, or any script, should drive `oskac`. Oska is built for this loop: every message has a stable code and a precise span, many carry a fix a tool can apply, and the authority report says what a change lets the program do.

## The loop

1. **Check.** `oskac check file.osk --format=json`. Read `diagnostics`.
2. **Apply machine fixes.** Apply the `fix` of a diagnostic that has one, then check again. Repeat until no diagnostic has a fix.
3. **Fix the rest by hand,** choosing by `code`, one diagnostic at a time, first one first.
4. **Run.** `oskac run file.osk --format=json -- args...`. The exit code and standard error say how it ended. On aarch64 Linux the program runs natively, elsewhere on the interpreter, with the same output; if a native result looks wrong, run it again with `--interpret` and report any difference.
5. **Test.** `oskac test file.osk --format=json`. Write a test for every behavior you add.
6. **Read the authority report.** Compare `functions` and `audit` from `oskac check --format=json` with what you intended. A change that widens authority, or adds an audit site, is not finished until you can say why.
7. **Release check.** `oskac check file.osk --release` must pass: no `dbg(...)` and no `todo()` left.

You are done when the check, the tests, and the release check all exit with 0, and the authority report is no wider than the task needs.

## Machine-readable outputs

| Command | Output with `--format=json` | Where |
| --- | --- | --- |
| `oskac check` | One object: `diagnostics`, `functions`, `audit` | standard output |
| `oskac test` | One object: `tests`, `passed`, `failed`, `skipped`, `diagnostics` | standard output |
| `oskac run` | The program's own output, unchanged. Diagnostics and panics are one JSON object per line. | standard error |
| `oskac build` | One object: `output` (the executable's path, or `null`), `diagnostics` | standard output |
| `oskac parse` | The AST, documented in the [AST reference](../ast.md) | standard output |
| `oskac lex` | Tokens (the same text as without the flag). A lexical error is a diagnostic object. | standard output |
| `oskac --version` | One object: `name`, `version` | standard output |

Every diagnostic has the same shape: `code`, `message`, `span`, and sometimes `fix`. Match on `code`. The shapes and exit codes are in the [CLI reference](cli-reference.md#output-formats). If `oskac` cannot read the file it prints `error: cannot read ...` to standard error, exits with 1, and prints no JSON, so check for empty standard output before parsing.

## Applying fixes

A `fix` is a message and a list of `edits`. Each edit replaces the bytes `span.start` up to `span.end` with `replacement`. The offsets count UTF-8 bytes in the file as it was checked.

```python
import json, subprocess

def check(path):
    out = subprocess.run(["oskac", "check", path, "--format=json"], capture_output=True)
    return json.loads(out.stdout)

def apply_fix(path, fix):
    src = open(path, "rb").read()
    for edit in sorted(fix["edits"], key=lambda e: e["span"]["start"], reverse=True):
        start, end = edit["span"]["start"], edit["span"]["end"]
        src = src[:start] + edit["replacement"].encode() + src[end:]
    open(path, "wb").write(src)

def apply_machine_fixes(path):
    while True:
        report = check(path)
        fixable = [d for d in report["diagnostics"] if "fix" in d]
        if not fixable:
            return report
        apply_fix(path, fixable[0]["fix"])  # one fix, then re-check
```

Rules that keep this safe:

- Apply one fix, then check again. Another diagnostic's offsets are stale after any edit.
- Within one fix, apply edits from the end of the file to the start.
- Work on bytes, not characters.
- A missing `fix` means the repair needs judgment. Do not guess one from the message.
- Read what a fix did. The fix for a non-exhaustive `match` (`E0253`) adds arms whose body is `todo()`, which compiles but panics with `P0003` when reached, and fails `--release`. Fill them in.

An example. The file declares a struct that holds a capability without saying so:

```oska error=E0210
struct Logger { out: Stdout }
```

The diagnostic has the fix `{"message":"declare it `cap struct`","edits":[{"span":{"start":0,"end":0},"replacement":"cap "}]}`. Applying it, an insertion at byte 0, gives a program that checks:

```oska
cap struct Logger { out: Stdout }
```

## Reading the authority report

The report is the review. For each function it says what the function can reach, and it lists every audit site:

```oska
fn word_count(docs: DirRead, path: Path) -> Result<Int, IoError> {
    let text = docs.read_to_string(path)?
    Ok(text.split(" ").len())
}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let docs = fs.restrict_to("notes").read_only()
    let words = word_count(docs, Path.new("today.txt"))?
    io.println("{words}")?
    Ok(())
}
```

```console cmd=check json
{
  "diagnostics": [],
  "functions": [
    {"name":"word_count","authority":["DirRead"],"via":[{"param":"docs","capabilities":["DirRead"]}],"pure":false},
    {"name":"main","authority":["Stdout","Fs"],"via":[{"param":"io","capabilities":["Stdout"]},{"param":"fs","capabilities":["Fs"]}],"pure":false}
  ],
  "audit": [],
  "attenuations": [
    {"function":"main","method":"restrict_to","path":"main.osk","span":{"start":210,"end":233},"source":"Fs","result":"Dir","root":"notes"},
    {"function":"main","method":"read_only","path":"main.osk","span":{"start":210,"end":245},"source":"Dir","result":"DirRead","root":null}
  ]
}
```

Before you finish a task, compare this against the report from before your change:

- **A function's `authority` grew,** or a new capability appears in `main`'s signature. Is that the task, or a shortcut? Prefer passing a narrower capability, such as `fs.restrict_to("dir").read_only()`, to adding a wider one.
- **A new `audit` entry appeared.** Every `assume_trusted` and `expose` weakens a guarantee. Use one only inside a validator that really checks the text, and say so in your summary.
- **A new `attenuations` entry has a wider root than the task needs.** Each one names the function it is in, the capability it narrows from and to, and its `root`. A `root` of `"/"` is the whole filesystem, and `null` means the root is computed at runtime (or, for `read_only` and `write_only`, that there is none).
- **A function that was `pure` is not.** It now has a capability-bearing parameter, or one of a bounded type parameter (`generic T`) that it can perform effects through.

Never silence a diagnostic by weakening authority or trust: do not add capabilities to `main`, call `.assume_trusted()`, or call `.expose()` only to make a message go away. Ask first when the task seems to need it.

## Things an agent should know

- **Untrusted text.** Anything from `Args`, files, standard input, or the environment is untrusted. A `String` parameter is trusted. See [untrusted data and secrets](untrusted-data.md).
- **Ignoring a `Result` is an error** (`E0240`). Write `let _ = f()` to discard one on purpose.
- **Mutation** needs `var` on the binding, and `var self` on the method.
- **Newlines end statements.** There is no `;`.
- **Interpolation holds only a name or a field path.** Compute with `let` first.
- **Contracts are enforced.** `requires` and `ensures` run on every call, in every build. `P0006` points at the call that broke a `requires`, so fix the caller. `P0007` points at the `ensures` clause, so fix the function. A trait method's contract is declared on the trait, not on its impls (`E0218`), and not on `main` (`E0219`).
- **Known gaps in this release:** a module-level `const` cannot be referenced, a type misspelled in a `let` annotation is not reported, and `Net`, `std.sql`, and `std.proc` check but do not run. See [troubleshooting](troubleshooting.md#by-symptom).
- **Packages.** A program can be one file or a package: an `oska.toml` and modules under `src/`, imported with `use pkg.a.b`. Give `oskac` the package directory (or no path, in it). Items are private to their module until marked `pub(pkg)` or `pub`; `E0206`'s fix does it. There are no dependencies, re-exports or renaming yet. See [packages and modules](packages.md).
- **`dbg` writes to standard error** and returns its argument.

## Telling an agent about the project

Put the loop in the project's instructions file, such as `AGENTS.md`:

```markdown
## Working with Oska

- Check: `oskac check <file.osk> --format=json`. Apply a diagnostic's `fix` if it has one, then check again.
- Run: `oskac run <file.osk> -- args`. Test: `oskac test <file.osk>`.
- Before finishing: `oskac check <file.osk> --release` must pass, and read the authority report. Do not widen `main`'s capabilities or add `.assume_trusted()` or `.expose()` without saying why.
- Match on the diagnostic `code`, not the message. `oskac explain <code>` explains a code offline, with an example and its fix. The same entries are at `https://oskalang.dev/codes/<code>` and in `https://oskalang.dev/codes.json`.
```

## Files for agents

These are published on oskalang.dev and are produced from the same docs as the pages you read.

- **`llms.txt` and Markdown copies:** `https://oskalang.dev/llms.txt` lists the pages and `https://oskalang.dev/llms-full.txt` holds all of them in one file. Every page also has a Markdown copy at the same address with `.md` in place of `.html`.
- **The installers' `--json` output** (see [install](install.md#one-line-installers)), so an agent can confirm that the checksum and signature verified.
- **A copy-and-paste agent setup prompt** that installs Oska and configures a project with the loop above, at `https://oskalang.dev/agent-setup.md`. Along with it are an `AGENTS.md` template, a `CLAUDE.md` template, and an opt-in Claude Code hook that runs `oskac check --format=json` after an edit to a `.osk` file. The hook is a POSIX shell script, so it works on Linux and macOS only. It is not available on Windows yet.
