# Agent guide

How an AI coding agent, or any script, should drive `oskac`. Oska is built for this loop: every message has a stable code and a precise span, many carry a fix a tool can apply, and the authority report says what a change lets the program do.

## The loop

1. **Check.** `oskac check file.osk --format=json`. Read `diagnostics`.
2. **Apply machine fixes.** `oskac fix file.osk` applies every fix that is certain and checks again, until none is left. Read what it printed.
3. **Fix the rest by hand,** choosing by `code`, one diagnostic at a time, first one first.
4. **Run.** `oskac run file.osk --format=json -- args...`. The exit code and standard error say how it ended. On Linux and macOS the program runs natively, on Windows on the interpreter, with the same output; if a native result looks wrong, run it again with `--interpret` and report any difference.
5. **Test.** `oskac test file.osk --format=json`. Write a test for every behavior you add.
6. **Compare the authority.** `oskac check file.osk --baseline=authority.json` compares the program with the project's saved authority and exits with 3 when it widened. A change that widens authority, or adds an audit site, is not finished until you can say why.
7. **Release check.** `oskac check file.osk --release` must pass: no `dbg(...)` and no `todo()` left.

You are done when the check, the tests, and the release check all exit with 0, and the authority report is no wider than the task needs.

## Machine-readable outputs

| Command | Output with `--format=json` | Where |
| --- | --- | --- |
| `oskac check` | One object: `diagnostics`, `functions`, `audit`, `attenuations`, and with `--baseline`, `authority_diff` | standard output |
| `oskac fix` | One object: `applied`, `files`, `diagnostics`, `rounds`, `written`, and with `--dry-run`, `diff` | standard output |
| `oskac test` | One object: `tests`, `passed`, `failed`, `skipped`, `diagnostics` | standard output |
| `oskac run` | The program's own output, unchanged. Diagnostics and panics are one JSON object per line. | standard error |
| `oskac build` | One object: `output` (the executable's path, or `null`), `diagnostics`. With `--target all`, `outputs` (a list of paths) replaces `output` on success. | standard output |
| `oskac parse` | The AST, documented in the [AST reference](../ast.md) | standard output |
| `oskac lex` | Tokens (the same text as without the flag). A lexical error is a diagnostic object. | standard output |
| `oskac --version` | One object: `name`, `version` | standard output |

Every diagnostic has the same shape: `code`, `message`, `span` with its `line` and `column`, `severity`, `url`, `related` places, and sometimes `fix`, whose `applicability` says whether it is certain (`machine`), a guess (`maybe`), or leaves `todo()` to fill in (`placeholders`). Match on `code`. The shapes and exit codes are in the [CLI reference](cli-reference.md#output-formats). If `oskac` cannot read the file it prints `error: cannot read ...` to standard error, exits with 1, and prints no JSON, so check for empty standard output before parsing.

## Building for a platform

`oskac build` writes an executable that runs without `oskac`, on any host, for `linux-aarch64`, `linux-x86_64`, `macos-aarch64` or `macos-x86_64`. Without `--target` it builds for the host's platform when that has a backend (so on Linux and macOS the result runs on the machine that built it), else for `linux-aarch64`. A macOS executable is ad-hoc signed, which is enough to run it on a Mac but not to pass Gatekeeper after a download: see [Signing and notarizing for macOS](macos-signing.md). To build for another machine, name the platform with `--target <platform>`, using the same names as the installers, `latest.json` and the release assets. `--target all` writes one executable per platform, named `<name>-<platform>`, into the directory `-o` names:

```sh
oskac build app.osk --target linux-x86_64 -o app-x86_64
oskac build app.osk --target all -o dist --format=json
```

The second prints `{"outputs":["dist/app-linux-aarch64","dist/app-linux-x86_64"],"diagnostics":[]}`. The same program built by the same `oskac` gives the same bytes on every host.

`--target` is refused, with exit code 2 and nothing written, in two cases. Match on the `code`:

- `E0393`: the name is not a platform. The message lists the valid names (`linux-aarch64`, `linux-x86_64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`, and `all`). Fix the typo.
- `E0394`: the platform is supported but has no native backend yet. Every supported platform has one now, so this is not reported; the code is reserved for a platform added later.

Only `oskac build` takes `--target`: `run` and `test` always use the host's platform. See the [CLI reference](cli-reference.md#oskac-build).

## Applying fixes

`oskac fix [path]` applies fixes for you, so that you do not write the loop or pay a tool call per fix. It checks the program, applies the fix of the first diagnostic that has a certain one, checks again, and repeats until none is left. It prints one line per fix it applied (`path:line:column: fixed CODE: message`), then the diagnostics that remain, and exits with 0 when none remain. `--format=json` prints the same as one object, and `--dry-run` prints the diff the fixes would make and writes nothing.

```sh
oskac fix file.osk --dry-run
oskac fix file.osk
```

What it does and does not apply:

- It applies a fix whose `applicability` is `machine`, which is certainly right. It never applies a `maybe` fix, such as the closest name to an unknown one: check that it is what was meant, then edit by hand.
- `oskac fix file.osk --placeholders` also applies fixes whose `applicability` is `placeholders`. The fix for a non-exhaustive `match` (`E0253`) adds arms whose body is `todo()`, which compiles but panics with `P0003` when reached, and fails `--release`. Fill them in.
- A missing `fix` means the repair needs judgment. Do not guess one from the message.
- It writes each changed file once, at the end, and writes nothing if a file changed while it ran, if a fix would edit a file outside the program or through a symlink, or if the fixed program would have wider authority than before. Each refusal is an `error:` that names the rule, with exit code 1.
- It stops when a fix does not remove its own diagnostic, so that a faulty fix cannot loop forever.

A tool that applies fixes itself, and not through `oskac fix`, reads the `fix` of a diagnostic: a message and a list of `edits`. Each edit replaces the bytes `span.start` up to `span.end` with `replacement`. The offsets count UTF-8 bytes in the file as it was checked, so apply one fix and check again, apply the edits of one fix from the end of the file to the start, work on bytes and not characters, and read `applicability` before applying a fix that is not `machine`.

An example. The file declares a struct that holds a capability without saying so:

```oska error=E0210
struct Logger { out: Stdout }
```

The diagnostic has the fix `{"message":"declare it `cap struct`","edits":[{"span":{"start":0,"end":0},"replacement":"cap "}],"applicability":"machine"}`. Applying it, an insertion at byte 0, gives a program that checks:

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
  ],
  "verdict": "ok"
}
```

Before you finish a task, compare the report with the one from before your change. Do not do it by hand: with a project's committed baseline, which is the report without places, `oskac check file.osk --baseline=authority.json` does it and says which functions gained what, which audit sites and attenuations are new, and which functions stopped being pure.

```sh
oskac check file.osk --baseline=authority.json
```

It fails with exit code 3 when authority widened, a function became impure, an audit site was added, or a new function reaches a capability no function had; narrowing, removals and anything in a test block are reported and never fail. Without a committed baseline, save the report before you change anything and compare with that: `--baseline` accepts a saved `oskac check --format=json` report too.

```sh
oskac check file.osk --format=json > before.json
oskac check file.osk --baseline=before.json
```

A project writes its baseline with `oskac check file.osk --update-baseline=authority.json`, commits it, and runs `oskac check file.osk --baseline=authority.json` in CI. There is no flag that lets a widening pass: approving one is a change to the committed baseline in the same pull request, a diff a person reviews. Do not refresh a committed baseline to make the gate pass without telling the user why.

What the comparison reports, and what to ask yourself about each:

- **A function's `authority` grew,** or a new capability appears in `main`'s signature. Is that the task, or a shortcut? Prefer passing a narrower capability, such as `fs.restrict_to("dir").read_only()`, to adding a wider one.
- **A new `audit` entry appeared.** Every `assume_trusted` and `expose` weakens a guarantee. Use one only inside a validator that really checks the text, and say so in your summary.
- **A new `attenuations` entry has a wider root than the task needs.** Each one names the function it is in, the capability it narrows from and to, and its `root`. A `root` of `"/"` is the whole filesystem, and `null` means the root is computed at runtime (or, for `read_only` and `write_only`, that there is none).
- **A function that was `pure` is not.** It now has a capability-bearing parameter, or one of a bounded type parameter (`generic T`) that it can perform effects through.

Never silence a diagnostic by weakening authority or trust: do not add capabilities to `main`, call `.assume_trusted()`, or call `.expose()` only to make a message go away. Ask first when the task seems to need it.

## Hooks

Claude Code can run `oskac` for you at two points. Both are opt-in, work the same on Linux, macOS and Windows, and are in the agent setup prompt's choices.

- **After an edit.** `oskac hook claude --event=post-edit` checks the package of the `.osk` file or `oska.toml` that was just edited and, when there are diagnostics, hands back one line each (`path:line:column: error[CODE]: message`), at most 20, and says how many have a `machine` fix and to run `oskac fix`. It prints nothing when the program checks. It exits with 2 to hand the lines back, and with 1 and one line when its input is malformed. With `--format=json`, either hook hands back one JSON object instead, each diagnostic in the shape `oskac check --format=json` gives it ([CLI reference](cli-reference.md#oskac-hook)).
- **Before finishing.** `oskac hook claude --event=stop` runs the release check and then the tests, without `--integration`, for the project's programs, and exits with 2 to block the agent from finishing while a diagnostic or a test fails. Targets named after the event limit it to those programs, for a project with `.osk` files that are broken on purpose. A program that uses something the interpreter cannot run yet does not block it. A target it cannot read does, and so does a `.osk` file that is a symlink: the gate never lets an agent finish on what it could not check.

The settings call `oskac` by name, with no script and no shell syntax, so `oskac` must be on `PATH`. The old script's fallback to `$OSKA_HOME/bin/oskac` is gone: add the installer's `bin_dir` to `PATH`. See the [CLI reference](cli-reference.md#oskac-hook) for the exact behavior.

## Things an agent should know

- **Untrusted text.** Anything from `Args`, files, standard input, or the environment is untrusted. A `String` parameter is trusted. See [untrusted data and secrets](untrusted-data.md).
- **Ignoring a `Result` is an error** (`E0240`). Write `let _ = f()` to discard one on purpose.
- **Mutation** needs `var` on the binding, and `var self` on the method.
- **Newlines end statements.** There is no `;`.
- **Interpolation holds only a name or a field path.** Compute with `let` first. In a method the path may start with `self`, as in `"{self.balance}"`.
- **Contracts are enforced.** `requires` and `ensures` run on every call, in every build. `P0006` points at the call that broke a `requires`, so fix the caller. `P0007` points at the `ensures` clause, so fix the function. A trait method's contract is declared on the trait, not on its impls (`E0218`), and not on `main` (`E0219`).
- **Known gaps in this release:** `Net`, `std.sql`, and `std.proc` check but do not run. See [troubleshooting](troubleshooting.md#by-symptom).
- **Packages.** A program can be one file or a package: an `oska.toml` and modules under `src/`, imported with `use pkg.a.b`. Give `oskac` the package directory (or no path, in it). Items are private to their module until marked `pub(pkg)` or `pub`; `E0206`'s fix does it. There are no dependencies, re-exports or renaming yet. See [packages and modules](packages.md).
- **`dbg` writes to standard error** and returns its argument.

## Telling an agent about the project

Put the loop in the project's instructions file, such as `AGENTS.md`:

```markdown
## Working with Oska

- Check: `oskac check <file.osk> --format=json`. Apply the certain fixes with `oskac fix <file.osk>` (`--dry-run` shows the diff first), and the rest by hand.
- Run: `oskac run <file.osk> -- args`. Test: `oskac test <file.osk>`. Build an executable: `oskac build <file.osk>`, and for another platform `--target linux-x86_64` or `--target linux-aarch64` (`--target all` for both).
- Before finishing: `oskac check <file.osk> --release` must pass, and `oskac check <file.osk> --baseline=authority.json` must not widen authority (exit code 3). Do not widen `main`'s capabilities or add `.assume_trusted()` or `.expose()` without saying why, and do not refresh `authority.json` to make the gate pass without telling the user why.
- Match on the diagnostic `code`, not the message. `oskac explain <code>` explains a code offline, with an example and its fix. The same entries are at `https://oskalang.dev/codes/<code>` and in `https://oskalang.dev/codes.json`.
```

## Files for agents

These are published on oskalang.dev and are produced from the same docs as the pages you read.

- **`llms.txt` and Markdown copies:** `https://oskalang.dev/llms.txt` lists the pages, with the ones to read first under "Start here" and the ones you can skip under "Optional". `https://oskalang.dev/llms-full.txt` holds the documentation in one file, in reading order, without the per-code pages (those are in `https://oskalang.dev/codes.json`) and without the research, which is proposals and not documented behaviour (that is in `https://oskalang.dev/llms-research.txt`). Every page also has a Markdown copy at the same address with `.md` in place of `.html`.
- **The installers' `--json` output** (see [install](install.md#one-line-installers)), so an agent can confirm that the checksum and signature verified.
- **A copy-and-paste agent setup prompt** that installs Oska and configures a project with the loop above, at `https://oskalang.dev/agent-setup.md`. Along with it are an `AGENTS.md` template, a `CLAUDE.md` template, and the settings for the two opt-in Claude Code [hooks](#hooks), which call `oskac hook claude`. The older shell script `oska-check.sh` is still published, as a one-line call to the same command, for a project that saved it earlier.
